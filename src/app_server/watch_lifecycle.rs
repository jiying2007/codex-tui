//! An explicit removal retires subscriptions even before registry hydration.
use super::{ConversationEvent, WatchSet, send_conversation_event};
use crate::domain::ThreadId;
use serde_json::Value;
use tokio::sync::mpsc;

pub(super) async fn retire_removed_watches(
    method: &str,
    params: &Value,
    watched_threads: &WatchSet,
    watched_queue_threads: &WatchSet,
    conversation_tx: &mpsc::Sender<ConversationEvent>,
) {
    if !matches!(method, "thread/archived" | "thread/deleted") {
        return;
    }
    let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
        return;
    };
    // Clear shared actor ownership before awaiting bounded event delivery.
    // Removal's return value is taken under the membership lock, not a
    // separate contains/remove check. Queued reads cannot reinstate the owner.
    // Registry removal alone is a no-op for a provisional search target.
    // There is no tombstone set and duplicate notifications emit nothing.
    let history = watched_threads.remove(thread_id);
    let queue = watched_queue_threads.remove(thread_id);
    if history {
        send_conversation_event(
            conversation_tx,
            ConversationEvent::Failed {
                thread_id: ThreadId::new(thread_id),
                error: "conversation archived or deleted; history watch stopped".into(),
            },
        )
        .await;
    }
    if queue {
        send_conversation_event(
            conversation_tx,
            ConversationEvent::ThreadQueueFailed {
                thread_id: ThreadId::new(thread_id),
                error: "conversation archived or deleted; queue watch stopped".into(),
            },
        )
        .await;
    }
}

#[cfg(test)]
mod tests;
