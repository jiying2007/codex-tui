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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RpcRequestId {
    Integer(i64),
    String(String),
}

impl RpcRequestId {
    pub fn from_value(value: &Value) -> Result<Self> {
        if let Some(value) = value.as_i64() {
            return Ok(Self::Integer(value));
        }
        if let Some(value) = value.as_str() {
            return Ok(Self::String(value.to_string()));
        }
        anyhow::bail!("server request id must be an integer or string")
    }

    pub fn to_value(&self) -> Value {
        match self {
            Self::Integer(value) => Value::from(*value),
            Self::String(value) => Value::from(value.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserInputQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    pub is_secret: bool,
    pub options: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractiveRequestKind {
    CommandApproval {
        command: String,
        cwd: String,
        reason: Option<String>,
    },
    FileChangeApproval {
        reason: Option<String>,
    },
    PermissionsApproval {
        reason: Option<String>,
        network_requested: bool,
        filesystem_requested: bool,
    },
    UserInput {
        questions: Vec<UserInputQuestion>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractiveRequest {
    pub request_id: RpcRequestId,
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub item_id: String,
    pub kind: InteractiveRequestKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractiveResolution {
    Accept,
    Decline,
    Cancel,
    UserInput(BTreeMap<String, Vec<String>>),
}

pub fn parse_interactive_request(message: &Value) -> Result<Option<InteractiveRequest>> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Ok(None);
    };
    let params = message
        .get("params")
        .context("server request missing params")?;
    let request_id = RpcRequestId::from_value(
        message
            .get("id")
            .context("server request missing request id")?,
    )?;

    let request = match method {
        "item/commandExecution/requestApproval" => InteractiveRequest {
            request_id,
            thread_id: ThreadId::new(required_string(params, "threadId", "command approval")?),
            turn_id: required_string(params, "turnId", "command approval")?,
            item_id: params
                .get("approvalId")
                .and_then(Value::as_str)
                .or_else(|| params.get("itemId").and_then(Value::as_str))
                .context("command approval missing itemId")?
                .to_string(),
            kind: InteractiveRequestKind::CommandApproval {
                command: params
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                cwd: params
                    .get("cwd")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                reason: params
                    .get("reason")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            },
        },
        "item/fileChange/requestApproval" => InteractiveRequest {
            request_id,
            thread_id: ThreadId::new(required_string(params, "threadId", "file approval")?),
            turn_id: required_string(params, "turnId", "file approval")?,
            item_id: required_string(params, "itemId", "file approval")?,
            kind: InteractiveRequestKind::FileChangeApproval {
                reason: params
                    .get("reason")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            },
        },
        "item/permissions/requestApproval" => {
            let permissions = params
                .get("permissions")
                .context("permissions approval missing permissions")?;
            InteractiveRequest {
                request_id,
                thread_id: ThreadId::new(required_string(
                    params,
                    "threadId",
                    "permissions approval",
                )?),
                turn_id: required_string(params, "turnId", "permissions approval")?,
                item_id: required_string(params, "itemId", "permissions approval")?,
                kind: InteractiveRequestKind::PermissionsApproval {
                    reason: params
                        .get("reason")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    network_requested: permissions
                        .get("network")
                        .is_some_and(|value| !value.is_null()),
                    filesystem_requested: permissions
                        .get("fileSystem")
                        .is_some_and(|value| !value.is_null()),
                },
            }
        }
        "item/tool/requestUserInput" => {
            let questions = params
                .get("questions")
                .and_then(Value::as_array)
                .context("request_user_input missing questions")?
                .iter()
                .map(parse_user_input_question)
                .collect::<Result<Vec<_>>>()?;
            InteractiveRequest {
                request_id,
                thread_id: ThreadId::new(required_string(
                    params,
                    "threadId",
                    "request_user_input",
                )?),
                turn_id: required_string(params, "turnId", "request_user_input")?,
                item_id: required_string(params, "itemId", "request_user_input")?,
                kind: InteractiveRequestKind::UserInput { questions },
            }
        }
        _ => return Ok(None),
    };

    Ok(Some(request))
}

fn parse_user_input_question(value: &Value) -> Result<UserInputQuestion> {
    let options = value
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            option
                .get("label")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect();
    Ok(UserInputQuestion {
        id: required_string(value, "id", "user input question")?,
        header: value
            .get("header")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        question: required_string(value, "question", "user input question")?,
        is_secret: value
            .get("isSecret")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        options,
    })
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
    revision: u64,
    pub title: Option<String>,
    pub turns: Vec<ConversationTurn>,
    pub items: Vec<ConversationItem>,
    pub next_turn_cursor: Option<String>,
    pub next_item_cursor: Option<String>,
    pub loading: bool,
    pub loading_older: bool,
    pub error: Option<String>,
}

impl ConversationState {
    pub fn loading(thread_id: ThreadId) -> Self {
        Self {
            thread_id,
            revision: 0,
            title: None,
            turns: vec![],
            items: vec![],
            next_turn_cursor: None,
            next_item_cursor: None,
            loading: true,
            loading_older: false,
            error: None,
        }
    }

    pub const fn presentation_revision(&self) -> u64 {
        self.revision
    }

    pub fn active_turn_id(&self) -> Option<&str> {
        self.turns
            .iter()
            .rev()
            .find(|turn| turn.status == "inProgress")
            .map(|turn| turn.id.as_str())
    }

    pub fn replace_page(&mut self, page: ConversationPage) {
        self.revision = self.revision.saturating_add(1);
        self.title = page.title;
        self.turns = page.turns;
        self.items = page.items;
        self.next_turn_cursor = page.next_turn_cursor;
        self.next_item_cursor = page.next_item_cursor;
        self.loading = false;
        self.loading_older = false;
        self.error = None;
    }

    pub fn prepend_page(&mut self, page: ConversationPage) {
        self.revision = self.revision.saturating_add(1);
        let existing_turns = self
            .turns
            .iter()
            .map(|turn| turn.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let mut turns = page
            .turns
            .into_iter()
            .filter(|turn| !existing_turns.contains(turn.id.as_str()))
            .collect::<Vec<_>>();
        turns.append(&mut self.turns);
        self.turns = turns;

        let existing_items = self
            .items
            .iter()
            .map(|item| (item.turn_id.as_str(), item.item_id.as_str()))
            .collect::<std::collections::BTreeSet<_>>();
        let mut items = page
            .items
            .into_iter()
            .filter(|item| {
                !existing_items.contains(&(item.turn_id.as_str(), item.item_id.as_str()))
            })
            .collect::<Vec<_>>();
        items.append(&mut self.items);
        self.items = items;

        self.next_turn_cursor = page.next_turn_cursor;
        self.next_item_cursor = page.next_item_cursor;
        self.loading = false;
        self.loading_older = false;
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

pub fn parse_legacy_thread_read(result: Value, thread_id: ThreadId) -> Result<ConversationPage> {
    let title = parse_thread_title(&result);
    let turns_value = result
        .pointer("/thread/turns")
        .and_then(Value::as_array)
        .context("legacy thread/read response missing thread.turns")?;

    let mut turns = Vec::with_capacity(turns_value.len());
    let mut items = Vec::new();
    for turn in turns_value {
        let turn_id = required_string(turn, "id", "legacy turn")?;
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
            id: turn_id.clone(),
            status,
            started_at: turn.get("startedAt").and_then(Value::as_i64),
            completed_at: turn.get("completedAt").and_then(Value::as_i64),
            error,
        });

        if let Some(turn_items) = turn.get("items").and_then(Value::as_array) {
            for item in turn_items {
                items.push(normalize_item(turn_id.clone(), item)?);
            }
        }
    }

    Ok(merge_history(thread_id, title, turns, items, None, None))
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
        "userMessage" => (ConversationItemKind::User, user_message_text(item), None),
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
            Some("text") => input
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string),
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
    fn parses_command_approval_into_normalized_request() {
        let request = parse_interactive_request(&json!({
            "id": 7,
            "method": "item/commandExecution/requestApproval",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "itemId": "item-1",
                "command": "cargo test",
                "cwd": "/work/repo",
                "reason": "run tests"
            }
        }))
        .expect("parse")
        .expect("known request");
        assert_eq!(request.request_id, RpcRequestId::Integer(7));
        assert_eq!(request.thread_id.0, "thread-1");
        assert!(matches!(
            request.kind,
            InteractiveRequestKind::CommandApproval { .. }
        ));
    }

    #[test]
    fn parses_multiple_user_input_questions_without_losing_options() {
        let request = parse_interactive_request(&json!({
            "id": "req-1",
            "method": "item/tool/requestUserInput",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "itemId": "item-1",
                "questions": [
                    {
                        "id": "q1",
                        "header": "Mode",
                        "question": "Choose mode",
                        "isOther": false,
                        "isSecret": false,
                        "options": [{"label":"fast","description":"Fast"}]
                    },
                    {
                        "id": "q2",
                        "header": "Token",
                        "question": "Enter token",
                        "isOther": true,
                        "isSecret": true,
                        "options": null
                    }
                ],
                "isBlocking": true
            }
        }))
        .expect("parse")
        .expect("known request");
        let InteractiveRequestKind::UserInput { questions } = request.kind else {
            panic!("expected user input request");
        };
        assert_eq!(questions.len(), 2);
        assert_eq!(questions[0].options, vec!["fast"]);
        assert!(questions[1].is_secret);
    }

    #[test]
    fn legacy_thread_read_normalizes_full_turn_history() {
        let page = parse_legacy_thread_read(
            json!({
                "thread": {
                    "name": "Legacy",
                    "turns": [{
                        "id": "turn-1",
                        "status": "completed",
                        "startedAt": 1,
                        "completedAt": 2,
                        "error": null,
                        "items": [
                            {
                                "type": "userMessage",
                                "id": "u1",
                                "content": [{"type":"text","text":"hello"}]
                            },
                            {
                                "type": "agentMessage",
                                "id": "a1",
                                "text": "world"
                            }
                        ]
                    }]
                }
            }),
            ThreadId::new("thread-legacy"),
        )
        .expect("legacy page");
        assert_eq!(page.title.as_deref(), Some("Legacy"));
        assert_eq!(page.turns.len(), 1);
        assert_eq!(page.items.len(), 2);
        assert!(page.next_turn_cursor.is_none());
        assert!(page.next_item_cursor.is_none());
    }

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
    fn older_pages_prepend_without_duplicate_turns_or_items() {
        let mut state = ConversationState::loading(ThreadId::new("thread-1"));
        assert_eq!(state.presentation_revision(), 0);
        state.replace_page(ConversationPage {
            thread_id: ThreadId::new("thread-1"),
            title: Some("title".into()),
            turns: vec![ConversationTurn {
                id: "turn-2".into(),
                status: "completed".into(),
                started_at: None,
                completed_at: None,
                error: None,
            }],
            items: vec![ConversationItem {
                turn_id: "turn-2".into(),
                item_id: "item-2".into(),
                kind: ConversationItemKind::Assistant,
                text: "new".into(),
                status: None,
            }],
            next_turn_cursor: Some("cursor-1".into()),
            next_item_cursor: Some("cursor-1".into()),
        });
        assert_eq!(state.presentation_revision(), 1);
        state.prepend_page(ConversationPage {
            thread_id: ThreadId::new("thread-1"),
            title: None,
            turns: vec![
                ConversationTurn {
                    id: "turn-1".into(),
                    status: "completed".into(),
                    started_at: None,
                    completed_at: None,
                    error: None,
                },
                ConversationTurn {
                    id: "turn-2".into(),
                    status: "completed".into(),
                    started_at: None,
                    completed_at: None,
                    error: None,
                },
            ],
            items: vec![
                ConversationItem {
                    turn_id: "turn-1".into(),
                    item_id: "item-1".into(),
                    kind: ConversationItemKind::User,
                    text: "old".into(),
                    status: None,
                },
                ConversationItem {
                    turn_id: "turn-2".into(),
                    item_id: "item-2".into(),
                    kind: ConversationItemKind::Assistant,
                    text: "duplicate".into(),
                    status: None,
                },
            ],
            next_turn_cursor: None,
            next_item_cursor: None,
        });
        assert_eq!(state.presentation_revision(), 2);

        assert_eq!(
            state
                .turns
                .iter()
                .map(|turn| turn.id.as_str())
                .collect::<Vec<_>>(),
            vec!["turn-1", "turn-2"]
        );
        assert_eq!(
            state
                .items
                .iter()
                .map(|item| item.item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["item-1", "item-2"]
        );
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
