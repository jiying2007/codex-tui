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
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

const PAGE_SIZE: u32 = 200;
const STARTUP_REGISTRY_PAGE_LIMIT: usize = 1;
const REGISTRY_HYDRATION_YIELD_INTERVAL: Duration = Duration::from_millis(10);
const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const GOAL_PROBE_INTERVAL: Duration = Duration::from_millis(250);
const GOAL_EAGER_PROBE_LIMIT: usize = 100;
const APP_SERVER_COMMAND_QUEUE_CAPACITY: usize = 64;
const APP_SERVER_CONVERSATION_QUEUE_CAPACITY: usize = 256;
const RPC_QUEUED_MESSAGE_CAPACITY: usize = 1024;
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

fn is_registry_query_optimization_unsupported(error: &anyhow::Error) -> bool {
    let Some(source) = error.downcast_ref::<RpcResponseError>() else {
        return false;
    };
    if source.method != "thread/list" || !matches!(source.code, Some(-32600 | -32602)) {
        return false;
    }
    let message = source.message.to_ascii_lowercase();
    [
        "sortkey",
        "sort key",
        "sortdirection",
        "sort direction",
        "usestatedbonly",
        "use state db only",
        "unknown field",
        "unexpected field",
    ]
    .into_iter()
    .any(|fragment| message.contains(fragment))
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
    rx: watch::Receiver<BackendSnapshot>,
    conversation_rx: mpsc::Receiver<ConversationEvent>,
    command_tx: mpsc::Sender<BackendCommand>,
    task: JoinHandle<()>,
}

impl RegistryHandle {
    pub fn try_recv(&mut self) -> Option<BackendSnapshot> {
        try_recv_latest_snapshot(&mut self.rx)
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
        queue_backend_command(&self.command_tx, command)
    }
}

fn queue_backend_command(tx: &mpsc::Sender<BackendCommand>, command: BackendCommand) -> Result<()> {
    tx.try_send(command).map_err(|error| match error {
        mpsc::error::TrySendError::Full(_) => anyhow!("App Server actor queue is full"),
        mpsc::error::TrySendError::Closed(_) => anyhow!("App Server actor is not available"),
    })
}

async fn send_conversation_event(tx: &mpsc::Sender<ConversationEvent>, event: ConversationEvent) {
    let _ = tx.send(event).await;
}

impl Drop for RegistryHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn try_recv_latest_snapshot(
    receiver: &mut watch::Receiver<BackendSnapshot>,
) -> Option<BackendSnapshot> {
    match receiver.has_changed() {
        Ok(true) => Some(receiver.borrow_and_update().clone()),
        Ok(false) | Err(_) => None,
    }
}

#[derive(Debug)]
struct RegistryHydration {
    cursor: String,
    loaded_ids: Option<BTreeSet<String>>,
    use_state_db_only: bool,
    optimized_query: bool,
    tombstones: BTreeSet<String>,
}

#[derive(Debug)]
struct RegistryReconcile {
    hydration: RegistryHydration,
    candidate: BTreeMap<String, ThreadSummary>,
    loaded_supported: bool,
    live_overrides: BTreeSet<String>,
}

#[derive(Debug)]
struct RegistryLoad {
    threads: Vec<ThreadSummary>,
    loaded_supported: bool,
    registry_complete: bool,
    hydration: Option<RegistryHydration>,
}

#[derive(Debug)]
struct RegistryPage {
    threads: Vec<ThreadSummary>,
    next_cursor: Option<String>,
    optimized_query: bool,
}

async fn bootstrap_registry(
    rpc: &mut RpcSession,
    max_pages: Option<usize>,
) -> Result<(Vec<ThreadSummary>, BackendStatus, Option<RegistryHydration>)> {
    let init = initialize(rpc).await?;
    let mut status = status_from_initialize(&init);
    let load = load_registry_with_page_limit(rpc, false, max_pages).await?;
    status.capabilities.push("thread/list".into());
    status.capabilities.push("thread/status/changed".into());
    if load.loaded_supported {
        status.capabilities.push("thread/loaded/list".into());
    } else {
        status
            .optional_capabilities_missing
            .push("thread/loaded/list".into());
    }
    status.connected = true;
    status.registry_complete = load.registry_complete;
    status.last_refresh_unix_ms = Some(now_unix_ms());
    Ok((load.threads, status, load.hydration))
}

