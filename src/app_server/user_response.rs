//! Validate the observed form before I/O; quarantine any attempted write on failure.
use super::*;
use crate::user_response::{UserResponseOutcome, UserResponseSubmission};
use std::future::Future;
pub(super) async fn send(
    rpc: &mut RpcSession,
    pending: &mut BTreeMap<RpcRequestId, PendingServerRequest>,
    threads: &BTreeMap<String, ThreadSummary>,
    submission: UserResponseSubmission,
) -> UserResponseOutcome {
    if queued_invalidates(&rpc.queued_messages, &submission) {
        return UserResponseOutcome::NotSent(
            "newer request or thread event awaits processing; answer retained".into(),
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
        return UserResponseOutcome::NotSent("input request is no longer pending".into());
    };
    let message = json!({"id": id.to_value(), "method": current.method, "params": current.params});
    if parse_interactive_request(&message).ok().flatten().as_ref() != Some(&submission.request)
        || !threads.contains_key(&submission.request.thread_id.0)
    {
        return UserResponseOutcome::NotSent(
            "input request changed or its thread disappeared".into(),
        );
    }
    let crate::conversation::InteractiveRequestKind::UserInput { questions } =
        &submission.request.kind
    else {
        return UserResponseOutcome::NotSent("not a user-input form".into());
    };
    if submission.request.validate_user_input().is_err()
        || submission.answers.len() != questions.len()
        || questions
            .iter()
            .any(|q| submission.answers.get(&q.id).is_none_or(|a| a.is_empty()))
    {
        return UserResponseOutcome::NotSent("answer keys do not match the observed form".into());
    }
    let answers = submission
        .answers
        .into_iter()
        .map(|(id, answers)| (id, json!({"answers": answers})))
        .collect::<serde_json::Map<_, _>>();
    let response = json!({"id": id.to_value(), "result": {"answers": answers}});
    // From this point a partial write is possible. Never leave the old request
    // retryable merely because flush/timeout failed; local UI also fences retries.
    pending.remove(id);
    match with_rpc_deadline("user-input response", deadline, write(response)).await {
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
