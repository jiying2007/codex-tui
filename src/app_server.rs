use crate::backend::{BackendSnapshot, BackendStatus};
use crate::codex_protocol::{
    ThreadWire, apply_status, normalize_thread, parse_loaded_list, parse_thread_list,
};
use crate::conversation::{
    ConversationPage, merge_history, parse_items_page, parse_thread_title, parse_turns_page,
};
use crate::domain::{ThreadId, ThreadSummary};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const PAGE_SIZE: u32 = 200;
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const RPC_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub struct StartedRegistry {
    pub initial: BackendSnapshot,
    pub handle: RegistryHandle,
}

#[derive(Clone, Debug)]
pub enum BackendCommand {
    LoadConversation(ThreadId),
    SubmitPrompt {
        thread_id: ThreadId,
        text: String,
        active_turn_id: Option<String>,
    },
    InterruptTurn {
        thread_id: ThreadId,
        turn_id: String,
    },
}

#[derive(Clone, Debug)]
pub enum ConversationEvent {
    Loaded(ConversationPage),
    PromptSubmitted { thread_id: ThreadId, turn_id: String },
    Failed { thread_id: ThreadId, error: String },
}

pub struct RegistryHandle {
    rx: mpsc::UnboundedReceiver<BackendSnapshot>,
    conversation_rx: mpsc::UnboundedReceiver<ConversationEvent>,
    command_tx: mpsc::UnboundedSender<BackendCommand>,
    task: JoinHandle<()>,
}

impl RegistryHandle {
    pub fn try_recv(&mut self) -> Option<BackendSnapshot> {
        self.rx.try_recv().ok()
    }

    pub fn try_recv_conversation(&mut self) -> Option<ConversationEvent> {
        self.conversation_rx.try_recv().ok()
    }

    pub fn load_conversation(&self, thread_id: ThreadId) -> Result<()> {
        self.send_command(BackendCommand::LoadConversation(thread_id))
    }

    pub fn submit_prompt(
        &self,
        thread_id: ThreadId,
        text: String,
        active_turn_id: Option<String>,
    ) -> Result<()> {
        self.send_command(BackendCommand::SubmitPrompt {
            thread_id,
            text,
            active_turn_id,
        })
    }

    pub fn interrupt_turn(&self, thread_id: ThreadId, turn_id: String) -> Result<()> {
        self.send_command(BackendCommand::InterruptTurn { thread_id, turn_id })
    }

    fn send_command(&self, command: BackendCommand) -> Result<()> {
        self.command_tx
            .send(command)
            .map_err(|_| anyhow!("App Server actor is not available"))
    }
}

impl Drop for RegistryHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn start(codex_bin: Option<OsString>) -> Result<StartedRegistry> {
    let mut rpc = RpcSession::spawn(codex_bin).await?;
    let init = initialize(&mut rpc).await?;
    let mut status = status_from_initialize(&init);
    let (threads, loaded_supported) = load_registry(&mut rpc).await?;
    status.capabilities.push("thread/list".into());
    status.capabilities.push("thread/status/changed".into());
    if loaded_supported {
        status.capabilities.push("thread/loaded/list".into());
    } else {
        status
            .optional_capabilities_missing
            .push("thread/loaded/list".into());
    }
    status.connected = true;
    status.last_refresh_unix_ms = Some(now_unix_ms());

    let initial = BackendSnapshot {
        generation: 0,
        threads: threads.clone(),
        status: status.clone(),
    };
    let (tx, rx) = mpsc::unbounded_channel();
    let (conversation_tx, conversation_rx) = mpsc::unbounded_channel();
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(run_registry_actor(
        rpc,
        threads,
        status,
        tx,
        conversation_tx,
        command_rx,
    ));

    Ok(StartedRegistry {
        initial,
        handle: RegistryHandle {
            rx,
            conversation_rx,
            command_tx,
            task,
        },
    })
}

pub async fn probe(codex_bin: Option<OsString>) -> Result<BackendSnapshot> {
    Ok(start(codex_bin).await?.initial)
}

