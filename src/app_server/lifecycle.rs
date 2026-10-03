use super::{
    ConversationEvent, RpcResponseError, RpcSession, mark_optional_capability,
    send_conversation_event,
};
use crate::app_server_registry::snapshot;
use crate::backend::BackendStatus;
use crate::codex_protocol::{ThreadWire, normalize_thread};
use crate::domain::{ThreadId, ThreadSummary};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio::sync::{mpsc, watch};

fn is_method_unsupported(error: &anyhow::Error, method: &str) -> bool {
    let Some(source) = error.downcast_ref::<RpcResponseError>() else {
        return false;
    };
    source.method == method && matches!(source.code, Some(-32602..=-32600))
}

fn parse_lifecycle_thread(result: Value, operation: &str) -> Result<ThreadSummary> {
    let value = result
        .get("thread")
        .cloned()
        .with_context(|| format!("{operation} response missing thread"))?;
    let thread: ThreadWire =
        serde_json::from_value(value).with_context(|| format!("decode {operation} thread"))?;
    Ok(normalize_thread(thread, None))
}

async fn start_thread(rpc: &mut RpcSession, cwd: String) -> Result<ThreadSummary> {
    anyhow::ensure!(!cwd.trim().is_empty(), "thread/start cwd is empty");
    let result = rpc
        .request("thread/start", json!({ "cwd": cwd }))
        .await
        .context("start Codex thread")?;
    parse_lifecycle_thread(result, "thread/start")
}

async fn fork_thread(rpc: &mut RpcSession, thread_id: ThreadId) -> Result<ThreadSummary> {
    let result = rpc
        .request(
            "thread/fork",
            json!({
                "threadId": thread_id.0,
                "excludeTurns": true
            }),
        )
        .await
        .context("fork Codex thread")?;
    parse_lifecycle_thread(result, "thread/fork")
}

async fn publish_success(
    threads: &mut BTreeMap<String, ThreadSummary>,
    status: &mut BackendStatus,
    generation: &mut u64,
    tx: &watch::Sender<crate::backend::BackendSnapshot>,
    conversation_tx: &mpsc::Sender<ConversationEvent>,
    thread: ThreadSummary,
    operation: &'static str,
) {
    let thread_id = thread.id.clone();
    threads.insert(thread_id.0.clone(), thread);
    mark_optional_capability(status, operation, true);
    *generation = generation.saturating_add(1);
    let _ = tx.send(snapshot(*generation, threads, status));
    send_conversation_event(
        conversation_tx,
        ConversationEvent::ThreadCreated {
            thread_id,
            operation: operation.to_string(),
        },
    )
    .await;
}

async fn publish_failure(
    status: &mut BackendStatus,
    generation: &mut u64,
    threads: &BTreeMap<String, ThreadSummary>,
    tx: &watch::Sender<crate::backend::BackendSnapshot>,
    conversation_tx: &mpsc::Sender<ConversationEvent>,
    operation: &'static str,
    error: anyhow::Error,
) {
    if is_method_unsupported(&error, operation) {
        mark_optional_capability(status, operation, false);
        *generation = generation.saturating_add(1);
        let _ = tx.send(snapshot(*generation, threads, status));
    }
    send_conversation_event(
        conversation_tx,
        ConversationEvent::ThreadLifecycleFailed {
            operation: operation.to_string(),
            error: error.to_string(),
        },
    )
    .await;
}

pub(super) async fn handle_start_thread(
    rpc: &mut RpcSession,
    cwd: String,
    threads: &mut BTreeMap<String, ThreadSummary>,
    status: &mut BackendStatus,
    generation: &mut u64,
    tx: &watch::Sender<crate::backend::BackendSnapshot>,
    conversation_tx: &mpsc::Sender<ConversationEvent>,
) {
    match start_thread(rpc, cwd).await {
        Ok(thread) => {
            publish_success(
                threads,
                status,
                generation,
                tx,
                conversation_tx,
                thread,
                "thread/start",
            )
            .await;
        }
        Err(error) => {
            publish_failure(
                status,
                generation,
                threads,
                tx,
                conversation_tx,
                "thread/start",
                error,
            )
            .await;
        }
    }
}

pub(super) async fn handle_fork_thread(
    rpc: &mut RpcSession,
    thread_id: ThreadId,
    threads: &mut BTreeMap<String, ThreadSummary>,
    status: &mut BackendStatus,
    generation: &mut u64,
    tx: &watch::Sender<crate::backend::BackendSnapshot>,
    conversation_tx: &mpsc::Sender<ConversationEvent>,
) {
    match fork_thread(rpc, thread_id).await {
        Ok(thread) => {
            publish_success(
                threads,
                status,
                generation,
                tx,
                conversation_tx,
                thread,
                "thread/fork",
            )
            .await;
        }
        Err(error) => {
            publish_failure(
                status,
                generation,
                threads,
                tx,
                conversation_tx,
                "thread/fork",
                error,
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_method_codes_are_classified_fail_closed() {
        for code in [-32601, -32600, -32602] {
            let error: anyhow::Error = RpcResponseError {
                method: "thread/fork".into(),
                code: Some(code),
                message: "unsupported".into(),
            }
            .into();
            assert!(is_method_unsupported(&error, "thread/fork"));
            assert!(!is_method_unsupported(&error, "thread/start"));
        }
    }

    #[test]
    fn lifecycle_parser_requires_thread_identity() {
        assert!(parse_lifecycle_thread(json!({}), "thread/start").is_err());
    }
}
