use crate::backend::{BackendSnapshot, BackendStatus};
use crate::codex_protocol::{
    ThreadWire, apply_status, normalize_thread, parse_loaded_list, parse_thread_list,
};
use crate::conversation::{
    ConversationPage, InteractiveRequest, InteractiveResolution, RpcRequestId, merge_history,
    parse_interactive_request, parse_items_page, parse_legacy_thread_read, parse_thread_title,
    parse_turns_page,
};
use crate::domain::{ThreadId, ThreadSummary};
use crate::goal::{
    GoalObservation, GoalStatus, parse_goal_cleared_thread, parse_goal_get, parse_goal_set,
    parse_goal_updated,
};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::fmt;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const PAGE_SIZE: u32 = 200;
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const GOAL_PROBE_INTERVAL: Duration = Duration::from_millis(250);
const RPC_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
struct RpcResponseError {
    method: String,
    code: Option<i64>,
    message: String,
}

impl fmt::Display for RpcResponseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            Some(code) => write!(f, "{} failed ({code}): {}", self.method, self.message),
            None => write!(f, "{} failed: {}", self.method, self.message),
        }
    }
}

impl std::error::Error for RpcResponseError {}

fn is_goal_unsupported(error: &anyhow::Error) -> bool {
    let Some(source) = error.downcast_ref::<RpcResponseError>() else {
        return false;
    };
    if source.code == Some(-32601) {
        return true;
    }
    if !matches!(source.code, Some(-32600 | -32602)) {
        return false;
    }
    source.message.to_ascii_lowercase().contains("goal")
}

fn mark_goal_supported(status: &mut BackendStatus) {
    for capability in ["thread/goal/get", "thread/goal/set", "thread/goal/clear"] {
        if !status.capabilities.iter().any(|value| value == capability) {
            status.capabilities.push(capability.into());
        }
        status
            .optional_capabilities_missing
            .retain(|value| value != capability);
    }
}

fn mark_goal_unsupported(status: &mut BackendStatus) {
    for capability in ["thread/goal/get", "thread/goal/set", "thread/goal/clear"] {
        if !status
            .optional_capabilities_missing
            .iter()
            .any(|value| value == capability)
        {
            status.optional_capabilities_missing.push(capability.into());
        }
        status.capabilities.retain(|value| value != capability);
    }
}

fn is_history_pagination_unsupported(error: &anyhow::Error) -> bool {
    let Some(source) = error.downcast_ref::<RpcResponseError>() else {
        return false;
    };
    if source.code == Some(-32601) {
        return true;
    }
    if !matches!(source.code, Some(-32600 | -32602)) {
        return false;
    }
    let message = source.message.to_ascii_lowercase();
    [
        "historymode",
        "history mode",
        "excludeturns",
        "exclude turns",
        "thread/turns/list",
        "thread/items/list",
    ]
    .into_iter()
    .any(|field| message.contains(field))
        || (message.contains("paginated")
            && ["unknown variant", "unsupported variant", "invalid enum"]
                .into_iter()
                .any(|fragment| message.contains(fragment)))
}

pub struct StartedRegistry {
    pub initial: BackendSnapshot,
    pub handle: RegistryHandle,
}

#[derive(Clone, Debug)]
pub enum BackendCommand {
    LoadConversation(ThreadId),
    StopWatchingConversation(ThreadId),
    LoadOlderConversation {
        thread_id: ThreadId,
        turn_cursor: Option<String>,
        item_cursor: Option<String>,
    },
    SubmitPrompt {
        thread_id: ThreadId,
        text: String,
        active_turn_id: Option<String>,
    },
    InterruptTurn {
        thread_id: ThreadId,
        turn_id: String,
    },
    ResolveInteractive {
        request_id: RpcRequestId,
        resolution: InteractiveResolution,
    },
    RefreshGoal(ThreadId),
    SetGoal {
        thread_id: ThreadId,
        objective: Option<String>,
        status: Option<GoalStatus>,
    },
    ClearGoal(ThreadId),
}

#[derive(Clone, Debug)]
pub enum ConversationEvent {
    Loaded(ConversationPage),
    OlderLoaded(ConversationPage),
    PromptSubmitted {
        thread_id: ThreadId,
        turn_id: String,
    },
    InteractiveRequested(InteractiveRequest),
    InteractiveResolved {
        request_id: RpcRequestId,
    },
    GoalObserved(GoalObservation),
    GoalCleared(ThreadId),
    Failed {
        thread_id: ThreadId,
        error: String,
    },
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