async fn run_registry_actor(
    mut rpc: RpcSession,
    initial_threads: Vec<ThreadSummary>,
    mut status: BackendStatus,
    tx: mpsc::UnboundedSender<BackendSnapshot>,
    conversation_tx: mpsc::UnboundedSender<ConversationEvent>,
    mut command_rx: mpsc::UnboundedReceiver<BackendCommand>,
) {
    let mut generation = 0_u64;
    let mut threads = by_id(initial_threads);
    let mut refresh = tokio::time::interval(REFRESH_INTERVAL);
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    refresh.tick().await;

    loop {
        tokio::select! {
            command = command_rx.recv() => {
                let Some(command) = command else {
                    return;
                };
                match command {
                    BackendCommand::LoadConversation(thread_id) => {
                        emit_conversation_load(&mut rpc, thread_id, &conversation_tx).await;
                    }
                    BackendCommand::SubmitPrompt {
                        thread_id,
                        text,
                        active_turn_id,
                    } => {
                        match submit_prompt(
                            &mut rpc,
                            &mut threads,
                            &thread_id,
                            text,
                            active_turn_id,
                        )
                        .await
                        {
                            Ok(turn_id) => {
                                let _ = conversation_tx.send(ConversationEvent::PromptSubmitted {
                                    thread_id: thread_id.clone(),
                                    turn_id,
                                });
                                emit_conversation_load(
                                    &mut rpc,
                                    thread_id,
                                    &conversation_tx,
                                )
                                .await;
                            }
                            Err(error) => {
                                let _ = conversation_tx.send(ConversationEvent::Failed {
                                    thread_id,
                                    error: error.to_string(),
                                });
                            }
                        }
                    }
                    BackendCommand::InterruptTurn { thread_id, turn_id } => {
                        match interrupt_turn(&mut rpc, &thread_id, &turn_id).await {
                            Ok(()) => {
                                emit_conversation_load(
                                    &mut rpc,
                                    thread_id,
                                    &conversation_tx,
                                )
                                .await;
                            }
                            Err(error) => {
                                let _ = conversation_tx.send(ConversationEvent::Failed {
                                    thread_id,
                                    error: error.to_string(),
                                });
                            }
                        }
                    }
                }
            }
            _ = refresh.tick() => {
                match load_registry(&mut rpc).await {
                    Ok((fresh, loaded_supported)) => {
                        threads = by_id(fresh);
                        status.connected = true;
                        status.error = None;
                        status.last_refresh_unix_ms = Some(now_unix_ms());
                        if loaded_supported
                            && !status.capabilities.iter().any(|value| value == "thread/loaded/list")
                        {
                            status.capabilities.push("thread/loaded/list".into());
                            status.optional_capabilities_missing
                                .retain(|value| value != "thread/loaded/list");
                        }
                    }
                    Err(error) => {
                        status.error = Some(error.to_string());
                    }
                }
                generation = generation.saturating_add(1);
                let _ = tx.send(snapshot(generation, &threads, &status));
            }
            message = rpc.read_message() => {
                match message {
                    Ok(Some(message)) => {
                        if let Err(error) = handle_unsolicited(&mut rpc, message, &mut threads).await {
                            status.error = Some(error.to_string());
                        }
                        generation = generation.saturating_add(1);
                        let _ = tx.send(snapshot(generation, &threads, &status));
                    }
                    Ok(None) => {
                        status.connected = false;
                        status.error = Some("codex app-server closed stdout".into());
                        generation = generation.saturating_add(1);
                        let _ = tx.send(snapshot(generation, &threads, &status));
                        return;
                    }
                    Err(error) => {
                        status.connected = false;
                        status.error = Some(error.to_string());
                        generation = generation.saturating_add(1);
                        let _ = tx.send(snapshot(generation, &threads, &status));
                        return;
                    }
                }
            }
        }
    }
}

