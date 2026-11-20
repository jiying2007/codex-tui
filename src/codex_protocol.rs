use crate::domain::{
    AttentionReason, RuntimeStatus, ThreadId, ThreadLineage, ThreadMetadata, ThreadSummary,
};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use url::Url;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListPage {
    pub data: Vec<ThreadWire>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadLoadedPage {
    pub data: Vec<String>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadWire {
    pub id: String,
    #[serde(flatten)]
    pub lineage: ThreadLineage,
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub recency_at: Option<i64>,
    #[serde(default)]
    pub status: Value,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub source: Value,
    #[serde(default)]
    pub git_info: Option<GitInfoWire>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInfoWire {
    #[serde(default)]
    pub origin_url: Option<String>,
}

pub fn normalize_thread(thread: ThreadWire, loaded: Option<&BTreeSet<String>>) -> ThreadSummary {
    let (runtime, attention) = normalize_status(&thread.status);
    let title = thread
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| first_nonempty_line(&thread.preview))
        .unwrap_or_else(|| thread.id.clone());
    let (workspace, workspace_key, workspace_basis) = workspace_for(&thread);
    let source = normalize_source(&thread.source);
    let is_loaded = loaded.map(|ids| ids.contains(&thread.id));

    ThreadSummary {
        id: ThreadId::new(thread.id),
        workspace,
        title,
        runtime,
        attention,
        pinned: false,
        alias: None,
        metadata: ThreadMetadata {
            lineage: thread.lineage,
            cwd: thread.cwd,
            model: thread.model,
            project_id: thread.project_id,
            source,
            updated_at: thread.recency_at.unwrap_or(thread.updated_at),
            loaded: is_loaded,
            workspace_key,
            workspace_basis,
        },
    }
}

pub fn apply_status(summary: &mut ThreadSummary, status: &Value) {
    let (runtime, mut attention) = normalize_status(status);
    if summary.attention.contains(&AttentionReason::MarkedUnread) {
        attention.push(AttentionReason::MarkedUnread);
    }
    summary.runtime = runtime;
    summary.attention = attention;
}

pub fn parse_thread_list(value: Value) -> Result<ThreadListPage> {
    serde_json::from_value(value).context("decode thread/list response")
}

pub fn parse_loaded_list(value: Value) -> Result<ThreadLoadedPage> {
    serde_json::from_value(value).context("decode thread/loaded/list response")
}

fn normalize_status(status: &Value) -> (RuntimeStatus, Vec<AttentionReason>) {
    let kind = status
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    match kind {
        "active" => {
            let flags = status
                .get("activeFlags")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            let mut attention = Vec::new();
            if flags.contains(&"waitingOnApproval") {
                attention.push(AttentionReason::ApprovalRequired);
            }
            if flags.contains(&"waitingOnUserInput") {
                attention.push(AttentionReason::UserInputRequired);
            }
            let runtime = if attention.is_empty() {
                RuntimeStatus::Working
            } else {
                RuntimeStatus::WaitingHuman
            };
            (runtime, attention)
        }
        "idle" => (RuntimeStatus::Ready, vec![]),
        "systemError" => (
            RuntimeStatus::SystemError,
            vec![AttentionReason::SystemError],
        ),
        "notLoaded" => (RuntimeStatus::Inactive, vec![]),
        _ => (RuntimeStatus::Unknown, vec![]),
    }
}

fn workspace_for(thread: &ThreadWire) -> (String, String, String) {
    if let Some(project_id) = thread
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return (
            format!("project:{project_id}"),
            format!("project:{project_id}"),
            "codex-project".into(),
        );
    }

    if let Some(origin) = thread
        .git_info
        .as_ref()
        .and_then(|git| git.origin_url.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && let Some(key) = normalize_git_origin(origin)
    {
        return (repo_label(&key), format!("git:{key}"), "git-origin".into());
    }

    fallback_workspace_from_cwd(&thread.cwd)
}

/// A project-unassignment event carries no Git origin. Use the known cwd
/// until the authoritative thread/list refresh can recover its full metadata.
pub(crate) fn fallback_workspace_from_cwd(cwd: &str) -> (String, String, String) {
    let cwd = cwd.trim();
    let label = path_label(cwd).unwrap_or_else(|| cwd.to_string());
    (label, format!("cwd:{cwd}"), "cwd".into())
}

/// Normalize a remote as host/repository, never retaining URL userinfo, query
/// or fragment bytes in a display name, search field or headless workspace key.
fn normalize_git_origin(origin: &str) -> Option<String> {
    let origin = origin.trim();
    let (host, path) = if origin.contains("://") {
        let url = Url::parse(origin).ok()?;
        if !matches!(url.scheme(), "https" | "http" | "ssh" | "git") {
            return None;
        }
        let host = url.host_str()?.to_string();
        let host = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host,
        };
        (host, url.path().to_string())
    } else {
        // Git's SCP-style user@host:namespace/repo.git remote syntax.
        let (authority, path) = origin.split_once(':')?;
        let host = authority.rsplit('@').next()?.trim();
        if host.is_empty()
            || host
                .chars()
                .any(|ch| matches!(ch, '/' | '\\' | '?' | '#' | ' '))
        {
            return None;
        }
        (host.to_string(), path.to_string())
    };
    let path = path.trim().trim_start_matches('/').trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if host.is_empty()
        || path.is_empty()
        || path
            .chars()
            .any(|ch| matches!(ch, '\\' | '?' | '#' | '\n' | '\r'))
    {
        return None;
    }
    Some(format!("{host}/{path}").to_ascii_lowercase())
}

fn repo_label(origin: &str) -> String {
    origin
        .rsplit(['/', ':'])
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(origin)
        .to_string()
}

fn path_label(path: &str) -> Option<String> {
    path.rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .map(ToOwned::to_owned)
}

fn normalize_source(source: &Value) -> String {
    // Source is a provenance category, not a safe place to echo arbitrary
    // nested JSON (sub-agent metadata and custom extensions may be private).
    match source {
        Value::String(value)
            if matches!(
                value.as_str(),
                "cli" | "vscode" | "exec" | "appServer" | "mcp" | "unknown"
            ) =>
        {
            value.clone()
        }
        Value::Object(map) if map.len() == 1 && map.contains_key("subAgent") => {
            "subAgent".into()
        }
        Value::Object(map) if map.len() == 1 && map.contains_key("custom") => "custom".into(),
        _ => "unknown".into(),
    }
}

#[cfg(test)]
mod safe_source_tests {
    use super::normalize_source;
    use serde_json::json;

    #[test]
    fn only_known_source_categories_are_displayed() {
        for kind in ["cli", "vscode", "exec", "appServer", "mcp", "unknown"] {
            assert_eq!(normalize_source(&json!(kind)), kind);
        }
        assert_eq!(normalize_source(&json!("unexpected_new_kind")), "unknown");
        assert_eq!(normalize_source(&json!("credential\u{001b}[2J")), "unknown");
    }

    #[test]
    fn nested_subagent_or_custom_payload_is_not_echoed_as_json() {
        let source = json!({
            "subAgent": {"threadSpawn": {"parentThreadId": "root", "private": "redact"}}
        });
        let label = normalize_source(&source);
        assert_eq!(label, "subAgent");
        assert!(!label.contains("redact"));
        assert_eq!(normalize_source(&json!({"custom":"private value"})), "custom");
        assert_eq!(
            normalize_source(&json!({"subAgent":"review", "extra":"secret"})),
            "unknown"
        );
        assert_eq!(normalize_source(&json!(null)), "unknown");
    }
}

fn first_nonempty_line(value: &str) -> Option<String> {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod status_tests;
#[cfg(test)]
mod tests;
