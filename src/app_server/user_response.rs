//! Validate the observed form before I/O; quarantine any attempted write on failure.
use super::*;
use crate::user_response::{UserResponseOutcome, UserResponseSubmission};
use std::future::Future;
#[cfg(test)]
mod approval_tests;
mod payload;
pub(super) async fn send(
    rpc: &mut RpcSession,
    pending: &mut BTreeMap<RpcRequestId, PendingServerRequest>,
    threads: &BTreeMap<String, ThreadSummary>,
    submission: UserResponseSubmission,
) -> UserResponseOutcome {
    if queued_invalidates(&rpc.queued_messages, &submission) {
        return UserResponseOutcome::NotSent(
            "newer request or thread event awaits processing; input retained".into(),
        );
    }
    send_with(
        pending,
        threads,
        submission,
        RPC_REQUEST_TIMEOUT,
        |message| async move { rpc.write_message(&message).await },
    )
    .await
}
async fn send_with<F, Fut>(
    pending: &mut BTreeMap<RpcRequestId, PendingServerRequest>,
    threads: &BTreeMap<String, ThreadSummary>,
    submission: UserResponseSubmission,
    deadline: Duration,
    write: F,
) -> UserResponseOutcome
where
    F: FnOnce(Value) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let id = &submission.request.request_id;
    let Some(current) = pending.get(id) else {
        return UserResponseOutcome::NotSent("interactive request is no longer pending".into());
    };
    let message = json!({"id": id.to_value(), "method": current.method, "params": current.params});
    if parse_interactive_request(&message).ok().flatten().as_ref() != Some(&submission.request)
        || !threads.contains_key(&submission.request.thread_id.0)
    {
        return UserResponseOutcome::NotSent(
            "interactive request changed or its thread disappeared".into(),
        );
    }
    let response = match payload::response(&submission, current) {
        Ok(response) => response,
        Err(error) => return UserResponseOutcome::NotSent(error.to_string()),
    };
    // From this point a partial write is possible. Never leave the old request
    // retryable merely because flush/timeout failed; local UI also fences retries.
    pending.remove(id);
    match with_rpc_deadline("interactive response", deadline, write(response)).await {
        Ok(()) => UserResponseOutcome::Written,
        Err(error) => UserResponseOutcome::Unknown(error.to_string()),
    }
}
fn queued_invalidates(queue: &VecDeque<Value>, submission: &UserResponseSubmission) -> bool {
    let id = submission.request.request_id.to_value();
    queue.iter().any(|message| {
        let method = message.get("method").and_then(Value::as_str);
        (method.is_some() && message.get("id") == Some(&id))
            || (method == Some("serverRequest/resolved")
                && message.pointer("/params/requestId") == Some(&id))
            || (matches!(
                method,
                Some("thread/archived" | "thread/deleted" | "thread/unarchived")
            ) && message.pointer("/params/threadId").and_then(Value::as_str)
                == Some(submission.request.thread_id.0.as_str()))
    })
}
#[cfg(test)]
mod tests;