fn by_id(threads: Vec<ThreadSummary>) -> BTreeMap<String, ThreadSummary> {
    threads
        .into_iter()
        .map(|thread| (thread.id.0.clone(), thread))
        .collect()
}

fn snapshot(
    generation: u64,
    threads: &BTreeMap<String, ThreadSummary>,
    status: &BackendStatus,
) -> BackendSnapshot {
    BackendSnapshot {
        generation,
        threads: threads.values().cloned().collect(),
        status: status.clone(),
    }
}

async fn initialize(rpc: &mut RpcSession) -> Result<Value> {
    let result = rpc
        .request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "codex-tui",
                    "title": "Codex TUI",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": false
                }
            }),
        )
        .await
        .context("initialize codex app-server")?;
    tokio::time::timeout(RPC_REQUEST_TIMEOUT, rpc.notify("initialized", None))
        .await
        .context("initialized notification timed out")??;
    Ok(result)
}

fn status_from_initialize(result: &Value) -> BackendStatus {
    let version = result
        .pointer("/serverInfo/version")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            result
                .get("userAgent")
                .and_then(Value::as_str)
                .and_then(|value| value.split_whitespace().last())
                .map(ToOwned::to_owned)
        });
    let platform_family = result.get("platformFamily").and_then(Value::as_str);
    let platform_os = result.get("platformOs").and_then(Value::as_str);
    let platform = match (platform_family, platform_os) {
        (Some(family), Some(os)) => Some(format!("{family}/{os}")),
        (Some(family), None) => Some(family.to_string()),
        (None, Some(os)) => Some(os.to_string()),
        (None, None) => None,
    };

    BackendStatus {
        source: "codex-app-server".into(),
        connected: true,
        version,
        platform,
        codex_home: result
            .get("codexHome")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        capabilities: vec![],
        optional_capabilities_missing: vec![],
        last_refresh_unix_ms: None,
        error: None,
    }
}

