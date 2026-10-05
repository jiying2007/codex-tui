use crate::domain::ThreadId;
use crate::operation::new_operation_id;
use anyhow::{Context, Result};
use serde_json::{Value, json};

pub const THREAD_QUEUE_PAGE_LIMIT: u32 = 100;
pub const THREAD_QUEUE_MAX_PAGES: usize = 10;
pub const THREAD_QUEUE_TEXT_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedSubmission {
    pub id: String,
    pub client_user_message_id: String,
    pub input: Vec<Value>,
    pub summary: String,
    pub editable_text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadQueueSnapshot {
    pub thread_id: ThreadId,
    pub submissions: Vec<QueuedSubmission>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThreadQueueMutation {
    Add {
        thread_id: ThreadId,
        text: String,
        client_user_message_id: String,
    },
    Update {
        thread_id: ThreadId,
        queued_submission_id: String,
        text: String,
    },
    Delete {
        thread_id: ThreadId,
        queued_submission_id: String,
    },
    Reorder {
        thread_id: ThreadId,
        queued_submission_ids: Vec<String>,
    },
    Start {
        thread_id: ThreadId,
        queued_submission_id: String,
    },
}

impl ThreadQueueMutation {
    pub fn add(thread_id: ThreadId, text: String, now_unix_ms: u64) -> Result<Self> {
        let text = validate_text(text)?;
        let operation_id = new_operation_id(now_unix_ms);
        Ok(Self::Add {
            thread_id,
            text,
            client_user_message_id: format!("codex-tui-{operation_id}"),
        })
    }

    pub fn update(thread_id: ThreadId, queued_submission_id: String, text: String) -> Result<Self> {
        let text = validate_text(text)?;
        anyhow::ensure!(
            !queued_submission_id.trim().is_empty(),
            "queued submission id must not be empty"
        );
        Ok(Self::Update {
            thread_id,
            queued_submission_id,
            text,
        })
    }

    pub fn delete(thread_id: ThreadId, queued_submission_id: String) -> Result<Self> {
        anyhow::ensure!(
            !queued_submission_id.trim().is_empty(),
            "queued submission id must not be empty"
        );
        Ok(Self::Delete {
            thread_id,
            queued_submission_id,
        })
    }

    pub fn reorder(thread_id: ThreadId, queued_submission_ids: Vec<String>) -> Result<Self> {
        anyhow::ensure!(
            !queued_submission_ids.is_empty(),
            "queue reorder requires at least one submission"
        );
        anyhow::ensure!(
            queued_submission_ids.iter().all(|id| !id.trim().is_empty()),
            "queue reorder contains an empty submission id"
        );
        let unique = queued_submission_ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        anyhow::ensure!(
            unique.len() == queued_submission_ids.len(),
            "queue reorder contains duplicate submission ids"
        );
        Ok(Self::Reorder {
            thread_id,
            queued_submission_ids,
        })
    }

    pub fn start(thread_id: ThreadId, queued_submission_id: String) -> Result<Self> {
        anyhow::ensure!(
            !queued_submission_id.trim().is_empty(),
            "queued submission id must not be empty"
        );
        Ok(Self::Start {
            thread_id,
            queued_submission_id,
        })
    }

    pub const fn destructive(&self) -> bool {
        matches!(self, Self::Delete { .. } | Self::Start { .. })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Add { .. } => "add",
            Self::Update { .. } => "update",
            Self::Delete { .. } => "delete",
            Self::Reorder { .. } => "reorder",
            Self::Start { .. } => "start",
        }
    }
}

pub fn parse_queue_list(thread_id: ThreadId, result: Value) -> Result<ThreadQueueSnapshot> {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .context("thread/queue/list response missing data")?;
    anyhow::ensure!(
        data.len() <= THREAD_QUEUE_PAGE_LIMIT as usize,
        "thread queue page exceeds requested limit"
    );
    let submissions = data
        .iter()
        .map(parse_submission)
        .collect::<Result<Vec<_>>>()?;
    validate_submissions(&submissions)?;
    let next_cursor = match result.get("nextCursor") {
        None | Some(Value::Null) => None,
        Some(Value::String(cursor)) if !cursor.trim().is_empty() => Some(cursor.clone()),
        _ => anyhow::bail!("thread queue nextCursor must be null or a non-empty string"),
    };
    Ok(ThreadQueueSnapshot {
        thread_id,
        submissions,
        next_cursor,
    })
}

pub fn parse_submission(value: &Value) -> Result<QueuedSubmission> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .context("queued submission missing id")?
        .to_string();
    let client_user_message_id = value
        .get("clientUserMessageId")
        .and_then(Value::as_str)
        .context("queued submission missing clientUserMessageId")?
        .to_string();
    anyhow::ensure!(
        !id.trim().is_empty(),
        "queued submission id must not be empty"
    );
    anyhow::ensure!(
        !client_user_message_id.trim().is_empty(),
        "queued client message id must not be empty"
    );
    let input = value
        .get("input")
        .and_then(Value::as_array)
        .context("queued submission missing input")?
        .clone();

    let mut text_parts = Vec::new();
    let mut text_only = !input.is_empty();
    for item in &input {
        match item.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    text_parts.push(text.to_string());
                } else {
                    text_only = false;
                    text_parts.push("[invalid text]".into());
                }
                // The text editor replaces input with text_input(), so only a
                // fully understood plain-text shape can be edited without loss.
                text_only &= match item.get("textElements") {
                    None => true, // previous optional-field shape
                    Some(Value::Array(elements)) => elements.is_empty(),
                    _ => false,
                };
                text_only &= item.as_object().is_some_and(|object| {
                    object
                        .keys()
                        .all(|key| matches!(key.as_str(), "type" | "text" | "textElements"))
                });
            }
            Some(other) => {
                text_only = false;
                text_parts.push(format!("[{other}]"));
            }
            None => {
                text_only = false;
                text_parts.push("[input]".into());
            }
        }
    }
    let summary = if text_parts.is_empty() {
        "(empty input)".into()
    } else {
        text_parts.join(" ")
    };
    let editable_text = text_only.then(|| {
        input
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n")
    });

    Ok(QueuedSubmission {
        id,
        client_user_message_id,
        input,
        summary,
        editable_text,
    })
}

