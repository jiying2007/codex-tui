use crate::{
    app_server_wire::decode_wire_line,
    backend::{BackendSnapshot, BackendStatus},
    codex_protocol::{ThreadWire, apply_status, normalize_thread},
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
            let Some(thread) = threads.get_mut(thread_id) else {
                return Ok(Some(false));
            };
            let Some(name) = params
                .get("threadName")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
            else {
                return Ok(Some(false));
            };
            thread.title = name.to_string();
            if let Some(updated_at) = emitted_at_seconds {
                thread.metadata.updated_at = thread.metadata.updated_at.max(updated_at);
            }
            Ok(Some(true))
        }
        "thread/project/updated" => {
            let Some(thread) = threads.get_mut(thread_id) else {
                return Ok(Some(false));
            };
            let Some(project_id) = params
                .get("projectId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|project_id| !project_id.is_empty())
            else {
                return Ok(Some(false));
            };
            thread.metadata.project_id = Some(project_id.to_string());
            thread.workspace = format!("project:{project_id}");
            thread.metadata.workspace_key = format!("project:{project_id}");
            thread.metadata.workspace_basis = "codex-project".into();
            if let Some(updated_at) = emitted_at_seconds {
                thread.metadata.updated_at = thread.metadata.updated_at.max(updated_at);
            }
            Ok(Some(true))
        }
        _ => Ok(None),
    }
}

