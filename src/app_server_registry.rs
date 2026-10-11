use crate::{
    app_server_wire::decode_wire_line,
    backend::{BackendSnapshot, BackendStatus},
    codex_protocol::{ThreadWire, apply_status, fallback_workspace_from_cwd, normalize_thread},
    domain::ThreadSummary,
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn by_id(threads: Vec<ThreadSummary>) -> BTreeMap<String, ThreadSummary> {
    threads
        .into_iter()
        .map(|thread| (thread.id.0.clone(), thread))
        .collect()
}

pub(crate) fn snapshot(
    generation: u64,
    threads: &BTreeMap<String, ThreadSummary>,
    status: &BackendStatus,
) -> BackendSnapshot {
    let mut ordered = threads.values().cloned().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        right
            .metadata
            .updated_at
            .cmp(&left.metadata.updated_at)
            .then_with(|| right.id.0.cmp(&left.id.0))
    });
    BackendSnapshot {
        generation,
        threads: ordered,
        status: status.clone(),
    }
}

pub(crate) fn replay_registry_jsonl(input: &str) -> Result<Vec<BackendSnapshot>> {
    let mut threads = BTreeMap::new();
    let status = BackendStatus {
        source: "replay-app-server".into(),
        connected: true,
        version: Some("fixture".into()),
        platform: None,
        codex_home: None,
        capabilities: vec!["registry-replay".into()],
        optional_capabilities_missing: vec![],
        registry_complete: true,
        last_refresh_unix_ms: None,
        error: None,
    };
    let mut generation = 0_u64;
    let mut frames = vec![snapshot(generation, &threads, &status)];

    for raw in input.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let message = decode_wire_line(line)?;
        let emitted_at_seconds = message
            .get("emittedAtMs")
            .and_then(Value::as_u64)
            .and_then(|millis| i64::try_from(millis / 1_000).ok());
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            continue;
        };
        let Some(params) = message.get("params") else {
            continue;
        };

        if apply_registry_notification(method, params, emitted_at_seconds, &mut threads)?
            == Some(true)
        {
            generation = generation.saturating_add(1);
            frames.push(snapshot(generation, &threads, &status));
        }
    }

    Ok(frames)
}