async fn load_registry(rpc: &mut RpcSession) -> Result<(Vec<ThreadSummary>, bool)> {
    let loaded = load_all_loaded_ids(rpc).await;
    let (loaded_ids, loaded_supported) = match loaded {
        Ok(ids) => (Some(ids), true),
        Err(_) => (None, false),
    };

    let mut cursor: Option<String> = None;
    let mut raw_threads: Vec<ThreadWire> = Vec::new();
    loop {
        let result = rpc
            .request(
                "thread/list",
                json!({
                    "cursor": cursor,
                    "limit": PAGE_SIZE
                }),
            )
            .await?;
        let page = parse_thread_list(result)?;
        raw_threads.extend(page.data);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    Ok((
        raw_threads
            .into_iter()
            .map(|thread| normalize_thread(thread, loaded_ids.as_ref()))
            .collect(),
        loaded_supported,
    ))
}

async fn load_all_loaded_ids(rpc: &mut RpcSession) -> Result<BTreeSet<String>> {
    let mut cursor: Option<String> = None;
    let mut loaded = BTreeSet::new();
    loop {
        let result = rpc
            .request(
                "thread/loaded/list",
                json!({
                    "cursor": cursor,
                    "limit": PAGE_SIZE
                }),
            )
            .await?;
        let page = parse_loaded_list(result)?;
        loaded.extend(page.data);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    Ok(loaded)
}

async fn emit_conversation_load(
    rpc: &mut RpcSession,
    thread_id: ThreadId,
    tx: &mpsc::UnboundedSender<ConversationEvent>,
) {
    match load_conversation(rpc, thread_id.clone()).await {
        Ok(page) => {
            let _ = tx.send(ConversationEvent::Loaded(page));
        }
        Err(error) => {
            let _ = tx.send(ConversationEvent::Failed {
                thread_id,
                error: error.to_string(),
            });
        }
    }
}

async fn submit_prompt(
    rpc: &mut RpcSession,
    threads: &mut BTreeMap<String, ThreadSummary>,
    thread_id: &ThreadId,
    text: String,
    active_turn_id: Option<String>,
) -> Result<String> {
    if let Some(turn_id) = active_turn_id {
        let response = rpc
            .request(
                "turn/steer",
                json!({
                    "threadId": thread_id.0,
                    "input": [{
                        "type": "text",
                        "text": text,
                        "textElements": []
                    }],
                    "expectedTurnId": turn_id
                }),
            )
            .await
            .context("steer active turn")?;
        return response
            .get("turnId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .context("turn/steer response missing turnId");
    }

    ensure_thread_loaded(rpc, threads, thread_id).await?;
    let response = rpc
        .request(
            "turn/start",
            json!({
                "threadId": thread_id.0,
                "input": [{
                    "type": "text",
                    "text": text,
                    "textElements": []
                }]
            }),
        )
        .await
        .context("start turn")?;
    response
        .pointer("/turn/id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .context("turn/start response missing turn.id")
}

async fn ensure_thread_loaded(
    rpc: &mut RpcSession,
    threads: &mut BTreeMap<String, ThreadSummary>,
    thread_id: &ThreadId,
) -> Result<()> {
    let metadata = rpc
        .request(
            "thread/read",
            json!({
                "threadId": thread_id.0,
                "includeTurns": false
            }),
        )
        .await
        .context("read thread before turn start")?;
    let is_not_loaded = metadata.pointer("/thread/status/type").and_then(Value::as_str)
        == Some("notLoaded");
    if !is_not_loaded {
        return Ok(());
    }

    rpc.request(
        "thread/resume",
        json!({
            "threadId": thread_id.0,
            "excludeTurns": true
        }),
    )
    .await
    .context("resume thread before turn start")?;

    if let Some(thread) = threads.get_mut(&thread_id.0) {
        thread.metadata.loaded = Some(true);
    }
    Ok(())
}

async fn interrupt_turn(rpc: &mut RpcSession, thread_id: &ThreadId, turn_id: &str) -> Result<()> {
    rpc.request(
        "turn/interrupt",
        json!({
            "threadId": thread_id.0,
            "turnId": turn_id
        }),
    )
    .await
    .context("interrupt active turn")?;
    Ok(())
}

async fn load_conversation(rpc: &mut RpcSession, thread_id: ThreadId) -> Result<ConversationPage> {
    let metadata = rpc
        .request(
            "thread/read",
            json!({
                "threadId": thread_id.0,
                "includeTurns": false
            }),
        )
        .await
        .context("read thread metadata")?;
    let title = parse_thread_title(&metadata);

    let turns_result = rpc
        .request(
            "thread/turns/list",
            json!({
                "threadId": thread_id.0,
                "cursor": null,
                "limit": 20,
                "sortDirection": "desc",
                "itemsView": "notLoaded"
            }),
        )
        .await
        .context("list recent turns")?;
    let (mut turns, next_turn_cursor) = parse_turns_page(turns_result)?;
    turns.reverse();

    let items_result = rpc
        .request(
            "thread/items/list",
            json!({
                "threadId": thread_id.0,
                "cursor": null,
                "limit": 100,
                "sortDirection": "desc"
            }),
        )
        .await
        .context("list recent thread items")?;
    let (mut items, next_item_cursor) = parse_items_page(items_result)?;
    items.reverse();

    Ok(merge_history(
        thread_id,
        title,
        turns,
        items,
        next_turn_cursor,
        next_item_cursor,
    ))
}

async fn handle_unsolicited(
    rpc: &mut RpcSession,
    message: Value,
    threads: &mut BTreeMap<String, ThreadSummary>,
) -> Result<()> {
    if message.get("id").is_some() && message.get("method").is_some() {
        rpc.reject_server_request(&message).await?;
        return Ok(());
    }

    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Ok(());
    };
    if method != "thread/status/changed" {
        return Ok(());
    }
    let Some(params) = message.get("params") else {
        return Ok(());
    };
    let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(status) = params.get("status") else {
        return Ok(());
    };
    if let Some(thread) = threads.get_mut(thread_id) {
        apply_status(thread, status);
    }
    Ok(())
}

fn now_unix_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

struct RpcSession {
    _child: Child,
    reader: Lines<BufReader<ChildStdout>>,
    writer: BufWriter<ChildStdin>,
    next_id: u64,
    queued_messages: VecDeque<Value>,
}

impl RpcSession {
    async fn spawn(codex_bin: Option<OsString>) -> Result<Self> {
        let mut command = Command::new(codex_bin.unwrap_or_else(|| OsString::from("codex")));
        command
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().context("spawn codex app-server")?;
        let stdin = child
            .stdin
            .take()
            .context("codex app-server stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .context("codex app-server stdout unavailable")?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(_line)) = lines.next_line().await {}
            });
        }

        Ok(Self {
            _child: child,
            reader: BufReader::new(stdout).lines(),
            writer: BufWriter::new(stdin),
            next_id: 1,
            queued_messages: VecDeque::new(),
        })
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        tokio::time::timeout(
            RPC_REQUEST_TIMEOUT,
            self.request_without_timeout(method, params),
        )
        .await
        .with_context(|| {
            format!(
                "{method} timed out after {}s",
                RPC_REQUEST_TIMEOUT.as_secs()
            )
        })?
    }

    async fn request_without_timeout(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.write_message(&json!({
            "id": id,
            "method": method,
            "params": params
        }))
        .await?;

        loop {
            let message = self
                .read_wire_message()
                .await?
                .ok_or_else(|| anyhow!("codex app-server closed while waiting for {method}"))?;
            if message.get("id").and_then(Value::as_u64) == Some(id)
                && message.get("method").is_none()
            {
                if let Some(error) = message.get("error") {
                    return Err(anyhow!("{method} failed: {error}"));
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| anyhow!("{method} response missing result"));
            }
            if message.get("id").is_some() && message.get("method").is_some() {
                self.reject_server_request(&message).await?;
            } else if message.get("method").is_some() {
                self.queued_messages.push_back(message);
            }
        }
    }

    async fn notify(&mut self, method: &str, params: Option<Value>) -> Result<()> {
        let mut message = json!({"method": method});
        if let Some(params) = params {
            message["params"] = params;
        }
        self.write_message(&message).await
    }

    async fn read_message(&mut self) -> Result<Option<Value>> {
        if let Some(message) = self.queued_messages.pop_front() {
            return Ok(Some(message));
        }
        self.read_wire_message().await
    }

    async fn read_wire_message(&mut self) -> Result<Option<Value>> {
        loop {
            let Some(line) = self
                .reader
                .next_line()
                .await
                .context("read app-server stdout")?
            else {
                return Ok(None);
            };
            if line.trim().is_empty() {
                continue;
            }
            let value = serde_json::from_str(&line)
                .with_context(|| format!("decode app-server JSON line: {line}"))?;
            return Ok(Some(value));
        }
    }

    async fn reject_server_request(&mut self, request: &Value) -> Result<()> {
        let Some(id) = request.get("id").cloned() else {
            return Ok(());
        };
        self.write_message(&json!({
            "id": id,
            "error": {
                "code": -32601,
                "message": "codex-tui M1 registry is read-only"
            }
        }))
        .await
    }

    async fn write_message(&mut self, message: &Value) -> Result<()> {
        let mut encoded = serde_json::to_vec(message).context("encode app-server request")?;
        encoded.push(b'\n');
        self.writer
            .write_all(&encoded)
            .await
            .context("write app-server stdin")?;
        self.writer.flush().await.context("flush app-server stdin")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_metadata_is_capability_oriented_not_version_gated() {
        let status = status_from_initialize(&json!({
            "userAgent": "codex-cli 0.157.1",
            "codexHome": "/home/user/.codex",
            "platformFamily": "unix",
            "platformOs": "linux",
            "serverInfo": {"version": "0.157.1"}
        }));
        assert_eq!(status.version.as_deref(), Some("0.157.1"));
        assert_eq!(status.platform.as_deref(), Some("unix/linux"));
        assert!(status.capabilities.is_empty());
    }
}
