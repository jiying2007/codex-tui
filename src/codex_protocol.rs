use crate::domain::{AttentionReason, RuntimeStatus, ThreadId, ThreadMetadata, ThreadSummary};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;

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
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub updated_at: i64,
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
            cwd: thread.cwd,
            model: thread.model,
            project_id: thread.project_id,
            source,
            updated_at: thread.updated_at,
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
        .unwrap_or("notLoaded");
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
        "systemError" => (RuntimeStatus::Ready, vec![AttentionReason::ReadyForReview]),
        _ => (RuntimeStatus::Inactive, vec![]),
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
    {
        let key = normalize_git_origin(origin);
        return (repo_label(&key), format!("git:{key}"), "git-origin".into());
    }

    let cwd = thread.cwd.trim();
    let label = path_label(cwd).unwrap_or_else(|| cwd.to_string());
    (label, format!("cwd:{cwd}"), "cwd".into())
}

fn normalize_git_origin(origin: &str) -> String {
    origin
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase()
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
    match source {
        Value::String(value) => value.clone(),
        Value::Null => "unknown".into(),
        value => value.to_string(),
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
mod tests {
    use super::*;
    use serde_json::json;

    fn wire(status: Value) -> ThreadWire {
        ThreadWire {
            id: "0199-test".into(),
            preview: "First user prompt\nsecond line".into(),
            project_id: None,
            model: Some("gpt-5.6".into()),
            updated_at: 42,
            status,
            cwd: "/work/audio-pipeline".into(),
            source: json!("cli"),
            git_info: Some(GitInfoWire {
                origin_url: Some("https://github.com/jiying2007/audio-pipeline.git".into()),
            }),
            name: None,
        }
    }

    #[test]
    fn active_waiting_flags_become_attention_without_inventing_state() {
        let summary = normalize_thread(
            wire(json!({
                "type": "active",
                "activeFlags": ["waitingOnApproval", "waitingOnUserInput"]
            })),
            None,
        );
        assert_eq!(summary.runtime, RuntimeStatus::WaitingHuman);
        assert_eq!(
            summary.attention,
            vec![
                AttentionReason::ApprovalRequired,
                AttentionReason::UserInputRequired
            ]
        );
    }

    #[test]
    fn workspace_fallback_prefers_project_then_git_then_cwd() {
        let mut thread = wire(json!({"type": "idle"}));
        thread.project_id = Some("proj-1".into());
        let project = normalize_thread(thread.clone(), None);
        assert_eq!(project.metadata.workspace_basis, "codex-project");

        thread.project_id = None;
        let git = normalize_thread(thread.clone(), None);
        assert_eq!(git.workspace, "audio-pipeline");
        assert_eq!(git.metadata.workspace_basis, "git-origin");

        thread.git_info = None;
        let cwd = normalize_thread(thread, None);
        assert_eq!(cwd.workspace, "audio-pipeline");
        assert_eq!(cwd.metadata.workspace_basis, "cwd");
    }

    #[test]
    fn exact_thread_id_is_preserved_as_identity() {
        let summary = normalize_thread(wire(json!({"type": "notLoaded"})), None);
        assert_eq!(summary.id.0, "0199-test");
    }
}