pub async fn start(codex_bin: Option<OsString>) -> Result<StartedRegistry> {
    let mut rpc = RpcSession::spawn(codex_bin).await?;
    let (threads, status, hydration) =
        bootstrap_registry(&mut rpc, Some(STARTUP_REGISTRY_PAGE_LIMIT)).await?;

    let initial = BackendSnapshot {
        generation: 0,
        threads: threads.clone(),
        status: status.clone(),
    };
    let (tx, rx) = watch::channel(initial.clone());
    let (conversation_tx, conversation_rx) = mpsc::channel(APP_SERVER_CONVERSATION_QUEUE_CAPACITY);
    let (command_tx, command_rx) = mpsc::channel(APP_SERVER_COMMAND_QUEUE_CAPACITY);
    let task = tokio::spawn(run_registry_actor(
        rpc,
        threads,
        status,
        tx,
        conversation_tx,
        command_rx,
        hydration,
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
    let mut rpc = RpcSession::spawn(codex_bin).await?;
    let (threads, status, hydration) = bootstrap_registry(&mut rpc, None).await?;
    debug_assert!(hydration.is_none());
    Ok(BackendSnapshot {
        generation: 0,
        threads,
        status,
    })
}

fn eager_goal_probe_queue(
    threads: &BTreeMap<String, ThreadSummary>,
    goal_probed: &BTreeSet<String>,
) -> VecDeque<ThreadId> {
    let mut candidates = threads.values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .metadata
            .updated_at
            .cmp(&left.metadata.updated_at)
            .then_with(|| right.id.0.cmp(&left.id.0))
    });
    candidates
        .into_iter()
        .filter(|thread| !goal_probed.contains(&thread.id.0))
        .take(GOAL_EAGER_PROBE_LIMIT)
        .map(|thread| thread.id.clone())
        .collect()
}

fn reset_eager_goal_queue(
    threads: &BTreeMap<String, ThreadSummary>,
    goal_probed: &BTreeSet<String>,
    goal_queued: &mut BTreeSet<String>,
    goal_probe_queue: &mut VecDeque<ThreadId>,
) {
    *goal_probe_queue = eager_goal_probe_queue(threads, goal_probed);
    *goal_queued = goal_probe_queue
        .iter()
        .map(|thread_id| thread_id.0.clone())
        .collect();
}

fn apply_full_registry_refresh(
    fresh: Vec<ThreadSummary>,
    loaded_supported: bool,
    threads: &mut BTreeMap<String, ThreadSummary>,
    status: &mut BackendStatus,
) {
    *threads = by_id(fresh);
    status.connected = true;
    status.registry_complete = true;
    status.error = None;
    status.last_refresh_unix_ms = Some(now_unix_ms());
    if loaded_supported
        && !status
            .capabilities
            .iter()
            .any(|value| value == "thread/loaded/list")
    {
        status.capabilities.push("thread/loaded/list".into());
        status
            .optional_capabilities_missing
            .retain(|value| value != "thread/loaded/list");
    }
}

fn merge_registry_hydration_page(
    threads: &mut BTreeMap<String, ThreadSummary>,
    page: Vec<ThreadSummary>,
    tombstones: &BTreeSet<String>,
) {
    for incoming in page {
        if tombstones.contains(&incoming.id.0) {
            continue;
        }
        let replace = threads
            .get(&incoming.id.0)
            .is_none_or(|current| current.metadata.updated_at < incoming.metadata.updated_at);
        if replace {
            threads.insert(incoming.id.0.clone(), incoming);
        }
    }
}

fn observe_registry_hydration_message(hydration: &mut RegistryHydration, message: &Value) {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return;
    };
    match method {
        "thread/archived" | "thread/deleted" => {
            if let Some(thread_id) = message.pointer("/params/threadId").and_then(Value::as_str) {
                hydration.tombstones.insert(thread_id.to_string());
            }
        }
        "thread/unarchived" => {
            if let Some(thread_id) = message.pointer("/params/threadId").and_then(Value::as_str) {
                hydration.tombstones.remove(thread_id);
            }
        }
        "thread/started" => {
            if let Some(thread_id) = message.pointer("/params/thread/id").and_then(Value::as_str) {
                hydration.tombstones.remove(thread_id);
            }
        }
        _ => {}
    }
}

fn observe_registry_reconcile_message(reconcile: &mut RegistryReconcile, message: &Value) {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return;
    };
    let thread_id = if method == "thread/started" {
        message.pointer("/params/thread/id").and_then(Value::as_str)
    } else {
        message.pointer("/params/threadId").and_then(Value::as_str)
    };
    let Some(thread_id) = thread_id else {
        return;
    };

    match method {
        "thread/started"
        | "thread/status/changed"
        | "thread/name/updated"
        | "thread/project/updated"
        | "turn/started"
        | "turn/completed"
        | "item/completed" => {
            reconcile.live_overrides.insert(thread_id.to_string());
        }
        "thread/archived" | "thread/deleted" | "thread/unarchived" => {
            reconcile.live_overrides.remove(thread_id);
        }
        _ => {}
    }
}

fn finish_registry_reconcile(
    mut reconcile: RegistryReconcile,
    threads: &mut BTreeMap<String, ThreadSummary>,
    status: &mut BackendStatus,
) {
    for thread_id in &reconcile.hydration.tombstones {
        reconcile.candidate.remove(thread_id);
    }
    for thread_id in &reconcile.live_overrides {
        if let Some(thread) = threads.get(thread_id) {
            reconcile
                .candidate
                .insert(thread_id.clone(), thread.clone());
        }
    }
    apply_full_registry_refresh(
        reconcile.candidate.into_values().collect(),
        reconcile.loaded_supported,
        threads,
        status,
    );
}

