use crate::domain::ThreadId;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConversationItemKind {
    User,
    Assistant,
    Reasoning,
    Command,
    FileChange,
    Tool,
    Plan,
    Other,
}

impl ConversationItemKind {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::User => "YOU",
            Self::Assistant => "CODEX",
            Self::Reasoning => "THINK",
            Self::Command => "CMD",
            Self::FileChange => "PATCH",
            Self::Tool => "TOOL",
            Self::Plan => "PLAN",
            Self::Other => "ITEM",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationItem {
    pub turn_id: String,
    pub item_id: String,
    pub kind: ConversationItemKind,
    pub text: String,
    pub status: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationTurn {
    pub id: String,
    pub status: String,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationPage {
    pub thread_id: ThreadId,
    pub title: Option<String>,
    pub turns: Vec<ConversationTurn>,
    pub items: Vec<ConversationItem>,
    pub next_turn_cursor: Option<String>,
    pub next_item_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationState {
    pub thread_id: ThreadId,
    pub title: Option<String>,
    pub turns: Vec<ConversationTurn>,
    pub items: Vec<ConversationItem>,
    pub next_turn_cursor: Option<String>,
    pub next_item_cursor: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
}

impl ConversationState {
    pub fn loading(thread_id: ThreadId) -> Self {
        Self {
            thread_id,
            title: None,
            turns: vec![],
            items: vec![],
            next_turn_cursor: None,
            next_item_cursor: None,
            loading: true,
            error: None,
        }
    }

    pub fn replace_page(&mut self, page: ConversationPage) {
        self.title = page.title;
        self.turns = page.turns;
        self.items = page.items;
        self.next_turn_cursor = page.next_turn_cursor;
        self.next_item_cursor = page.next_item_cursor;
        self.loading = false;
        self.error = None;
    }
}

pub fn parse_thread_title(result: &Value) -> Option<String> {
    result
        .pointer("/thread/name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            result
                .pointer("/thread/preview")
                .and_then(Value::as_str)
                .and_then(|preview| {
                    preview
                        .lines()
                        .map(str::trim)
                        .find(|line| !line.is_empty())
                        .map(ToOwned::to_owned)
                })
        })
}

pub fn parse_turns_page(result: Value) -> Result<(Vec<ConversationTurn>, Option<String>)> {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .context("thread/turns/list response missing data")?;
    let mut turns = Vec::with_capacity(data.len());
    for turn in data {
        let id = required_string(turn, "id", "turn")?;
        let status = turn
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let error = turn
            .pointer("/error/message")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        turns.push(ConversationTurn {
            id,
            status,
            started_at: turn.get("startedAt").and_then(Value::as_i64),
            completed_at: turn.get("completedAt").and_then(Value::as_i64),
            error,
        });
    }
    let next_cursor = optional_string(&result, "nextCursor");
    Ok((turns, next_cursor))
}

pub fn parse_items_page(result: Value) -> Result<(Vec<ConversationItem>, Option<String>)> {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .context("thread/items/list response missing data")?;
    let mut items = Vec::with_capacity(data.len());
    for entry in data {
        let turn_id = required_string(entry, "turnId", "item entry")?;
        let item = entry.get("item").context("item entry missing item")?;
        items.push(normalize_item(turn_id, item)?);
    }
    let next_cursor = optional_string(&result, "nextCursor");
    Ok((items, next_cursor))
}

pub fn merge_history(
    thread_id: ThreadId,
    title: Option<String>,
    turns: Vec<ConversationTurn>,
    mut items: Vec<ConversationItem>,
    next_turn_cursor: Option<String>,
    next_item_cursor: Option<String>,
) -> ConversationPage {
    let turn_order = turns
        .iter()
        .enumerate()
        .map(|(index, turn)| (turn.id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    items.sort_by_key(|item| {
        turn_order
            .get(item.turn_id.as_str())
            .copied()
            .unwrap_or(usize::MAX)
    });
    ConversationPage {
        thread_id,
        title,
        turns,
        items,
        next_turn_cursor,
        next_item_cursor,
    }
}

fn normalize_item(turn_id: String, item: &Value) -> Result<ConversationItem> {
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let item_id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(item_type)
        .to_string();

    let (kind, text, status) = match item_type {
        "userMessage" => (
            ConversationItemKind::User,
            user_message_text(item),
            None,
        ),
        "agentMessage" => (
            ConversationItemKind::Assistant,
            item.get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            None,
        ),
        "reasoning" => {
            let text = item
                .get("summary")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            (ConversationItemKind::Reasoning, text, None)
        }
        "plan" => (
            ConversationItemKind::Plan,
            item.get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            None,
        ),
        "commandExecution" => (
            ConversationItemKind::Command,
            item.get("command")
                .and_then(Value::as_str)
                .unwrap_or("command")
                .to_string(),
            optional_status(item),
        ),
        "fileChange" => {
            let count = item
                .get("changes")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            (
                ConversationItemKind::FileChange,
                format!("{count} file change(s)"),
                optional_status(item),
            )
        }
        "mcpToolCall" | "dynamicToolCall" | "collabAgentToolCall" => {
            let tool = item
                .get("tool")
                .and_then(Value::as_str)
                .or_else(|| item.get("server").and_then(Value::as_str))
                .unwrap_or(item_type);
            (
                ConversationItemKind::Tool,
                tool.to_string(),
                optional_status(item),
            )
        }
        _ => (
            ConversationItemKind::Other,
            item_type.to_string(),
            optional_status(item),
        ),
    };

    Ok(ConversationItem {
        turn_id,
        item_id,
        kind,
        text,
        status,
    })
}

fn user_message_text(item: &Value) -> String {
    item.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|input| match input.get("type").and_then(Value::as_str) {
            Some("text") => input.get("text").and_then(Value::as_str).map(str::to_string),
            Some(kind) => Some(format!("[{kind}]")),
            None => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn optional_status(item: &Value) -> Option<String> {
    item.get("status").map(|value| match value {
        Value::String(status) => status.clone(),
        other => other.to_string(),
    })
}

fn required_string(value: &Value, field: &str, context: &str) -> Result<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .with_context(|| format!("{context} missing {field}"))
}

fn optional_string(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_recent_history_without_protocol_types_leaking_to_ui() {
        let (turns, next_turn) = parse_turns_page(json!({
            "data": [{
                "id": "turn-1",
                "status": "completed",
                "startedAt": 10,
                "completedAt": 12,
                "error": null
            }],
            "nextCursor": "older-turns"
        }))
        .expect("turns");
        let (items, next_item) = parse_items_page(json!({
            "data": [
                {
                    "turnId": "turn-1",
                    "item": {
                        "type": "userMessage",
                        "id": "user-1",
                        "content": [{"type": "text", "text": "hello"}]
                    }
                },
                {
                    "turnId": "turn-1",
                    "item": {
                        "type": "agentMessage",
                        "id": "agent-1",
                        "text": "world"
                    }
                }
            ],
            "nextCursor": "older-items"
        }))
        .expect("items");

        assert_eq!(turns[0].status, "completed");
        assert_eq!(items[0].kind, ConversationItemKind::User);
        assert_eq!(items[0].text, "hello");
        assert_eq!(items[1].kind, ConversationItemKind::Assistant);
        assert_eq!(next_turn.as_deref(), Some("older-turns"));
        assert_eq!(next_item.as_deref(), Some("older-items"));
    }

    #[test]
    fn unknown_items_degrade_to_other_instead_of_failing() {
        let (items, _) = parse_items_page(json!({
            "data": [{
                "turnId": "turn-1",
                "item": {"type": "futureItem", "id": "future-1", "status": {"state":"new"}}
            }],
            "nextCursor": null
        }))
        .expect("items");
        assert_eq!(items[0].kind, ConversationItemKind::Other);
        assert_eq!(items[0].text, "futureItem");
    }

    #[test]
    fn thread_title_prefers_name_then_preview() {
        assert_eq!(
            parse_thread_title(&json!({"thread":{"name":"Named","preview":"Preview"}})).as_deref(),
            Some("Named")
        );
        assert_eq!(
            parse_thread_title(&json!({"thread":{"name":null,"preview":"\nPreview"}})).as_deref(),
            Some("Preview")
        );
    }
}