fn validate_submissions(submissions: &[QueuedSubmission]) -> Result<()> {
    anyhow::ensure!(
        submissions.len() <= THREAD_QUEUE_MAX_PAGES * THREAD_QUEUE_PAGE_LIMIT as usize,
        "thread queue exceeds bounded item count"
    );
    let mut ids = std::collections::BTreeSet::new();
    for item in submissions {
        anyhow::ensure!(
            !item.id.trim().is_empty() && !item.client_user_message_id.trim().is_empty(),
            "thread queue contains empty identity"
        );
        anyhow::ensure!(
            ids.insert(&item.id),
            "thread queue contains duplicate submission identity"
        );
    }
    Ok(())
}

impl ThreadQueueSnapshot {
    pub fn validate_complete(&self) -> Result<()> {
        anyhow::ensure!(
            !self.thread_id.0.trim().is_empty(),
            "thread queue has empty thread identity"
        );
        anyhow::ensure!(
            self.next_cursor.is_none(),
            "thread queue snapshot is incomplete"
        );
        validate_submissions(&self.submissions)
    }
}

pub fn text_input(text: &str) -> Value {
    json!({
        "type": "text",
        "text": text,
        "textElements": []
    })
}

fn validate_text(text: String) -> Result<String> {
    let text = text.trim().to_string();
    anyhow::ensure!(!text.is_empty(), "queue text must not be empty");
    anyhow::ensure!(
        text.len() <= THREAD_QUEUE_TEXT_LIMIT,
        "queue text exceeds {THREAD_QUEUE_TEXT_LIMIT} bytes"
    );
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn queue_list_preserves_authoritative_ids_and_text() {
        let page = parse_queue_list(
            ThreadId::new("thread-1"),
            json!({
                "data": [{
                    "id": "queued-1",
                    "input": [{
                        "type": "text",
                        "text": "run the regression",
                        "textElements": []
                    }],
                    "clientUserMessageId": "client-1"
                }],
                "nextCursor": null
            }),
        )
        .expect("queue");
        assert_eq!(page.submissions.len(), 1);
        assert_eq!(page.submissions[0].id, "queued-1");
        assert_eq!(
            page.submissions[0].editable_text.as_deref(),
            Some("run the regression")
        );
    }

    #[test]
    fn multimodal_submission_is_visible_but_not_text_editable() {
        let submission = parse_submission(&json!({
            "id": "queued-2",
            "input": [
                {"type": "text", "text": "inspect", "textElements": []},
                {"type": "localImage", "path": "/tmp/a.png"}
            ],
            "clientUserMessageId": "client-2"
        }))
        .expect("submission");
        assert!(submission.editable_text.is_none());
        assert!(submission.summary.contains("[localImage]"));
    }

    #[test]
    fn mutations_validate_text_and_reorder_identity() {
        assert!(ThreadQueueMutation::add(ThreadId::new("t"), " ".into(), 1).is_err());
        assert!(
            ThreadQueueMutation::reorder(ThreadId::new("t"), vec!["one".into(), "one".into()])
                .is_err()
        );
        let mutation =
            ThreadQueueMutation::start(ThreadId::new("t"), "queued-1".into()).expect("start");
        assert!(mutation.destructive());
    }
}