async fn run_registry_actor(
    mut rpc: RpcSession,
    initial_threads: Vec<ThreadSummary>,
    mut status: BackendStatus,
    tx: watch::Sender<BackendSnapshot>,
    conversation_tx: mpsc::Sender<ConversationEvent>,
    mut command_rx: mpsc::Receiver<BackendCommand>,
    mut hydration: Option<RegistryHydration>,
) {
    let mut generation = 0_u64;
    let mut threads = by_id(initial_threads);
    let mut pending_requests: BTreeMap<RpcRequestId, PendingServerRequest> = BTreeMap::new();
    let mut watched_threads = BTreeSet::new();
    let mut goal_supported: Option<bool> = None;
    let mut goal_probed = BTreeSet::new();
    let mut goal_queued = BTreeSet::new();
    let mut goal_probe_queue = VecDeque::new();
    reset_eager_goal_queue(
        &threads,
        &goal_probed,
        &mut goal_queued,
        &mut goal_probe_queue,
    );

    let mut refresh = tokio::time::interval(REFRESH_INTERVAL);
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    refresh.tick().await;

    let mut hydration_tick = tokio::time::interval(REGISTRY_HYDRATION_YIELD_INTERVAL);
    hydration_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut hydration_pending_publish = false;
    let mut reconcile: Option<RegistryReconcile> = None;

    let mut goal_probe = tokio::time::interval(GOAL_PROBE_INTERVAL);
    goal_probe.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    goal_probe.tick().await;

    loop {
        let registry_page_can_advance =
            (hydration.is_some() || reconcile.is_some()) && !rpc.has_queued_messages();
        tokio::select! {
            biased;
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
                                send_conversation_event(
                                    &conversation_tx,
                                    ConversationEvent::OlderLoaded(page),
                                )
                                .await;
                            }
                            Err(error) => {
                                send_conversation_event(
                                    &conversation_tx,
                                    ConversationEvent::Failed {
                                        thread_id,
                                        error: error.to_string(),
                                    },
                                )
                                .await;
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
                                send_conversation_event(
                                    &conversation_tx,
                                    ConversationEvent::PromptSubmitted {
                                        thread_id: thread_id.clone(),
                                        turn_id,
                                    },
                                )
                                .await;
                                emit_conversation_load(
                                    &mut rpc,
                                    thread_id,
                                    &conversation_tx,
                                )
                                .await;
                            }
                            Err(error) => {
                                send_conversation_event(
                                    &conversation_tx,
                                    ConversationEvent::Failed {
                                        thread_id,
                                        error: error.to_string(),
                                    },
                                )
                                .await;
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
                                send_conversation_event(
                                    &conversation_tx,
                                    ConversationEvent::Failed {
                                        thread_id,
                                        error: error.to_string(),
                                    },
                                )
                                .await;
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
                                send_conversation_event(
                                    &conversation_tx,
                                    ConversationEvent::InteractiveResolved { request_id },
                                )
                                .await;
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
                                send_conversation_event(
                                    &conversation_tx,
                                    ConversationEvent::GoalObserved(goal),
                                )
                                .await;
                            }
                            Ok(None) => {
                                goal_supported = Some(true);
                                goal_probed.insert(thread_id.0.clone());
                                mark_goal_supported(&mut status);
                                send_conversation_event(
                                    &conversation_tx,
                                    ConversationEvent::GoalCleared(thread_id),
                                )
                                .await;
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
                                send_conversation_event(
                                    &conversation_tx,
                                    ConversationEvent::GoalObserved(goal),
                                )
                                .await;
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
                                send_conversation_event(
                                    &conversation_tx,
                                    ConversationEvent::GoalCleared(thread_id),
                                )
                                .await;
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
            _ = hydration_tick.tick(), if registry_page_can_advance => {
                if let Some(state) = hydration.as_mut() {
                    match load_registry_page(
                        &mut rpc,
                        Some(state.cursor.clone()),
                        state.use_state_db_only,
                        state.optimized_query,
                        state.loaded_ids.as_ref(),
                    )
                    .await
                    {
                        Ok(page) => {
                            let next_cursor = page.next_cursor;
                            state.optimized_query = page.optimized_query;
                            merge_registry_hydration_page(
                                &mut threads,
                                page.threads,
                                &state.tombstones,
                            );
                            status.last_refresh_unix_ms = Some(now_unix_ms());

                            if let Some(next_cursor) = next_cursor {
                                state.cursor = next_cursor;
                            } else {
                                hydration = None;
                                status.registry_complete = true;
                                if goal_supported != Some(false) {
                                    reset_eager_goal_queue(
                                        &threads,
                                        &goal_probed,
                                        &mut goal_queued,
                                        &mut goal_probe_queue,
                                    );
                                }
                            }
                            hydration_pending_publish = true;
                            hydration_tick.reset();
                        }
                        Err(error) => {
                            status.error = Some(format!("Registry hydration failed: {error}"));
                            hydration = None;
                            hydration_pending_publish = true;
                        }
                    }
                } else if let Some(reconcile_state) = reconcile.as_mut() {
                    let state = &mut reconcile_state.hydration;
                    match load_registry_page(
                        &mut rpc,
                        Some(state.cursor.clone()),
                        state.use_state_db_only,
                        state.optimized_query,
                        state.loaded_ids.as_ref(),
                    )
                    .await
                    {
                        Ok(page) => {
                            let next_cursor = page.next_cursor;
                            state.optimized_query = page.optimized_query;
                            merge_registry_hydration_page(
                                &mut reconcile_state.candidate,
                                page.threads,
                                &state.tombstones,
                            );

                            if let Some(next_cursor) = next_cursor {
                                state.cursor = next_cursor;
                            } else {
                                let finished = reconcile.take().expect("reconcile state");
                                finish_registry_reconcile(finished, &mut threads, &mut status);
                                if goal_supported != Some(false) {
                                    reset_eager_goal_queue(
                                        &threads,
                                        &goal_probed,
                                        &mut goal_queued,
                                        &mut goal_probe_queue,
                                    );
                                }
                                hydration_pending_publish = true;
                            }
                            hydration_tick.reset();
                        }
                        Err(error) => {
                            status.error = Some(format!("Registry reconcile failed: {error}"));
                            reconcile = None;
                            hydration_pending_publish = true;
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
                                send_conversation_event(
                                    &conversation_tx,
                                    ConversationEvent::GoalObserved(goal),
                                )
                                .await;
                            }
                            Ok(None) => {
                                goal_supported = Some(true);
                                goal_probed.insert(thread_id.0.clone());
                                mark_goal_supported(&mut status);
                                send_conversation_event(
                                    &conversation_tx,
                                    ConversationEvent::GoalCleared(thread_id),
                                )
                                .await;
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
            _ = refresh.tick(), if hydration.is_none() && reconcile.is_none() => {
                match load_registry_with_page_limit(&mut rpc, true, Some(1)).await {
                    Ok(load) if load.registry_complete => {
                        apply_full_registry_refresh(
                            load.threads,
                            load.loaded_supported,
                            &mut threads,
                            &mut status,
                        );
                        if goal_supported != Some(false) {
                            reset_eager_goal_queue(
                                &threads,
                                &goal_probed,
                                &mut goal_queued,
                                &mut goal_probe_queue,
                            );
                        }
                        generation = generation.saturating_add(1);
                        let _ = tx.send(snapshot(generation, &threads, &status));
                    }
                    Ok(load) => {
                        reconcile = Some(RegistryReconcile {
                            hydration: load.hydration.expect("incomplete Registry load"),
                            candidate: by_id(load.threads),
                            loaded_supported: load.loaded_supported,
                            live_overrides: BTreeSet::new(),
                        });
                        hydration_tick.reset();
                    }
                    Err(error) => {
                        status.error = Some(error.to_string());
                        generation = generation.saturating_add(1);
                        let _ = tx.send(snapshot(generation, &threads, &status));
                    }
                }
            }
            message = rpc.read_message() => {
                match message {
                    Ok(Some(message)) => {
                        if let Some(hydration) = hydration.as_mut() {
                            observe_registry_hydration_message(hydration, &message);
                        }
                        if let Some(reconcile) = reconcile.as_mut() {
                            observe_registry_hydration_message(&mut reconcile.hydration, &message);
                            observe_registry_reconcile_message(reconcile, &message);
                        }
                        let registry_changed = match handle_unsolicited(
                            &mut rpc,
                            message,
                            &mut threads,
                            &mut pending_requests,
                            &watched_threads,
                            &conversation_tx,
                        )
                        .await
                        {
                            Ok(changed) => changed,
                            Err(error) => {
                                status.error = Some(error.to_string());
                                true
                            }
                        };
                        if registry_changed {
                            generation = generation.saturating_add(1);
                            let _ = tx.send(snapshot(generation, &threads, &status));
                            hydration_pending_publish = false;
                        }
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

        if hydration_pending_publish && !rpc.has_queued_messages() {
            generation = generation.saturating_add(1);
            let _ = tx.send(snapshot(generation, &threads, &status));
            hydration_pending_publish = false;
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
        registry_complete: false,
        last_refresh_unix_ms: None,
        error: None,
    }
}

async fn load_registry(
    rpc: &mut RpcSession,
    use_state_db_only: bool,
) -> Result<(Vec<ThreadSummary>, bool)> {
    let load = load_registry_with_page_limit(rpc, use_state_db_only, None).await?;
    debug_assert!(load.registry_complete);
    debug_assert!(load.hydration.is_none());
    Ok((load.threads, load.loaded_supported))
}

async fn load_registry_page(
    rpc: &mut RpcSession,
    cursor: Option<String>,
    use_state_db_only: bool,
    mut optimized_query: bool,
    loaded_ids: Option<&BTreeSet<String>>,
) -> Result<RegistryPage> {
    let optimized_params = json!({
        "cursor": cursor,
        "limit": PAGE_SIZE,
        "sortKey": "recency_at",
        "sortDirection": "desc",
        "useStateDbOnly": use_state_db_only
    });
    let legacy_params = json!({
        "cursor": cursor,
        "limit": PAGE_SIZE
    });

    let result = if optimized_query {
        match rpc.request("thread/list", optimized_params).await {
            Ok(result) => result,
            Err(error) if is_registry_query_optimization_unsupported(&error) => {
                optimized_query = false;
                rpc.request("thread/list", legacy_params).await?
            }
            Err(error) => return Err(error),
        }
    } else {
        rpc.request("thread/list", legacy_params).await?
    };

    let page = parse_thread_list(result)?;
    Ok(RegistryPage {
        threads: page
            .data
            .into_iter()
            .map(|thread| normalize_thread(thread, loaded_ids))
            .collect(),
        next_cursor: page.next_cursor,
        optimized_query,
    })
}

async fn load_registry_with_page_limit(
    rpc: &mut RpcSession,
    use_state_db_only: bool,
    max_pages: Option<usize>,
) -> Result<RegistryLoad> {
    let loaded = load_all_loaded_ids(rpc).await;
    let (loaded_ids, loaded_supported) = match loaded {
        Ok(ids) => (Some(ids), true),
        Err(_) => (None, false),
    };

    let mut cursor: Option<String> = None;
    let mut threads = Vec::new();
    let mut optimized_query = true;
    let mut pages = 0_usize;

    loop {
        let page = load_registry_page(
            rpc,
            cursor,
            use_state_db_only,
            optimized_query,
            loaded_ids.as_ref(),
        )
        .await?;
        optimized_query = page.optimized_query;
        threads.extend(page.threads);
        pages = pages.saturating_add(1);

        let Some(next_cursor) = page.next_cursor else {
            return Ok(RegistryLoad {
                threads,
                loaded_supported,
                registry_complete: true,
                hydration: None,
            });
        };

        if max_pages.is_some_and(|limit| pages >= limit) {
            return Ok(RegistryLoad {
                threads,
                loaded_supported,
                registry_complete: false,
                hydration: Some(RegistryHydration {
                    cursor: next_cursor,
                    loaded_ids,
                    use_state_db_only,
                    optimized_query,
                    tombstones: BTreeSet::new(),
                }),
            });
        }
        cursor = Some(next_cursor);
    }
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
    tx: &mpsc::Sender<ConversationEvent>,
) {
    match load_conversation(rpc, thread_id.clone()).await {
        Ok(page) => {
            send_conversation_event(tx, ConversationEvent::Loaded(page)).await;
        }
        Err(error) => {
            send_conversation_event(
                tx,
                ConversationEvent::Failed {
                    thread_id,
                    error: error.to_string(),
                },
            )
            .await;
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

fn apply_registry_notification(
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

async fn handle_unsolicited(
    rpc: &mut RpcSession,
    message: Value,
    threads: &mut BTreeMap<String, ThreadSummary>,
    pending_requests: &mut BTreeMap<RpcRequestId, PendingServerRequest>,
    watched_threads: &BTreeSet<String>,
    conversation_tx: &mpsc::Sender<ConversationEvent>,
) -> Result<bool> {
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
            send_conversation_event(
                conversation_tx,
                ConversationEvent::InteractiveRequested(request),
            )
            .await;
            return Ok(false);
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

    let emitted_at_seconds = message
        .get("emittedAtMs")
        .and_then(Value::as_u64)
        .and_then(|millis| i64::try_from(millis / 1_000).ok());
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Ok(false);
    };
    let Some(params) = message.get("params") else {
        return Ok(false);
    };

    if method == "serverRequest/resolved" {
        if let Some(request_id) = params
            .get("requestId")
            .map(RpcRequestId::from_value)
            .transpose()?
        {
            pending_requests.remove(&request_id);
            send_conversation_event(
                conversation_tx,
                ConversationEvent::InteractiveResolved { request_id },
            )
            .await;
        }
        return Ok(false);
    }

    if method == "thread/goal/updated" {
        let goal = parse_goal_updated(params, now_unix_ms())?;
        send_conversation_event(conversation_tx, ConversationEvent::GoalObserved(goal)).await;
        return Ok(false);
    }

    if method == "thread/goal/cleared" {
        let thread_id = parse_goal_cleared_thread(params)?;
        send_conversation_event(conversation_tx, ConversationEvent::GoalCleared(thread_id)).await;
        return Ok(false);
    }

    if let Some(changed) = apply_registry_notification(method, params, emitted_at_seconds, threads)?
    {
        return Ok(changed);
    }

    let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
        return Ok(false);
    };

    let conversation_boundary =
        matches!(method, "turn/started" | "turn/completed" | "item/completed");
    if conversation_boundary && watched_threads.contains(thread_id) {
        emit_conversation_load(rpc, ThreadId::new(thread_id), conversation_tx).await;
    }

    if conversation_boundary && let Some(thread) = threads.get_mut(thread_id) {
        if let Some(updated_at) = emitted_at_seconds {
            thread.metadata.updated_at = thread.metadata.updated_at.max(updated_at);
        }
        return Ok(true);
    }

    Ok(false)
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

fn should_queue_rpc_message(message: &Value) -> bool {
    if message.get("id").is_some() && message.get("method").is_some() {
        return true;
    }

    matches!(
        message.get("method").and_then(Value::as_str),
        Some(
            "serverRequest/resolved"
                | "thread/goal/updated"
                | "thread/goal/cleared"
                | "thread/started"
                | "thread/status/changed"
                | "thread/archived"
                | "thread/deleted"
                | "thread/unarchived"
                | "thread/name/updated"
                | "thread/project/updated"
                | "turn/started"
                | "turn/completed"
                | "item/completed"
        )
    )
}

fn enqueue_rpc_message(queue: &mut VecDeque<Value>, message: Value) -> Result<()> {
    anyhow::ensure!(
        queue.len() < RPC_QUEUED_MESSAGE_CAPACITY,
        "App Server queued-message capacity exceeded ({RPC_QUEUED_MESSAGE_CAPACITY})"
    );
    queue.push_back(message);
    Ok(())
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
            if should_queue_rpc_message(&message) {
                enqueue_rpc_message(&mut self.queued_messages, message)?;
            }
        }
    }

    fn has_queued_messages(&self) -> bool {
        !self.queued_messages.is_empty()
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
                .with_context(|| format!("decode app-server JSON line ({} bytes)", line.len()))?;
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
    fn app_server_channels_are_bounded_in_production() {
        let source = include_str!("app_server.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        assert!(!production.contains("unbounded_channel"));
        assert!(!production.contains("UnboundedSender"));
        assert!(!production.contains("UnboundedReceiver"));
    }

    #[test]
    fn app_server_command_queue_reports_backpressure() {
        let (tx, _rx) = mpsc::channel(1);
        queue_backend_command(
            &tx,
            BackendCommand::StopWatchingConversation(ThreadId::new("first")),
        )
        .expect("first command");
        let error = queue_backend_command(
            &tx,
            BackendCommand::StopWatchingConversation(ThreadId::new("second")),
        )
        .expect_err("second command must hit bounded queue");
        assert!(error.to_string().contains("queue is full"));
    }

    #[tokio::test]
    async fn conversation_event_queue_backpressures_without_dropping() {
        let (tx, mut rx) = mpsc::channel(1);
        send_conversation_event(&tx, ConversationEvent::GoalCleared(ThreadId::new("first"))).await;

        let second_tx = tx.clone();
        let second = tokio::spawn(async move {
            send_conversation_event(
                &second_tx,
                ConversationEvent::GoalCleared(ThreadId::new("second")),
            )
            .await;
        });
        tokio::task::yield_now().await;
        assert!(
            !second.is_finished(),
            "second event must wait while the bounded queue is full"
        );

        assert!(matches!(
            rx.recv().await,
            Some(ConversationEvent::GoalCleared(thread_id)) if thread_id.0 == "first"
        ));
        tokio::time::timeout(Duration::from_secs(1), second)
            .await
            .expect("second send must complete after capacity is released")
            .expect("second sender task");
        assert!(matches!(
            rx.recv().await,
            Some(ConversationEvent::GoalCleared(thread_id)) if thread_id.0 == "second"
        ));
    }

    #[test]
    fn rpc_wait_queue_keeps_semantic_messages_and_drops_ignored_streaming_delta() {
        assert!(!should_queue_rpc_message(&json!({
            "method": "item/agentMessage/delta",
            "params": {"threadId": "thread-1", "delta": "x"}
        })));
        assert!(should_queue_rpc_message(&json!({
            "id": 42,
            "method": "item/fileChange/requestApproval",
            "params": {"threadId": "thread-1"}
        })));
        assert!(should_queue_rpc_message(&json!({
            "method": "serverRequest/resolved",
            "params": {"requestId": 42}
        })));
        assert!(should_queue_rpc_message(&json!({
            "method": "thread/status/changed",
            "params": {"threadId": "thread-1"}
        })));
        assert!(should_queue_rpc_message(&json!({
            "method": "turn/completed",
            "params": {"threadId": "thread-1"}
        })));
    }

    #[test]
    fn rpc_wait_queue_is_hard_bounded_without_dropping_existing_semantic_messages() {
        let mut queue = VecDeque::new();
        for index in 0..RPC_QUEUED_MESSAGE_CAPACITY {
            enqueue_rpc_message(
                &mut queue,
                json!({"method": "turn/completed", "params": {"threadId": index.to_string()}}),
            )
            .expect("queue semantic message");
        }
        let error = enqueue_rpc_message(
            &mut queue,
            json!({"method": "thread/status/changed", "params": {"threadId": "overflow"}}),
        )
        .expect_err("bounded queue must fail closed");
        assert!(error.to_string().contains("capacity exceeded"));
        assert_eq!(queue.len(), RPC_QUEUED_MESSAGE_CAPACITY);
        assert_eq!(
            queue
                .front()
                .and_then(|message| message.pointer("/params/threadId").and_then(Value::as_str)),
            Some("0")
        );
    }

    #[test]
    fn production_rpc_decode_error_does_not_echo_unbounded_wire_payload() {
        let source = include_str!("app_server.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        assert!(!production.contains("decode app-server JSON line: {line}"));
        assert!(production.contains("decode app-server JSON line ({} bytes)"));
    }

    #[test]
    fn registry_notifications_update_only_registry_relevant_state() {
        let mut threads = BTreeMap::new();
        assert_eq!(
            apply_registry_notification(
                "thread/started",
                &json!({
                    "thread": {
                        "id": "thread-new",
                        "preview": "New work",
                        "updatedAt": 7,
                        "status": {"type": "idle"},
                        "cwd": "/repo",
                        "source": "cli"
                    }
                }),
                Some(8),
                &mut threads,
            )
            .expect("started"),
            Some(true)
        );
        let thread = threads.get("thread-new").expect("new thread");
        assert_eq!(thread.title, "New work");
        assert_eq!(thread.metadata.loaded, Some(true));

        assert_eq!(
            apply_registry_notification(
                "thread/status/changed",
                &json!({
                    "threadId": "thread-new",
                    "status": {
                        "type": "active",
                        "activeFlags": ["waitingOnApproval"]
                    }
                }),
                Some(9),
                &mut threads,
            )
            .expect("status"),
            Some(true)
        );
        let thread = threads.get("thread-new").expect("updated thread");
        assert_eq!(thread.runtime, crate::domain::RuntimeStatus::WaitingHuman);
        assert_eq!(thread.metadata.updated_at, 9);

        assert_eq!(
            apply_registry_notification(
                "thread/name/updated",
                &json!({"threadId": "thread-new", "threadName": "Renamed"}),
                Some(10),
                &mut threads,
            )
            .expect("name"),
            Some(true)
        );
        assert_eq!(threads["thread-new"].title, "Renamed");

        assert_eq!(
            apply_registry_notification(
                "item/agentMessage/delta",
                &json!({"threadId": "thread-new", "delta": "ignored"}),
                Some(11),
                &mut threads,
            )
            .expect("delta"),
            None
        );

        assert_eq!(
            apply_registry_notification(
                "thread/archived",
                &json!({"threadId": "thread-new"}),
                Some(12),
                &mut threads,
            )
            .expect("archived"),
            Some(true)
        );
        assert!(!threads.contains_key("thread-new"));
    }

    #[test]
    fn registry_full_reconcile_is_low_frequency_fallback() {
        assert!(REFRESH_INTERVAL >= Duration::from_secs(5 * 60));
    }

    #[test]
    fn bootstrap_registry_completeness_follows_actual_pagination_exhaustion() {
        let source = include_str!("app_server.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        assert!(production.contains("status.registry_complete = load.registry_complete;"));
        assert!(production.contains("registry_complete: true"));
        assert!(production.contains("status.registry_complete = true;"));
    }

    #[test]
    fn startup_registry_hydration_is_pagewise_and_command_priority() {
        assert_eq!(STARTUP_REGISTRY_PAGE_LIMIT, 1);
        assert!(REGISTRY_HYDRATION_YIELD_INTERVAL <= Duration::from_millis(50));

        let source = include_str!("app_server.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        let actor = production
            .split("async fn run_registry_actor")
            .nth(1)
            .expect("registry actor");
        let command = actor
            .find("command = command_rx.recv()")
            .expect("command branch");
        let hydration = actor
            .find("_ = hydration_tick.tick(), if registry_page_can_advance")
            .expect("hydration branch");

        assert!(
            production
                .contains("bootstrap_registry(&mut rpc, Some(STARTUP_REGISTRY_PAGE_LIMIT)).await?")
        );
        assert!(production.contains("bootstrap_registry(&mut rpc, None).await?"));
        assert!(actor.contains("biased;"));
        assert!(command < hydration);
        assert!(actor.contains("hydration_tick.reset();"));
    }

    #[test]
    fn hydration_merge_preserves_newer_live_state_and_tombstones() {
        let mut current = FakeBackend::scaled(1)
            .snapshot()
            .threads
            .into_iter()
            .next()
            .expect("current");
        current.id = ThreadId::new("same");
        current.title = "live-newer".into();
        current.metadata.updated_at = 20;

        let mut stale = current.clone();
        stale.title = "stale-page".into();
        stale.metadata.updated_at = 10;

        let mut fresh = current.clone();
        fresh.id = ThreadId::new("fresh");
        fresh.title = "fresh-page".into();
        fresh.metadata.updated_at = 30;

        let mut tombstoned = current.clone();
        tombstoned.id = ThreadId::new("gone");
        tombstoned.metadata.updated_at = 30;

        let mut threads = BTreeMap::from([(current.id.0.clone(), current)]);
        let tombstones = BTreeSet::from(["gone".to_string()]);
        merge_registry_hydration_page(&mut threads, vec![stale, fresh, tombstoned], &tombstones);

        assert_eq!(threads["same"].title, "live-newer");
        assert_eq!(threads["fresh"].title, "fresh-page");
        assert!(!threads.contains_key("gone"));
    }

    #[test]
    fn hydration_tombstones_follow_archive_and_reappearance_notifications() {
        let mut hydration = RegistryHydration {
            cursor: "next".into(),
            loaded_ids: None,
            use_state_db_only: false,
            optimized_query: true,
            tombstones: BTreeSet::new(),
        };

        observe_registry_hydration_message(
            &mut hydration,
            &json!({"method": "thread/archived", "params": {"threadId": "thread-1"}}),
        );
        assert!(hydration.tombstones.contains("thread-1"));

        observe_registry_hydration_message(
            &mut hydration,
            &json!({
                "method": "thread/started",
                "params": {"thread": {"id": "thread-1"}}
            }),
        );
        assert!(!hydration.tombstones.contains("thread-1"));
    }

    #[test]
    fn periodic_registry_reconcile_is_pagewise() {
        let source = include_str!("app_server.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        assert!(production.contains(
            "load_registry_with_page_limit(&mut rpc, true, Some(1)).await"
        ));
        assert!(production.contains("reconcile: Option<RegistryReconcile>"));
        assert!(!production.contains(
            "_ = refresh.tick(), if hydration.is_none() =>"
        ));
    }

    #[test]
    fn reconcile_finalization_preserves_live_overrides_and_removals() {
        let mut base = FakeBackend::scaled(3).snapshot().threads;
        base[0].id = ThreadId::new("live");
        base[0].title = "live-new".into();
        base[0].metadata.updated_at = 30;
        base[1].id = ThreadId::new("removed");
        base[2].id = ThreadId::new("stable");

        let live_threads = by_id(base.clone());

        let mut stale_live = base[0].clone();
        stale_live.title = "stale-candidate".into();
        stale_live.metadata.updated_at = 10;
        let candidate = BTreeMap::from([
            ("live".to_string(), stale_live),
            ("removed".to_string(), base[1].clone()),
            ("stable".to_string(), base[2].clone()),
        ]);

        let reconcile = RegistryReconcile {
            hydration: RegistryHydration {
                cursor: "done".into(),
                loaded_ids: None,
                use_state_db_only: true,
                optimized_query: true,
                tombstones: BTreeSet::from(["removed".to_string()]),
            },
            candidate,
            loaded_supported: true,
            live_overrides: BTreeSet::from(["live".to_string()]),
        };

        let mut threads = live_threads;
        let mut status = BackendStatus::fake();
        finish_registry_reconcile(reconcile, &mut threads, &mut status);

        assert_eq!(threads["live"].title, "live-new");
        assert!(!threads.contains_key("removed"));
        assert!(threads.contains_key("stable"));
    }

    #[test]
    fn eager_goal_probe_queue_is_bounded_and_prefers_recent_threads() {
        let mut threads = BTreeMap::new();
        for index in 0..150 {
            let mut thread = FakeBackend::seeded()
                .snapshot()
                .threads
                .into_iter()
                .next()
                .expect("thread");
            thread.id = ThreadId::new(format!("thread-{index:03}"));
            thread.metadata.updated_at = index;
            threads.insert(thread.id.0.clone(), thread);
        }

        let queue = eager_goal_probe_queue(&threads, &BTreeSet::new());
        assert_eq!(queue.len(), GOAL_EAGER_PROBE_LIMIT);
        assert_eq!(queue.front().map(|id| id.0.as_str()), Some("thread-149"));
        assert_eq!(queue.back().map(|id| id.0.as_str()), Some("thread-050"));

        let probed = ["thread-149".to_string(), "thread-148".to_string()]
            .into_iter()
            .collect();
        let queue = eager_goal_probe_queue(&threads, &probed);
        assert_eq!(queue.len(), GOAL_EAGER_PROBE_LIMIT);
        assert_eq!(queue.front().map(|id| id.0.as_str()), Some("thread-147"));
    }

    #[test]
    fn registry_query_optimization_falls_back_only_for_invalid_thread_list_params() {
        let invalid = anyhow::Error::new(RpcResponseError {
            method: "thread/list".into(),
            code: Some(-32602),
            message: "unknown field useStateDbOnly".into(),
        });
        assert!(is_registry_query_optimization_unsupported(&invalid));

        let unrelated = anyhow::Error::new(RpcResponseError {
            method: "thread/list".into(),
            code: Some(-32000),
            message: "database unavailable".into(),
        });
        assert!(!is_registry_query_optimization_unsupported(&unrelated));

        let other_method = anyhow::Error::new(RpcResponseError {
            method: "thread/read".into(),
            code: Some(-32602),
            message: "unknown field".into(),
        });
        assert!(!is_registry_query_optimization_unsupported(&other_method));
    }

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
    fn registry_snapshot_delivery_coalesces_to_latest_value() {
        let initial = BackendSnapshot {
            generation: 0,
            threads: vec![],
            status: BackendStatus::fake(),
        };
        let (tx, mut rx) = watch::channel(initial);

        tx.send(BackendSnapshot {
            generation: 1,
            threads: vec![],
            status: BackendStatus::fake(),
        })
        .expect("send generation 1");
        tx.send(BackendSnapshot {
            generation: 2,
            threads: vec![],
            status: BackendStatus::fake(),
        })
        .expect("send generation 2");

        assert_eq!(
            try_recv_latest_snapshot(&mut rx)
                .expect("latest snapshot")
                .generation,
            2
        );
        assert!(try_recv_latest_snapshot(&mut rx).is_none());
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