    pub fn stop_watching_conversation(&self, thread_id: ThreadId) -> Result<()> {
        self.send_command(BackendCommand::StopWatchingConversation(thread_id))
    }

    pub fn load_older_conversation(
        &self,
        thread_id: ThreadId,
        turn_cursor: Option<String>,
        item_cursor: Option<String>,
    ) -> Result<()> {
        self.send_command(BackendCommand::LoadOlderConversation {
            thread_id,
            turn_cursor,
            item_cursor,
        })
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

    pub fn resolve_interactive(
        &self,
        request_id: RpcRequestId,
        resolution: InteractiveResolution,
    ) -> Result<()> {
        self.send_command(BackendCommand::ResolveInteractive {
            request_id,
            resolution,
        })
    }

    pub fn refresh_goal(&self, thread_id: ThreadId) -> Result<()> {
        self.send_command(BackendCommand::RefreshGoal(thread_id))
    }

    pub fn set_goal(
        &self,
        thread_id: ThreadId,
        objective: Option<String>,
        status: Option<GoalStatus>,
    ) -> Result<()> {
        self.send_command(BackendCommand::SetGoal {
            thread_id,
            objective,
            status,
        })
    }

    pub fn clear_goal(&self, thread_id: ThreadId) -> Result<()> {
        self.send_command(BackendCommand::ClearGoal(thread_id))
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
    let mut pending_requests: BTreeMap<RpcRequestId, PendingServerRequest> = BTreeMap::new();
    let mut watched_threads = BTreeSet::new();
    let mut goal_supported: Option<bool> = None;
    let mut goal_probed = BTreeSet::new();
    let mut goal_queued = threads.keys().cloned().collect::<BTreeSet<_>>();
    let mut goal_probe_queue = threads
        .keys()
        .cloned()
        .map(ThreadId::new)
        .collect::<VecDeque<_>>();

    let mut refresh = tokio::time::interval(REFRESH_INTERVAL);
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    refresh.tick().await;

    let mut goal_probe = tokio::time::interval(GOAL_PROBE_INTERVAL);
    goal_probe.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    goal_probe.tick().await;

    loop {
        tokio::select! {
            command = command_rx.recv() => {
                let Some(command) = command else {
                    return;
                };
                match command {
                    BackendCommand::LoadConversation(thread_id) => {
                        watched_threads.insert(thread_id.0.clone());
                        emit_conversation_load(&mut rpc, thread_id, &conversation_tx).await;
                    }
                    BackendCommand::StopWatchingConversation(thread_id) => {
                        watched_threads.remove(&thread_id.0);
                    }
                    BackendCommand::LoadOlderConversation {
                        thread_id,
                        turn_cursor,
                        item_cursor,
                    } => {
                        watched_threads.insert(thread_id.0.clone());
                        match load_older_conversation(
                            &mut rpc,
                            thread_id.clone(),
                            turn_cursor,
                            item_cursor,
                        )
                        .await
                        {
                            Ok(page) => {
                                let _ = conversation_tx.send(ConversationEvent::OlderLoaded(page));
                            }
                            Err(error) => {
                                let _ = conversation_tx.send(ConversationEvent::Failed {
                                    thread_id,
                                    error: error.to_string(),
                                });
                            }
                        }
                    }
                    BackendCommand::SubmitPrompt {
                        thread_id,
                        text,
                        active_turn_id,
                    } => {
                        watched_threads.insert(thread_id.0.clone());
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
                    BackendCommand::ResolveInteractive {
                        request_id,
                        resolution,
                    } => {
                        match resolve_interactive(
                            &mut rpc,
                            &mut pending_requests,
                            &request_id,
                            resolution,
                        )
                        .await
                        {
                            Ok(()) => {
                                let _ = conversation_tx.send(
                                    ConversationEvent::InteractiveResolved { request_id },
                                );
                            }
                            Err(error) => {
                                status.error = Some(error.to_string());
                            }
                        }
                    }
                    BackendCommand::RefreshGoal(thread_id) => {
                        match load_goal(&mut rpc, thread_id.clone()).await {
                            Ok(Some(goal)) => {
                                goal_supported = Some(true);
                                goal_probed.insert(thread_id.0.clone());
                                mark_goal_supported(&mut status);
                                let _ = conversation_tx.send(ConversationEvent::GoalObserved(goal));
                            }
                            Ok(None) => {
                                goal_supported = Some(true);
                                goal_probed.insert(thread_id.0.clone());
                                mark_goal_supported(&mut status);
                                let _ = conversation_tx.send(ConversationEvent::GoalCleared(thread_id));
                            }
                            Err(error) if is_goal_unsupported(&error) => {
                                goal_supported = Some(false);
                                goal_probe_queue.clear();
                                goal_queued.clear();
                                mark_goal_unsupported(&mut status);
                            }
                            Err(error) => {
                                status.error = Some(format!("Goal refresh failed: {error}"));
                            }
                        }
                    }
                    BackendCommand::SetGoal {
                        thread_id,
                        objective,
                        status: goal_status,
                    } => {
                        match set_goal(
                            &mut rpc,
                            thread_id.clone(),
                            objective,
                            goal_status,
                        )
                        .await
                        {
                            Ok(goal) => {
                                goal_supported = Some(true);
                                goal_probed.insert(thread_id.0.clone());
                                mark_goal_supported(&mut status);
                                let _ = conversation_tx.send(ConversationEvent::GoalObserved(goal));
                            }
                            Err(error) if is_goal_unsupported(&error) => {
                                goal_supported = Some(false);
                                goal_probe_queue.clear();
                                goal_queued.clear();
                                mark_goal_unsupported(&mut status);
                            }
                            Err(error) => {
                                status.error = Some(format!("Goal update failed: {error}"));
                            }
                        }
                    }
                    BackendCommand::ClearGoal(thread_id) => {
                        match clear_goal(&mut rpc, &thread_id).await {
                            Ok(()) => {
                                goal_supported = Some(true);
                                goal_probed.insert(thread_id.0.clone());
                                mark_goal_supported(&mut status);
                                let _ = conversation_tx.send(ConversationEvent::GoalCleared(thread_id));
                            }
                            Err(error) if is_goal_unsupported(&error) => {
                                goal_supported = Some(false);
                                goal_probe_queue.clear();
                                goal_queued.clear();
                                mark_goal_unsupported(&mut status);
                            }
                            Err(error) => {
                                status.error = Some(format!("Goal clear failed: {error}"));
                            }
                        }
                    }
                }
            }
            _ = goal_probe.tick() => {
                if goal_supported != Some(false)
                    && let Some(thread_id) = goal_probe_queue.pop_front()
                {
                    goal_queued.remove(&thread_id.0);
                    if !goal_probed.contains(&thread_id.0) {
                        match load_goal(&mut rpc, thread_id.clone()).await {
                            Ok(Some(goal)) => {
                                goal_supported = Some(true);
                                goal_probed.insert(thread_id.0.clone());
                                mark_goal_supported(&mut status);
                                let _ = conversation_tx.send(ConversationEvent::GoalObserved(goal));
                            }
                            Ok(None) => {
                                goal_supported = Some(true);
                                goal_probed.insert(thread_id.0.clone());
                                mark_goal_supported(&mut status);
                                let _ = conversation_tx.send(
                                    ConversationEvent::GoalCleared(thread_id),
                                );
                            }
                            Err(error) if is_goal_unsupported(&error) => {
                                goal_supported = Some(false);
                                goal_probe_queue.clear();
                                goal_queued.clear();
                                mark_goal_unsupported(&mut status);
                            }
                            Err(_) => {
                                goal_probed.insert(thread_id.0);
                            }
                        }
                    }
                }
            }
            _ = refresh.tick() => {
                match load_registry(&mut rpc).await {
                    Ok((fresh, loaded_supported)) => {
                        threads = by_id(fresh);
                        if goal_supported != Some(false) {
                            for thread_id in threads.keys() {
                                if !goal_probed.contains(thread_id)
                                    && goal_queued.insert(thread_id.clone())
                                {
                                    goal_probe_queue.push_back(ThreadId::new(thread_id));
                                }
                            }
                        }
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
                        if let Err(error) = handle_unsolicited(
                            &mut rpc,
                            message,
                            &mut threads,
                            &mut pending_requests,
                            &watched_threads,
                            &conversation_tx,
                        )
                        .await
                        {
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

async fn load_goal(rpc: &mut RpcSession, thread_id: ThreadId) -> Result<Option<GoalObservation>> {
    let result = rpc
        .request(
            "thread/goal/get",
            json!({
                "threadId": thread_id.0
            }),
        )
        .await
        .context("get thread Goal")?;
    parse_goal_get(result, now_unix_ms())
}

async fn set_goal(
    rpc: &mut RpcSession,
    thread_id: ThreadId,
    objective: Option<String>,
    status: Option<GoalStatus>,
) -> Result<GoalObservation> {
    let mut params = serde_json::Map::new();
    params.insert("threadId".into(), json!(thread_id.0));
    if let Some(objective) = objective {
        params.insert("objective".into(), json!(objective));
    }
    if let Some(status) = status {
        params.insert("status".into(), json!(status.wire()));
    }
    let result = rpc
        .request("thread/goal/set", Value::Object(params))
        .await
        .context("set thread Goal")?;
    parse_goal_set(result, now_unix_ms())
}

async fn clear_goal(rpc: &mut RpcSession, thread_id: &ThreadId) -> Result<()> {
    let result = rpc
        .request(
            "thread/goal/clear",
            json!({
                "threadId": thread_id.0
            }),
        )
        .await
        .context("clear thread Goal")?;
    anyhow::ensure!(
        result.get("cleared").and_then(Value::as_bool) == Some(true),
        "thread/goal/clear did not confirm cleared=true"
    );
    Ok(())
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
    let is_not_loaded = metadata
        .pointer("/thread/status/type")
        .and_then(Value::as_str)
        == Some("notLoaded");
    if !is_not_loaded {
        return Ok(());
    }

    let resume = rpc
        .request(
            "thread/resume",
            json!({
                "threadId": thread_id.0,
                "excludeTurns": true
            }),
        )
        .await;
    match resume {
        Ok(_) => {}
        Err(error) if is_history_pagination_unsupported(&error) => {
            rpc.request(
                "thread/resume",
                json!({
                    "threadId": thread_id.0
                }),
            )
            .await
            .context("resume legacy thread before turn start")?;
        }
        Err(error) => return Err(error).context("resume thread before turn start"),
    }

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

async fn load_older_conversation(
    rpc: &mut RpcSession,
    thread_id: ThreadId,
    turn_cursor: Option<String>,
    item_cursor: Option<String>,
) -> Result<ConversationPage> {
    let (turns, next_turn_cursor) = if let Some(cursor) = turn_cursor {
        let result = rpc
            .request(
                "thread/turns/list",
                json!({
                    "threadId": thread_id.0,
                    "cursor": cursor,
                    "limit": 20,
                    "sortDirection": "desc",
                    "itemsView": "notLoaded"
                }),
            )
            .await
            .context("list older turns")?;
        let (mut turns, next_cursor) = parse_turns_page(result)?;
        turns.reverse();
        (turns, next_cursor)
    } else {
        (vec![], None)
    };

    let (items, next_item_cursor) = if let Some(cursor) = item_cursor {
        let result = rpc
            .request(
                "thread/items/list",
                json!({
                    "threadId": thread_id.0,
                    "cursor": cursor,
                    "limit": 100,
                    "sortDirection": "desc"
                }),
            )
            .await
            .context("list older items")?;
        let (mut items, next_cursor) = parse_items_page(result)?;
        items.reverse();
        (items, next_cursor)
    } else {
        (vec![], None)
    };

    Ok(merge_history(
        thread_id,
        None,
        turns,
        items,
        next_turn_cursor,
        next_item_cursor,
    ))
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

    let turns_result = match rpc
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
    {
        Ok(result) => result,
        Err(error) if is_history_pagination_unsupported(&error) => {
            return load_legacy_conversation(rpc, thread_id).await;
        }
        Err(error) => return Err(error).context("list recent turns"),
    };
    let (mut turns, next_turn_cursor) = parse_turns_page(turns_result)?;
    turns.reverse();

    let items_result = match rpc
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
    {
        Ok(result) => result,
        Err(error) if is_history_pagination_unsupported(&error) => {
            return load_legacy_conversation(rpc, thread_id).await;
        }
        Err(error) => return Err(error).context("list recent thread items"),
    };
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

async fn load_legacy_conversation(
    rpc: &mut RpcSession,
    thread_id: ThreadId,
) -> Result<ConversationPage> {
    let result = rpc
        .request(
            "thread/read",
            json!({
                "threadId": thread_id.0,
                "includeTurns": true
            }),
        )
        .await
        .context("read legacy thread history")?;
    parse_legacy_thread_read(result, thread_id)
}

#[derive(Clone, Debug)]
struct PendingServerRequest {
    method: String,
    params: Value,
}

async fn handle_unsolicited(
    rpc: &mut RpcSession,
    message: Value,
    threads: &mut BTreeMap<String, ThreadSummary>,
    pending_requests: &mut BTreeMap<RpcRequestId, PendingServerRequest>,
    watched_threads: &BTreeSet<String>,
    conversation_tx: &mpsc::UnboundedSender<ConversationEvent>,
) -> Result<()> {
    if message.get("id").is_some() && message.get("method").is_some() {
        if let Some(request) = parse_interactive_request(&message)? {
            let method = message
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            pending_requests.insert(
                request.request_id.clone(),
                PendingServerRequest { method, params },
            );
            let _ = conversation_tx.send(ConversationEvent::InteractiveRequested(request));
            return Ok(());
        }

        rpc.reject_request(
            message
                .get("id")
                .cloned()
                .context("server request missing id")?,
            "unsupported App Server request in codex-tui",
        )
        .await?;
        anyhow::bail!(
            "unsupported App Server request: {}",
            message
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }

    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(params) = message.get("params") else {
        return Ok(());
    };

    if method == "serverRequest/resolved" {
        if let Some(request_id) = params
            .get("requestId")
            .map(RpcRequestId::from_value)
            .transpose()?
        {
            pending_requests.remove(&request_id);
            let _ = conversation_tx.send(ConversationEvent::InteractiveResolved { request_id });
        }
        return Ok(());
    }

    if method == "thread/goal/updated" {
        let goal = parse_goal_updated(params, now_unix_ms())?;
        let _ = conversation_tx.send(ConversationEvent::GoalObserved(goal));
        return Ok(());
    }

    if method == "thread/goal/cleared" {
        let thread_id = parse_goal_cleared_thread(params)?;
        let _ = conversation_tx.send(ConversationEvent::GoalCleared(thread_id));
        return Ok(());
    }

    let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
        return Ok(());
    };

    if method == "thread/status/changed" {
        let Some(status) = params.get("status") else {
            return Ok(());
        };
        if let Some(thread) = threads.get_mut(thread_id) {
            apply_status(thread, status);
        }
        return Ok(());
    }

    if matches!(method, "turn/started" | "turn/completed" | "item/completed")
        && watched_threads.contains(thread_id)
    {
        emit_conversation_load(rpc, ThreadId::new(thread_id), conversation_tx).await;
    }

    Ok(())
}

async fn resolve_interactive(
    rpc: &mut RpcSession,
    pending_requests: &mut BTreeMap<RpcRequestId, PendingServerRequest>,
    request_id: &RpcRequestId,
    resolution: InteractiveResolution,
) -> Result<()> {
    let pending = pending_requests
        .get(request_id)
        .cloned()
        .context("interactive request is no longer pending")?;

    match pending.method.as_str() {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            let decision = match resolution {
                InteractiveResolution::Accept => "accept",
                InteractiveResolution::Decline => "decline",
                InteractiveResolution::Cancel => "cancel",
                InteractiveResolution::UserInput(_) => {
                    anyhow::bail!("user-input answer cannot resolve an approval")
                }
            };
            rpc.respond_result(request_id.to_value(), json!({"decision": decision}))
                .await?;
        }
        "item/permissions/requestApproval" => match resolution {
            InteractiveResolution::Accept => {
                let permissions = pending
                    .params
                    .get("permissions")
                    .cloned()
                    .context("permission request missing permissions")?;
                rpc.respond_result(
                    request_id.to_value(),
                    json!({
                        "permissions": permissions,
                        "scope": "turn"
                    }),
                )
                .await?;
            }
            InteractiveResolution::Decline | InteractiveResolution::Cancel => {
                rpc.respond_result(
                    request_id.to_value(),
                    json!({
                        "permissions": {},
                        "scope": "turn"
                    }),
                )
                .await?;
            }
            InteractiveResolution::UserInput(_) => {
                anyhow::bail!("user-input answer cannot resolve a permission request")
            }
        },
        "item/tool/requestUserInput" => match resolution {
            InteractiveResolution::UserInput(answers) => {
                let answers = answers
                    .into_iter()
                    .map(|(question_id, answers)| (question_id, json!({"answers": answers})))
                    .collect::<serde_json::Map<_, _>>();
                rpc.respond_result(request_id.to_value(), json!({"answers": answers}))
                    .await?;
            }
            InteractiveResolution::Decline | InteractiveResolution::Cancel => {
                rpc.reject_request(request_id.to_value(), "user input cancelled by user")
                    .await?;
            }
            InteractiveResolution::Accept => {
                anyhow::bail!("request_user_input requires explicit answers")
            }
        },
        other => anyhow::bail!("unsupported pending server request: {other}"),
    }

    pending_requests.remove(request_id);
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
                    let code = error.get("code").and_then(Value::as_i64);
                    let error_message = error
                        .get("message")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| error.to_string());
                    return Err(RpcResponseError {
                        method: method.to_string(),
                        code,
                        message: error_message,
                    }
                    .into());
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| anyhow!("{method} response missing result"));
            }
            if message.get("method").is_some() {
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

    async fn respond_result(&mut self, id: Value, result: Value) -> Result<()> {
        self.write_message(&json!({
            "id": id,
            "result": result
        }))
        .await
    }

    async fn reject_request(&mut self, id: Value, message: &str) -> Result<()> {
        self.write_message(&json!({
            "id": id,
            "error": {
                "code": -32000,
                "message": message
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
    use crate::backend::{CodexBackend, FakeBackend};

    #[test]
    fn history_compatibility_matches_official_error_semantics() {
        let method_not_found = anyhow::Error::new(RpcResponseError {
            method: "thread/turns/list".into(),
            code: Some(-32601),
            message: "method not found".into(),
        });
        assert!(is_history_pagination_unsupported(&method_not_found));

        let legacy_invalid_params = anyhow::Error::new(RpcResponseError {
            method: "thread/resume".into(),
            code: Some(-32602),
            message: "unknown field excludeTurns".into(),
        });
        assert!(is_history_pagination_unsupported(&legacy_invalid_params));

        let ordinary_failure = anyhow::Error::new(RpcResponseError {
            method: "thread/items/list".into(),
            code: Some(-32000),
            message: "database unavailable".into(),
        });
        assert!(!is_history_pagination_unsupported(&ordinary_failure));
    }

    #[test]
    fn goal_capability_degrades_only_for_unsupported_protocol_errors() {
        let method_not_found = anyhow::Error::new(RpcResponseError {
            method: "thread/goal/get".into(),
            code: Some(-32601),
            message: "method not found".into(),
        });
        assert!(is_goal_unsupported(&method_not_found));

        let old_invalid_params = anyhow::Error::new(RpcResponseError {
            method: "thread/goal/get".into(),
            code: Some(-32602),
            message: "unknown Goal request".into(),
        });
        assert!(is_goal_unsupported(&old_invalid_params));

        let ordinary_failure = anyhow::Error::new(RpcResponseError {
            method: "thread/goal/get".into(),
            code: Some(-32000),
            message: "database unavailable".into(),
        });
        assert!(!is_goal_unsupported(&ordinary_failure));
    }

    #[test]
    fn registry_snapshot_orders_newest_threads_first() {
        let mut threads = BTreeMap::new();
        for (id, updated_at) in [("thread-old", 10), ("thread-new", 30), ("thread-mid", 20)] {
            let mut thread = FakeBackend::seeded()
                .snapshot()
                .threads
                .into_iter()
                .next()
                .expect("thread");
            thread.id = ThreadId::new(id);
            thread.metadata.updated_at = updated_at;
            threads.insert(id.to_string(), thread);
        }

        let snapshot = snapshot(1, &threads, &BackendStatus::fake());
        assert_eq!(
            snapshot
                .threads
                .iter()
                .map(|thread| thread.id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["thread-new", "thread-mid", "thread-old"]
        );
    }

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
