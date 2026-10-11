//! Retained payload accounting, not an RSS measurement or an allocator guarantee.
use super::*;
pub(super) const RPC_RETAINED_BYTES: usize = 16 * 1024 * 1024;
const PENDING_COUNT: usize = 64;
const PENDING_REQUEST_BYTES: usize = 256 * 1024;
const PENDING_RETAINED_BYTES: usize = 4 * 1024 * 1024;

// Charge container/node overhead as well as strings; no second serialized allocation.
// Serde's bounded decode depth bounds recursion on wire-derived Values.
pub(super) fn json_cost(value: &Value) -> usize {
    let nested = match value {
        Value::String(text) => text.capacity(),
        Value::Array(items) => items.iter().fold(
            items
                .capacity()
                .saturating_mul(std::mem::size_of::<Value>()),
            |sum, item| sum.saturating_add(json_cost(item)),
        ),
        Value::Object(fields) => fields.iter().fold(1024usize, |sum, (key, value)| {
            sum.saturating_add(128)
                .saturating_add(key.capacity())
                .saturating_add(json_cost(value))
        }),
        _ => 0,
    };
    std::mem::size_of::<Value>().saturating_add(nested)
}

pub(super) fn enqueue_rpc_message(
    queue: &mut VecDeque<Value>,
    bytes: &mut usize,
    message: Value,
) -> Result<()> {
    anyhow::ensure!(
        queue.len() < RPC_QUEUED_MESSAGE_CAPACITY,
        "App Server queued-message capacity exceeded ({RPC_QUEUED_MESSAGE_CAPACITY})"
    );
    let retained = bytes
        .checked_add(json_cost(&message))
        .context("App Server queued-message byte overflow")?;
    anyhow::ensure!(
        retained <= RPC_RETAINED_BYTES,
        "App Server queued-message retained-byte budget exceeded ({RPC_RETAINED_BYTES})"
    );
    queue.push_back(message);
    *bytes = retained;
    Ok(())
}

pub(super) fn admit_pending_request(
    pending: &BTreeMap<RpcRequestId, PendingServerRequest>,
    message: &Value,
) -> Result<()> {
    let requested = json_cost(message);
    anyhow::ensure!(
        requested <= PENDING_REQUEST_BYTES,
        "App Server interactive request payload budget exceeded"
    );
    let id = RpcRequestId::from_value(message.get("id").context("server request missing id")?)?;
    anyhow::ensure!(
        pending.contains_key(&id) || pending.len() < PENDING_COUNT,
        "App Server pending-request capacity exceeded ({PENDING_COUNT})"
    );
    let retained =
        pending
            .iter()
            .filter(|(key, _)| *key != &id)
            .fold(requested, |sum, (key, request)| {
                sum.saturating_add(2048)
                    .saturating_add(json_cost(&key.to_value()))
                    .saturating_add(request.method.capacity())
                    .saturating_add(json_cost(&request.params))
            });
    anyhow::ensure!(
        retained <= PENDING_RETAINED_BYTES,
        "App Server pending-request retained-byte budget exceeded ({PENDING_RETAINED_BYTES})"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_budget_rejects_before_count_limit_and_release_is_accounted() {
        let mut queue = VecDeque::new();
        let mut bytes = 0;
        let payload = json!({"method":"turn/completed", "params":{"text":"x".repeat(256 * 1024)}});
        while enqueue_rpc_message(&mut queue, &mut bytes, payload.clone()).is_ok() {}
        assert!(!queue.is_empty() && queue.len() < RPC_QUEUED_MESSAGE_CAPACITY);
        assert!(bytes <= RPC_RETAINED_BYTES);
        let len = queue.len();
        let retained = bytes;
        assert!(enqueue_rpc_message(&mut queue, &mut bytes, payload.clone()).is_err());
        assert_eq!((queue.len(), bytes), (len, retained));
        let removed = queue.pop_front().unwrap();
        bytes -= json_cost(&removed);
        enqueue_rpc_message(&mut queue, &mut bytes, payload).unwrap();
        assert_eq!((queue.len(), bytes), (len, retained));
    }
    #[test]
    fn pending_count_payload_and_total_budgets_are_independent() {
        let mut pending = BTreeMap::new();
        let request = |id, text: &str| json!({"id":id, "method":"item/tool/requestUserInput", "params":{"text":text}});
        assert!(
            admit_pending_request(&pending, &request(1, &"x".repeat(PENDING_REQUEST_BYTES)))
                .is_err()
        );
        for id in 0..PENDING_COUNT as i64 {
            let value = request(id, "small");
            admit_pending_request(&pending, &value).unwrap();
            pending.insert(
                RpcRequestId::Integer(id),
                PendingServerRequest {
                    method: "item/tool/requestUserInput".into(),
                    params: value["params"].clone(),
                },
            );
        }
        assert!(admit_pending_request(&pending, &request(1000, "small")).is_err());
        assert!(admit_pending_request(&pending, &request(0, "replacement")).is_ok());
        for entry in pending.values_mut() {
            entry.params = json!({"text":"x".repeat(96 * 1024)});
        }
        assert!(
            admit_pending_request(&pending, &request(0, "replacement"))
                .unwrap_err()
                .to_string()
                .contains("retained-byte")
        );
        assert_eq!(pending.len(), PENDING_COUNT);
    }
}