pub(crate) fn apply_registry_notification(
    method: &str,
    params: &Value,
    emitted_at_seconds: Option<i64>,
    threads: &mut BTreeMap<String, ThreadSummary>,
) -> Result<Option<bool>> {
    if method == "thread/started" {
        let wire: ThreadWire = serde_json::from_value(
            params
                .get("thread")
                .cloned()
                .context("thread/started notification missing thread")?,
        )
        .context("decode thread/started thread")?;
        let mut summary = normalize_thread(wire, None);
        summary.metadata.loaded = Some(true);
        threads.insert(summary.id.0.clone(), summary);
        return Ok(Some(true));
    }

    let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
        return Ok(None);
    };

    match method {
        "thread/status/changed" => {
            let Some(status) = params.get("status") else {
                return Ok(Some(false));
            };
            let Some(thread) = threads.get_mut(thread_id) else {
                return Ok(Some(false));
            };
            apply_status(thread, status);
            thread.metadata.loaded = match status.get("type").and_then(Value::as_str) {
                Some("notLoaded") => Some(false),
                Some("active" | "idle") => Some(true),
                _ => thread.metadata.loaded,
            };
            if let Some(updated_at) = emitted_at_seconds {
                thread.metadata.updated_at = thread.metadata.updated_at.max(updated_at);
            }
            Ok(Some(true))
        }
        "thread/archived" | "thread/deleted" => Ok(Some(threads.remove(thread_id).is_some())),
        "thread/unarchived" => Ok(Some(false)),
        "thread/name/updated" => {
            let name = match params.get("threadName") {
                None | Some(Value::Null) => None,
                Some(Value::String(value)) => Some(value.trim()).filter(|name| !name.is_empty()),
                _ => return Ok(Some(false)),
            };
            let Some(thread) = threads.get_mut(thread_id) else {
                return Ok(Some(false));
            };
            // A cleared name has no preview in this notification. Show the
            // stable ID until an authoritative thread/list read restores it.
            thread.title = name.unwrap_or(thread_id).to_string();
            if let Some(updated_at) = emitted_at_seconds {
                thread.metadata.updated_at = thread.metadata.updated_at.max(updated_at);
            }
            Ok(Some(true))
        }
        "thread/project/updated" => {
            let project_id = match params.get("projectId") {
                Some(Value::Null) => None,
                Some(Value::String(value)) => Some(value.trim()).filter(|value| !value.is_empty()),
                _ => return Ok(Some(false)),
            };
            let Some(thread) = threads.get_mut(thread_id) else {
                return Ok(Some(false));
            };
            thread.metadata.project_id = project_id.map(str::to_string);
            let (workspace, key, basis) = if let Some(project_id) = project_id {
                (
                    format!("project:{project_id}"),
                    format!("project:{project_id}"),
                    "codex-project".into(),
                )
            } else {
                fallback_workspace_from_cwd(&thread.metadata.cwd)
            };
            thread.workspace = workspace;
            thread.metadata.workspace_key = key;
            thread.metadata.workspace_basis = basis;
            if let Some(updated_at) = emitted_at_seconds {
                thread.metadata.updated_at = thread.metadata.updated_at.max(updated_at);
            }
            Ok(Some(true))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn start(threads: &mut BTreeMap<String, ThreadSummary>) {
        apply_registry_notification(
            "thread/started",
            &json!({"thread": {
                "id": "thread-a", "preview": "First prompt", "projectId": "project-A",
                "updatedAt": 1, "cwd": "/work/robot",
                "status": {"type": "idle"}, "source": "cli"
            }}),
            Some(1),
            threads,
        )
        .expect("thread started");
    }

    #[test]
    fn upstream_project_unassignment_clears_old_project_without_inventing_git_origin() {
        let mut threads = BTreeMap::new();
        start(&mut threads);
        assert_eq!(threads["thread-a"].workspace, "project:project-A");
        assert_eq!(
            apply_registry_notification(
                "thread/project/updated",
                &json!({"threadId": "thread-a", "projectId": null}),
                Some(2),
                &mut threads
            )
            .expect("null project notification"),
            Some(true)
        );
        let detached = &threads["thread-a"];
        assert_eq!(detached.metadata.project_id, None);
        assert_eq!(detached.workspace, "robot");
        assert_eq!(detached.metadata.workspace_key, "cwd:/work/robot");
        assert_eq!(detached.metadata.workspace_basis, "cwd");
        assert_eq!(detached.metadata.updated_at, 2);

        apply_registry_notification(
            "thread/project/updated",
            &json!({"threadId": "thread-a", "projectId": "project-B"}),
            Some(3),
            &mut threads,
        )
        .expect("project B");
        assert_eq!(threads["thread-a"].workspace, "project:project-B");
        assert_eq!(
            threads["thread-a"].metadata.project_id.as_deref(),
            Some("project-B")
        );
        assert_eq!(threads["thread-a"].metadata.updated_at, 3);
    }

    #[test]
    fn cleared_thread_name_uses_safe_id_until_authoritative_refresh() {
        let mut threads = BTreeMap::new();
        start(&mut threads);
        apply_registry_notification(
            "thread/name/updated",
            &json!({"threadId": "thread-a", "threadName": "Custom"}),
            Some(2),
            &mut threads,
        )
        .expect("set name");
        assert_eq!(threads["thread-a"].title, "Custom");
        apply_registry_notification(
            "thread/name/updated",
            &json!({"threadId": "thread-a"}),
            Some(3),
            &mut threads,
        )
        .expect("clear name");
        assert_eq!(threads["thread-a"].title, "thread-a");
        assert_eq!(threads["thread-a"].metadata.updated_at, 3);
    }

    #[test]
    fn malformed_project_event_does_not_clear_live_identity() {
        let mut threads = BTreeMap::new();
        start(&mut threads);
        assert_eq!(
            apply_registry_notification(
                "thread/project/updated",
                &json!({"threadId": "thread-a", "projectId": 42}),
                None,
                &mut threads,
            )
            .expect("invalid event is ignored"),
            Some(false)
        );
        assert_eq!(threads["thread-a"].workspace, "project:project-A");
    }
}
