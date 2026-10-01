use anyhow::Result;
use codex_tui::{
    app::{Action, AppState, Effect, InputMode, ViewKind, reduce},
    app_server::{self, ConversationEvent, RegistryHandle},
    backend::{BackendStatus, CodexBackend, FakeBackend},
    conversation::{InteractiveRequestKind, InteractiveResolution},
    domain::{CwdLocality, classify_cwd, display_cwd},
    forge::{self, ForgeEvent, ForgeHandle},
    forge_mutation::{ForgeMutationEvent, ForgeMutationHandle},
    git::{self, GitEvent, GitHandle},
    goal::GoalStatus,
    i18n::{UiLanguage, pick},
    keymap::{Command, command_for_key},
    planning::{
        LocalNote, PlanningSnapshot, SavedView, ScratchState, SourceKind, SourceRef, WorkCardRecord,
    },
    sqlite_store::SqliteStore,
    store::{AppConfig, LocalStateV1, LocalStore},
    terminal::TerminalSession,
    terminal_drawer::TerminalDrawerRuntime,
    ui,
    worktree::{MutationEvent, MutationRequest, WorktreeMutationHandle},
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const OPERATOR_STATE_WRITE_BEHIND_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Default)]
struct OperatorStateWriteBehind {
    dirty_since: Option<Instant>,
}

impl OperatorStateWriteBehind {
    fn mark(&mut self) {
        self.mark_at(Instant::now());
    }

    fn mark_at(&mut self, now: Instant) {
        self.dirty_since.get_or_insert(now);
    }

    fn is_due(&self) -> bool {
        self.is_due_at(Instant::now())
    }

    fn is_due_at(&self, now: Instant) -> bool {
        self.dirty_since.is_some_and(|dirty_since| {
            now.saturating_duration_since(dirty_since) >= OPERATOR_STATE_WRITE_BEHIND_INTERVAL
        })
    }

    fn clear(&mut self) {
        self.dirty_since = None;
    }
}

struct RuntimeStore {
    sqlite: SqliteStore,
    writable: bool,
    error: Option<String>,
    operator_state_write_behind: OperatorStateWriteBehind,
}

struct StoreBootstrap {
    config: AppConfig,
    local: LocalStateV1,
    planning: PlanningSnapshot,
}

impl RuntimeStore {
    fn discover() -> Result<(Self, StoreBootstrap)> {
        let sqlite = SqliteStore::discover()?;
        let config = sqlite.load_config()?;

        let mut error = None;
        let local = match sqlite.load_state() {
            Ok(state) => state,
            Err(store_error) => {
                error = Some(format!("SQLite LocalStore unavailable: {store_error:#}"));
                LocalStateV1::default()
            }
        };
        let planning = if error.is_none() {
            match sqlite.load_planning_snapshot() {
                Ok(snapshot) => snapshot,
                Err(store_error) => {
                    error = Some(format!(
                        "SQLite planning store unavailable: {store_error:#}"
                    ));
                    PlanningSnapshot::default()
                }
            }
        } else {
            PlanningSnapshot::default()
        };
        let writable = error.is_none();

        Ok((
            Self {
                sqlite,
                writable,
                error,
                operator_state_write_behind: OperatorStateWriteBehind::default(),
            },
            StoreBootstrap {
                config,
                local,
                planning,
            },
        ))
    }

    fn defer_operator_state(&mut self) {
        if self.writable {
            self.operator_state_write_behind.mark();
        }
    }

    fn operator_state_flush_due(&self) -> bool {
        self.writable && self.operator_state_write_behind.is_due()
    }

    fn persist_operator_state(&mut self, state: &LocalStateV1) -> Option<String> {
        self.operator_state_write_behind.clear();
        if !self.writable {
            return None;
        }
        match self.sqlite.save_state(state) {
            Ok(()) => None,
            Err(error) => {
                let message = format!("SQLite operator-state write failed: {error:#}");
                self.writable = false;
                self.error = Some(message.clone());
                Some(message)
            }
        }
    }

    fn error(&self) -> Option<String> {
        self.error.clone()
    }

    fn sqlite_clone(&self) -> SqliteStore {
        self.sqlite.clone()
    }

    fn mutate_work_card<F>(
        &mut self,
        anchor: SourceRef,
        operation: F,
    ) -> Result<PlanningSnapshot, String>
    where
        F: FnOnce(&mut WorkCardRecord),
    {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }

        let result = (|| -> Result<PlanningSnapshot> {
            let mut card = match self.sqlite.work_card_for_anchor(&anchor)? {
                Some(card) => card,
                None => match anchor.kind {
                    SourceKind::CodexThread => WorkCardRecord::implicit_thread(
                        &codex_tui::domain::ThreadId::new(anchor.value.clone()),
                    ),
                    SourceKind::ScratchWork => WorkCardRecord {
                        local_id: anchor.value.clone(),
                        anchor: anchor.clone(),
                        links: vec![],
                        overlay: Default::default(),
                    },
                    _ => anyhow::bail!("local overlay unsupported for {:?}", anchor.kind),
                },
            };
            operation(&mut card);
            self.sqlite.upsert_work_card(&card)?;
            self.sqlite.load_planning_snapshot()
        })();

        match result {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                let message = format!("SQLite WorkCard write failed: {error:#}");
                self.writable = false;
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }

    fn snooze_work_card(
        &mut self,
        anchor: SourceRef,
        duration_ms: u64,
    ) -> Result<PlanningSnapshot, String> {
        let until = now_unix_ms().saturating_add(duration_ms);
        self.mutate_work_card(anchor, |card| {
            card.overlay.snooze_until_unix_ms = Some(until);
        })
    }

    fn apply_local_batch(
        &mut self,
        plan: &codex_tui::batch_local::LocalBatchPlan,
    ) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .apply_local_batch(plan)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        self.finish_planning_write(result, "local batch")
    }

    fn save_source_note(
        &mut self,
        owner: SourceRef,
        text: String,
    ) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = if text.trim().is_empty() {
            self.sqlite.delete_note(&owner)
        } else {
            self.sqlite.upsert_note(&LocalNote {
                owner,
                text,
                updated_at_unix_ms: now_unix_ms(),
            })
        }
        .and_then(|_| self.sqlite.load_planning_snapshot());
        self.finish_planning_write(result, "note")
    }

    fn update_scratch_note(
        &mut self,
        scratch_id: String,
        note: Option<String>,
    ) -> Result<PlanningSnapshot, String> {
        self.update_scratch(scratch_id, |scratch| {
            scratch.note = note;
        })
    }

    fn update_scratch_state(
        &mut self,
        scratch_id: String,
        state: ScratchState,
    ) -> Result<PlanningSnapshot, String> {
        self.update_scratch(scratch_id, |scratch| {
            scratch.state = state;
        })
    }

    fn update_scratch<F>(
        &mut self,
        scratch_id: String,
        operation: F,
    ) -> Result<PlanningSnapshot, String>
    where
        F: FnOnce(&mut codex_tui::planning::ScratchWork),
    {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = (|| -> Result<PlanningSnapshot> {
            let mut scratch = self
                .sqlite
                .load_planning_snapshot()?
                .scratch
                .into_iter()
                .find(|scratch| scratch.id == scratch_id)
                .ok_or_else(|| anyhow::anyhow!("ScratchWork does not exist: {scratch_id}"))?;
            operation(&mut scratch);
            scratch.updated_at_unix_ms = now_unix_ms();
            self.sqlite.update_scratch(&scratch)?;
            self.sqlite.load_planning_snapshot()
        })();
        self.finish_planning_write(result, "ScratchWork")
    }

    fn delete_scratch(&mut self, scratch_id: String) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .delete_scratch(&scratch_id)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        self.finish_planning_write(result, "ScratchWork delete")
    }

    fn create_bookmark(
        &mut self,
        source: SourceRef,
        label: Option<String>,
    ) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .create_bookmark(source, label.as_deref(), None)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        self.finish_planning_write(result, "bookmark")
    }

    fn finish_planning_write(
        &mut self,
        result: Result<PlanningSnapshot>,
        label: &str,
    ) -> Result<PlanningSnapshot, String> {
        match result {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                let message = format!("SQLite {label} write failed: {error:#}");
                self.writable = false;
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }

    fn save_view(&mut self, view: SavedView) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .save_view(&view)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        self.finish_planning_write(result, "SavedView")
    }

    fn delete_view(&mut self, view_id: String) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .delete_view(&view_id)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        self.finish_planning_write(result, "SavedView delete")
    }

    fn set_hot_slot(&mut self, slot: u8, target: SourceRef) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .set_hot_slot(slot, target)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        match result {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                let message = format!("SQLite hot-slot write failed: {error:#}");
                self.writable = false;
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }

    fn create_scratch(
        &mut self,
        title: String,
        workspace: Option<String>,
    ) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .create_scratch(&title, None, workspace.as_deref(), None)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        match result {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                let message = format!("SQLite ScratchWork write failed: {error:#}");
                self.writable = false;
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }
}

struct RuntimeServices {
    git: GitHandle,
    forge: ForgeHandle,
    forge_mutations: ForgeMutationHandle,
    mutations: WorktreeMutationHandle,
    terminal_drawer: TerminalDrawerRuntime,
    store: RuntimeStore,
}

impl RuntimeServices {
    fn new(store: RuntimeStore) -> Self {
        let sqlite = store.sqlite_clone();
        Self {
            git: GitHandle::start(),
            forge: ForgeHandle::start(),
            forge_mutations: ForgeMutationHandle::start(sqlite.clone()),
            mutations: WorktreeMutationHandle::start(sqlite),
            terminal_drawer: TerminalDrawerRuntime::default(),
            store,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();

    if matches!(args.as_slice(), [arg] if arg == "--version" || arg == "version") {
        println!("codex-tui {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "release") {
        let code = codex_tui::release::run_cli(&args[1..])?;
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "headless") {
        let code = codex_tui::headless::run(&args[1..]).await?;
        if code != codex_tui::headless::EXIT_OK {
            std::process::exit(code);
        }
        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "doctor")
        && args.get(1).is_some_and(|arg| arg == "compat")
    {
        let flags = &args[2..];
        if flags.iter().any(|arg| arg != "--json") {
            eprintln!("usage: codex-tui doctor compat [--json]");
            std::process::exit(codex_tui::headless::EXIT_USAGE);
        }
        let code =
            codex_tui::headless::doctor_compat(flags.iter().any(|arg| arg == "--json")).await?;
        if code != codex_tui::headless::EXIT_OK {
            std::process::exit(code);
        }
        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "doctor") {
        return doctor(args.get(1).map(String::as_str)).await;
    }

    let fake_mode = args.iter().any(|arg| arg == "--fake");
    run_app(fake_mode).await
}

async fn doctor(scope: Option<&str>) -> Result<()> {
    let store = SqliteStore::discover()?;
    let config = store.load_config()?;

    println!("codex-tui {}", env!("CARGO_PKG_VERSION"));
    println!("config: {}", store.config_path().display());
    println!("state: {}", store.db_path().display());
    println!("mouse: {}", config.ui.mouse);
    println!(
        "language: {} -> {}",
        config.ui.language.as_str(),
        config.ui.language.resolve().as_str()
    );

    match store.load_state() {
        Ok(state) => println!("operator-schemaVersion: {}", state.schema_version),
        Err(error) => println!("operator-state: DEGRADED · {error:#}"),
    }

    if scope == Some("store") {
        match store.health() {
            Ok(health) => {
                println!("store-backend: sqlite");
                println!("store-schema: {}", health.schema_version);
                println!("integrity: {}", health.integrity);
                println!(
                    "legacy-import: {}",
                    health.legacy_import.as_deref().unwrap_or("unknown")
                );
                match store.load_planning_snapshot() {
                    Ok(snapshot) => {
                        println!("work-cards: {}", snapshot.cards.len());
                        println!("scratch: {}", snapshot.scratch.len());
                        println!("saved-views: {}", snapshot.saved_views.len());
                    }
                    Err(error) => println!("planning: DEGRADED · {error:#}"),
                }
                match store.load_managed_worktrees() {
                    Ok(worktrees) => println!("managed-worktrees: {}", worktrees.len()),
                    Err(error) => println!("managed-worktrees: DEGRADED · {error:#}"),
                }
                match store.load_recent_operation_receipts(10) {
                    Ok(receipts) => {
                        println!("recent-operation-receipts: {}", receipts.len());
                        for receipt in receipts {
                            println!(
                                "receipt: {} · {} · {:?}",
                                receipt.operation_id,
                                receipt.plan.kind.label(),
                                receipt.state
                            );
                        }
                    }
                    Err(error) => println!("operation-receipts: DEGRADED · {error:#}"),
                }
                match store.load_recent_forge_mutation_receipts(10) {
                    Ok(receipts) => {
                        println!("recent-forge-mutation-receipts: {}", receipts.len());
                        for receipt in receipts {
                            println!(
                                "forge-receipt: {} · {} · {:?} · {}/{} · mr={}",
                                receipt.operation_id,
                                receipt.plan.kind.label(),
                                receipt.state,
                                receipt.plan.host,
                                receipt.plan.project_path,
                                receipt
                                    .plan
                                    .change_request_iid
                                    .map_or_else(|| "-".into(), |iid| iid.to_string())
                            );
                        }
                    }
                    Err(error) => {
                        println!("forge-mutation-receipts: DEGRADED · {error:#}");
                    }
                }
            }
            Err(error) => {
                println!("store-backend: sqlite");
                println!("integrity: DEGRADED");
                println!("error: {error:#}");
            }
        }
    } else if scope == Some("git") {
        let cwd = std::env::current_dir()?;
        let context = git::probe_context(
            codex_tui::domain::ThreadId::new("doctor"),
            cwd.to_string_lossy().into_owned(),
        )
        .await?;
        println!("git-repository: {}", context.is_repository);
        println!("cwd: {}", context.cwd);
        if let Some(repo) = context.repo {
            println!("git-common-dir: {}", repo.git_common_dir);
            println!("repo-root: {}", repo.primary_root);
        }
        if let Some(worktree) = context.worktree {
            println!("worktree: {}", worktree.canonical_path);
        }
        println!(
            "branch: {}",
            context.branch.as_deref().unwrap_or("<detached/none>")
        );
        println!("dirty: {}", context.dirty);
        println!("changed-files: {}", context.changes.len());
        if let Some(error) = context.error {
            println!("error: {error}");
        }
    } else if scope == Some("forge") {
        let cwd = std::env::current_dir()?;
        let snapshot = forge::doctor(cwd.to_string_lossy().into_owned()).await;
        println!(
            "forge-client: {}",
            snapshot.client_name.as_deref().unwrap_or("<unresolved>")
        );
        println!(
            "forge-client-version: {}",
            snapshot
                .client_version
                .as_deref()
                .unwrap_or("<unavailable>")
        );
        println!(
            "forge-authenticated: {}",
            snapshot
                .authenticated
                .map(|value| value.to_string())
                .as_deref()
                .unwrap_or("<unknown>")
        );
        println!(
            "forge-server-version: {}",
            snapshot.server_version.as_deref().unwrap_or("<unknown>")
        );
        println!(
            "forge-server-edition: {}",
            snapshot.server_edition.as_deref().unwrap_or("<unknown>")
        );
        println!(
            "forge-server-tier: {}",
            snapshot
                .server_tier
                .as_deref()
                .unwrap_or("<unknown/not-discoverable>")
        );
        if let Some(remote) = &snapshot.remote {
            println!("forge-remote: {}", remote.remote_name);
            println!("forge-host: {}", remote.host);
            println!("forge-path: {}", remote.path_with_namespace);
        } else {
            println!("forge-remote: <unresolved>");
        }
        let observation = &snapshot.observation;
        if let Some(identity) = &observation.identity {
            println!("provider: {}", identity.provider.label());
            println!("project-id: {}", identity.project_id);
            println!("project-path: {}", identity.path_with_namespace);
            println!("project-url: {}", identity.web_url);
        } else {
            println!("provider: unavailable");
        }
        println!("freshness: {}", observation.freshness.label());
        for (capability, state) in &observation.capabilities {
            println!("capability.{}: {}", capability.label(), state.label());
        }
        println!("recent-issues: {}", observation.issues.len());
        println!("open-merge-requests: {}", observation.change_requests.len());
        println!("recent-pipelines: {}", observation.pipelines.len());
        println!("issue-boards: {}", snapshot.boards.len());
        if let Some(error) = &observation.error {
            println!("error: {error}");
        }
    } else if scope == Some("terminal") {
        let capabilities = codex_tui::pty::capabilities();
        println!("terminal-backend: {}", capabilities.backend);
        println!("terminal-platform: {}", capabilities.platform);
        println!("default-program: {}", capabilities.default_program);
        println!("resize: {}", capabilities.resize);
        println!("input: {}", capabilities.input);
        println!("event-queue-capacity: {}", capabilities.bounded_event_queue);
        println!(
            "default-scrollback-bytes: {}",
            capabilities.default_scrollback_bytes
        );
    } else if scope == Some("presets") {
        let cwd = std::env::current_dir()?;
        let context = git::probe_context(
            codex_tui::domain::ThreadId::new("doctor"),
            cwd.to_string_lossy().into_owned(),
        )
        .await?;
        let Some(repo) = context.repo else {
            println!("launch-config: unavailable");
            println!("error: current directory is not inside a Git repository");
            return Ok(());
        };
        let repo_root = std::path::Path::new(&repo.primary_root);
        let config_path = repo_root.join(codex_tui::launch::REPO_CONFIG_FILE);
        println!("launch-config: {}", config_path.display());
        match codex_tui::launch::RepoLaunchConfig::load(repo_root) {
            Ok(config) => {
                println!("launch-config-version: {}", config.version);
                println!("launch-presets: {}", config.launches.len());
                for preset in config.launches {
                    println!(
                        "preset: {} · cwd={:?} · argv={}",
                        preset.name,
                        preset.cwd,
                        preset
                            .argv
                            .iter()
                            .map(|arg| format!("{arg:?}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                }
            }
            Err(error) => println!("launch-config: DEGRADED · {error:#}"),
        }
    } else if scope == Some("codex") {
        match app_server::probe(None).await {
            Ok(snapshot) => {
                print_backend_status(&snapshot.status);
                println!(
                    "codex-home: {}",
                    snapshot.status.codex_home.as_deref().unwrap_or("<unknown>")
                );
                println!("threads: {}", snapshot.threads.len());

                let mut local = 0usize;
                let mut foreign_windows = 0usize;
                let mut foreign_unix = 0usize;
                let mut stale = 0usize;
                let mut relative = 0usize;
                let mut empty = 0usize;
                let mut local_samples = Vec::new();
                let mut foreign_samples = Vec::new();

                for thread in &snapshot.threads {
                    match classify_cwd(&thread.metadata.cwd) {
                        CwdLocality::LocalDirectory => {
                            local += 1;
                            if local_samples.len() < 5 {
                                local_samples.push(display_cwd(&thread.metadata.cwd).to_string());
                            }
                        }
                        CwdLocality::ForeignWindows => {
                            foreign_windows += 1;
                            if foreign_samples.len() < 5 {
                                foreign_samples.push(display_cwd(&thread.metadata.cwd).to_string());
                            }
                        }
                        CwdLocality::ForeignUnix => {
                            foreign_unix += 1;
                            if foreign_samples.len() < 5 {
                                foreign_samples.push(display_cwd(&thread.metadata.cwd).to_string());
                            }
                        }
                        CwdLocality::NativeMissing => stale += 1,
                        CwdLocality::Relative => relative += 1,
                        CwdLocality::Empty => empty += 1,
                    }
                }

                println!("threads-local: {local}");
                println!("threads-foreign-windows: {foreign_windows}");
                println!("threads-foreign-unix: {foreign_unix}");
                println!("threads-stale: {stale}");
                println!("threads-relative: {relative}");
                println!("threads-empty-cwd: {empty}");
                for cwd in local_samples {
                    println!("local-cwd: {cwd}");
                }
                for cwd in foreign_samples {
                    println!("foreign-cwd: {cwd}");
                }
            }
            Err(error) => {
                println!("backend: codex-app-server");
                println!("connected: false");
                println!("error: {error:#}");
            }
        }
    } else {
        println!(
            "hint: run `codex-tui doctor codex`, `doctor git`, `doctor forge`, `doctor store`, `doctor presets`, or `doctor terminal`"
        );
    }
    Ok(())
}

fn print_backend_status(status: &BackendStatus) {
    println!("backend: {}", status.source);
    println!("connected: {}", status.connected);
    println!(
        "version: {}",
        status.version.as_deref().unwrap_or("unknown")
    );
    println!(
        "platform: {}",
        status.platform.as_deref().unwrap_or("unknown")
    );
    println!("capabilities: {}", status.capabilities.join(", "));
    if !status.optional_capabilities_missing.is_empty() {
        println!(
            "optional-missing: {}",
            status.optional_capabilities_missing.join(", ")
        );
    }
    if let Some(error) = &status.error {
        println!("error: {error}");
    }
}

async fn run_app(fake_mode: bool) -> Result<()> {
    let (store, bootstrap) = RuntimeStore::discover()?;
    let config = bootstrap.config;
    let local = bootstrap.local;

    let mut fake_backend = fake_mode.then(FakeBackend::seeded);
    let mut registry: Option<RegistryHandle> = None;
    let mut connect_task = None;

    let mut app = if let Some(fake) = &fake_backend {
        let snapshot = fake.snapshot();
        let mut app = AppState::new(snapshot.threads);
        app.backend_status = snapshot.status;
        app
    } else {
        connect_task = Some(tokio::spawn(app_server::start(None)));
        let mut app = AppState::new(vec![]);
        app.backend_status = BackendStatus::starting("codex-app-server");
        app
    };
    app.language = config.ui.language.resolve();
    app.apply_local_state(&local);
    reduce(&mut app, Action::PlanningSnapshotLoaded(bootstrap.planning));
    reduce(&mut app, Action::PlanningStoreDegraded(store.error()));
    reduce(
        &mut app,
        Action::ReconcilePlanning {
            now_unix_ms: now_unix_ms(),
        },
    );

    let mut services = RuntimeServices::new(store);
    let language = app.language;
    if let Err(error) = services.mutations.recover() {
        reduce(
            &mut app,
            Action::MutationNotice(format!(
                "{}: {error}",
                runtime_text(
                    language,
                    "worktree recovery unavailable",
                    "worktree 恢复不可用",
                )
            )),
        );
    }
    if let Err(error) = services.forge_mutations.recover() {
        reduce(
            &mut app,
            Action::MutationNotice(format!(
                "{}: {error}",
                runtime_text(
                    language,
                    "forge mutation recovery unavailable",
                    "Forge 变更恢复不可用",
                )
            )),
        );
    }
    let initial_git_effects = reduce(&mut app, Action::RefreshGitProjections);
    apply_effects(
        &mut app,
        registry.as_ref(),
        &mut services,
        initial_git_effects,
    )?;

    let mut terminal = TerminalSession::enter(config.ui.mouse)?;
    let mut last_fake_tick = Instant::now();
    let mut last_git_reconcile = Instant::now();
    let mut last_forge_reconcile = Instant::now();
    let mut needs_render = true;

    while !app.should_quit {
        if connect_task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
        {
            let Some(task) = connect_task.take() else {
                continue;
            };
            match task.await {
                Ok(Ok(started)) => {
                    reduce(&mut app, Action::ReplaceThreads(started.initial.threads));
                    reduce(&mut app, Action::BackendStatus(started.initial.status));
                    app.apply_local_state(&local);
                    registry = Some(started.handle);
                    let effects = reduce(&mut app, Action::RefreshGitProjections);
                    apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
                    reduce(
                        &mut app,
                        Action::ReconcilePlanning {
                            now_unix_ms: now_unix_ms(),
                        },
                    );
                }
                Ok(Err(error)) => {
                    reduce(
                        &mut app,
                        Action::BackendStatus(backend_error_status(error.to_string())),
                    );
                }
                Err(error) => {
                    let language = app.language;
                    reduce(
                        &mut app,
                        Action::BackendStatus(backend_error_status(format!(
                            "{}: {error}",
                            runtime_text(
                                language,
                                "App Server connection task failed",
                                "App Server 连接任务失败",
                            )
                        ))),
                    );
                }
            }
            needs_render = true;
        }

        let registry_changes = drain_registry(&mut app, registry.as_mut(), &mut services.store);
        needs_render |= registry_changes.any;
        if registry_changes.registry_projection {
            let effects = reduce(&mut app, Action::RefreshGitProjections);
            apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
        }
        if registry_changes.planning {
            reduce(
                &mut app,
                Action::ReconcilePlanning {
                    now_unix_ms: now_unix_ms(),
                },
            );
        }
        let git_changes = drain_git(&mut app, &mut services.git);
        needs_render |= git_changes.any;
        if git_changes.planning_projection {
            let effects = reduce(&mut app, Action::RefreshForgeProjections);
            apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
            reduce(
                &mut app,
                Action::ReconcilePlanning {
                    now_unix_ms: now_unix_ms(),
                },
            );
        }

        let forge_changed = drain_forge(&mut app, &mut services.forge);
        needs_render |= forge_changed;
        if forge_changed {
            reduce(
                &mut app,
                Action::ReconcilePlanning {
                    now_unix_ms: now_unix_ms(),
                },
            );
        }

        let forge_mutation_changes =
            drain_forge_mutations(&mut app, &mut services.forge_mutations);
        needs_render |= forge_mutation_changes.any;
        if forge_mutation_changes.planning_projection {
            let effects = reduce(&mut app, Action::RefreshForgeProjections);
            apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
            reduce(
                &mut app,
                Action::ReconcilePlanning {
                    now_unix_ms: now_unix_ms(),
                },
            );
        }

        let mutation_changes = drain_mutations(&mut app, &mut services.mutations);
        needs_render |= mutation_changes.any;
        if mutation_changes.planning_projection {
            let effects = reduce(&mut app, Action::RefreshGitProjections);
            apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
            reduce(
                &mut app,
                Action::ReconcilePlanning {
                    now_unix_ms: now_unix_ms(),
                },
            );
        }

        let terminal_changed = drain_terminal_drawer(&mut app, &mut services.terminal_drawer);
        needs_render |= terminal_changed;

        if services.store.operator_state_flush_due()
            && let Some(error) = services.store.persist_operator_state(&app.to_local_state())
        {
            reduce(&mut app, Action::PlanningStoreDegraded(Some(error)));
        }

        if last_git_reconcile.elapsed() >= Duration::from_secs(10) {
            let effects = reduce(&mut app, Action::RefreshActiveGitProjections);
            apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
            last_git_reconcile = Instant::now();
        }

        if last_forge_reconcile.elapsed() >= Duration::from_secs(15) {
            let effects = reduce(&mut app, Action::RefreshForgeProjections);
            let forge_projection_changed = !effects.is_empty();
            apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
            if forge_projection_changed {
                reduce(
                    &mut app,
                    Action::ReconcilePlanning {
                        now_unix_ms: now_unix_ms(),
                    },
                );
                needs_render = true;
            }
            last_forge_reconcile = Instant::now();
        }

        if let Some(fake) = fake_backend.as_mut()
            && last_fake_tick.elapsed() >= Duration::from_millis(900)
        {
            let snapshot = fake.tick();
            reduce(&mut app, Action::ReplaceThreads(snapshot.threads));
            reduce(&mut app, Action::BackendStatus(snapshot.status));
            let effects = reduce(&mut app, Action::RefreshGitProjections);
            apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
            reduce(
                &mut app,
                Action::ReconcilePlanning {
                    now_unix_ms: now_unix_ms(),
                },
            );
            last_fake_tick = Instant::now();
            needs_render = true;
        }

        if needs_render {
            terminal
                .terminal_mut()
                .draw(|frame| ui::render(frame, &app))?;
            needs_render = false;
        }

        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(key) => {
                    let effects = handle_key(&mut app, key);
                    if !effects.is_empty()
                        || matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
                    {
                        needs_render = true;
                    }
                    apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
                }
                Event::Paste(text) => {
                    let effects = handle_paste(&mut app, text);
                    if !effects.is_empty() {
                        needs_render = true;
                    }
                    apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
                }
                Event::Resize(cols, rows) => {
                    needs_render = true;
                    if app.terminal_drawer_open {
                        let effects = vec![Effect::TerminalResize(ui::terminal_drawer_pty_size(
                            cols, rows,
                        ))];
                        apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
                    }
                }
                _ => {}
            }
        }

        tokio::time::sleep(Duration::from_millis(16)).await;
    }

    if let Some(task) = connect_task {
        task.abort();
    }
    if let Some(error) = services.store.persist_operator_state(&app.to_local_state()) {
        reduce(&mut app, Action::PlanningStoreDegraded(Some(error)));
    }
    Ok(())
}

fn runtime_text(
    language: UiLanguage,
    english: &'static str,
    simplified_chinese: &'static str,
) -> &'static str {
    pick(language, english, simplified_chinese)
}

fn backend_error_status(error: String) -> BackendStatus {
    BackendStatus {
        source: "codex-app-server".into(),
        connected: false,
        version: None,
        platform: None,
        codex_home: None,
        capabilities: vec![],
        optional_capabilities_missing: vec![],
        registry_complete: false,
        last_refresh_unix_ms: None,
        error: Some(error),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RegistryDrainChanges {
    any: bool,
    registry_projection: bool,
    planning: bool,
}

fn conversation_event_changes_planning(event: &ConversationEvent) -> bool {
    match event {
        ConversationEvent::GoalObserved(_) | ConversationEvent::GoalCleared(_) => true,
        ConversationEvent::Loaded(_)
        | ConversationEvent::OlderLoaded(_)
        | ConversationEvent::InteractiveRequested(_)
        | ConversationEvent::InteractiveResolved { .. }
        | ConversationEvent::PromptSubmitted { .. }
        | ConversationEvent::Failed { .. } => false,
    }
}

fn drain_registry(
    app: &mut AppState,
    registry: Option<&mut RegistryHandle>,
    store: &mut RuntimeStore,
) -> RegistryDrainChanges {
    let Some(registry) = registry else {
        return RegistryDrainChanges::default();
    };
    let mut changes = RegistryDrainChanges::default();
    while let Some(snapshot) = registry.try_recv() {
        reduce(app, Action::ReplaceThreads(snapshot.threads));
        reduce(app, Action::BackendStatus(snapshot.status));
        changes.any = true;
        changes.registry_projection = true;
        changes.planning = true;
    }
    while let Some(event) = registry.try_recv_conversation() {
        changes.planning |= conversation_event_changes_planning(&event);
        match event {
            ConversationEvent::Loaded(page) => {
                reduce(app, Action::ConversationLoaded(page));
            }
            ConversationEvent::OlderLoaded(page) => {
                reduce(app, Action::OlderConversationLoaded(page));
            }
            ConversationEvent::InteractiveRequested(request) => {
                reduce(app, Action::InteractiveRequested(request));
            }
            ConversationEvent::InteractiveResolved { request_id } => {
                reduce(app, Action::InteractiveResolved { request_id });
            }
            ConversationEvent::GoalObserved(goal) => {
                reduce(app, Action::GoalObserved(goal));
            }
            ConversationEvent::GoalCleared(thread_id) => {
                reduce(app, Action::GoalCleared(thread_id));
            }
            ConversationEvent::PromptSubmitted { thread_id, .. } => {
                let effects = reduce(app, Action::PromptSubmitted { thread_id });
                for effect in effects {
                    if effect == Effect::PersistOperatorState
                        && let Some(error) = store.persist_operator_state(&app.to_local_state())
                    {
                        reduce(app, Action::PlanningStoreDegraded(Some(error)));
                    }
                }
            }
            ConversationEvent::Failed { thread_id, error } => {
                reduce(app, Action::ConversationFailed { thread_id, error });
            }
        }
        changes.any = true;
    }
    changes
}

#[cfg(test)]
mod registry_drain_classification_tests {
    use super::*;

    #[test]
    fn only_goal_conversation_events_require_planning_reconcile() {
        assert!(conversation_event_changes_planning(
            &ConversationEvent::GoalCleared(codex_tui::domain::ThreadId::new("thread"))
        ));
        assert!(!conversation_event_changes_planning(
            &ConversationEvent::Failed {
                thread_id: codex_tui::domain::ThreadId::new("thread"),
                error: "fixture".into(),
            }
        ));
        assert!(!conversation_event_changes_planning(
            &ConversationEvent::PromptSubmitted {
                thread_id: codex_tui::domain::ThreadId::new("thread"),
                turn_id: "turn".into(),
            }
        ));
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ProjectionDrainChanges {
    any: bool,
    planning_projection: bool,
}

fn git_event_changes_planning(event: &GitEvent) -> bool {
    match event {
        GitEvent::Context(_) => true,
        GitEvent::Review(_) => false,
    }
}

fn drain_git(app: &mut AppState, git: &mut GitHandle) -> ProjectionDrainChanges {
    let mut changes = ProjectionDrainChanges::default();
    while let Some(event) = git.try_recv() {
        changes.planning_projection |= git_event_changes_planning(&event);
        match event {
            GitEvent::Context(context) => {
                reduce(app, Action::GitContextLoaded(context));
            }
            GitEvent::Review(review) => {
                reduce(app, Action::GitReviewLoaded(review));
            }
        }
        changes.any = true;
    }
    changes
}

#[cfg(test)]
mod projection_drain_classification_tests {
    use super::*;

    #[test]
    fn only_projection_changing_events_require_planning_work() {
        assert!(git_event_changes_planning(&GitEvent::Context(
            codex_tui::git::GitContext::pending(
                codex_tui::domain::ThreadId::new("thread"),
                "/repo",
            )
        )));
        assert!(!git_event_changes_planning(&GitEvent::Review(
            codex_tui::git::GitReview::pending(
                codex_tui::domain::ThreadId::new("thread"),
                "/repo",
            )
        )));
        assert!(!forge_mutation_event_changes_planning(
            &ForgeMutationEvent::Notice("fixture".into())
        ));
        assert!(!mutation_event_changes_planning(
            &MutationEvent::ManagedWorktrees(vec![])
        ));
        assert!(!mutation_event_changes_planning(
            &MutationEvent::Notice("fixture".into())
        ));
    }
}

fn drain_forge(app: &mut AppState, forge: &mut ForgeHandle) -> bool {
    let mut changed = false;
    while let Some(event) = forge.try_recv() {
        match event {
            ForgeEvent::Observation(observation) => {
                reduce(app, Action::ForgeObservationLoaded(*observation));
            }
            ForgeEvent::Review(review) => {
                reduce(app, Action::ForgeReviewLoaded(review));
            }
        }
        changed = true;
    }
    changed
}

fn forge_mutation_event_changes_planning(event: &ForgeMutationEvent) -> bool {
    match event {
        ForgeMutationEvent::Receipt(_) => true,
        ForgeMutationEvent::Notice(_) => false,
    }
}

fn drain_forge_mutations(
    app: &mut AppState,
    mutations: &mut ForgeMutationHandle,
) -> ProjectionDrainChanges {
    let mut changes = ProjectionDrainChanges::default();
    while let Some(event) = mutations.try_recv() {
        changes.planning_projection |= forge_mutation_event_changes_planning(&event);
        match event {
            ForgeMutationEvent::Receipt(receipt) => {
                reduce(app, Action::ForgeMutationReceipt(receipt));
            }
            ForgeMutationEvent::Notice(notice) => {
                reduce(app, Action::MutationNotice(notice));
            }
        }
        changes.any = true;
    }
    changes
}

fn mutation_event_changes_planning(event: &MutationEvent) -> bool {
    match event {
        MutationEvent::Receipt(_) => true,
        MutationEvent::ManagedWorktrees(_) | MutationEvent::Notice(_) => false,
    }
}

fn drain_mutations(
    app: &mut AppState,
    mutations: &mut WorktreeMutationHandle,
) -> ProjectionDrainChanges {
    let mut changes = ProjectionDrainChanges::default();
    while let Some(event) = mutations.try_recv() {
        changes.planning_projection |= mutation_event_changes_planning(&event);
        match event {
            MutationEvent::Receipt(receipt) => {
                reduce(app, Action::MutationReceipt(receipt));
            }
            MutationEvent::ManagedWorktrees(records) => {
                reduce(app, Action::ManagedWorktreesLoaded(records));
            }
            MutationEvent::Notice(notice) => {
                reduce(app, Action::MutationNotice(notice));
            }
        }
        changes.any = true;
    }
    changes
}

fn drain_terminal_drawer(app: &mut AppState, terminal: &mut TerminalDrawerRuntime) -> bool {
    let Some(snapshot) = terminal.drain() else {
        return false;
    };
    reduce(app, Action::TerminalSnapshot(snapshot));
    true
}

fn apply_effects(
    app: &mut AppState,
    registry: Option<&RegistryHandle>,
    services: &mut RuntimeServices,
    effects: Vec<Effect>,
) -> Result<()> {
    let RuntimeServices {
        git,
        forge,
        forge_mutations,
        mutations,
        terminal_drawer,
        store,
    } = services;
    for effect in effects {
        match effect {
            Effect::PersistOperatorState => {
                if let Some(error) = store.persist_operator_state(&app.to_local_state()) {
                    reduce(app, Action::PlanningStoreDegraded(Some(error)));
                }
            }
            Effect::PersistOperatorStateDeferred => {
                store.defer_operator_state();
            }
            Effect::CreateScratch { title, workspace } => {
                match store.create_scratch(title, workspace) {
                    Ok(snapshot) => {
                        reduce(app, Action::PlanningSnapshotLoaded(snapshot));
                        reduce(
                            app,
                            Action::ReconcilePlanning {
                                now_unix_ms: now_unix_ms(),
                            },
                        );
                        reduce(app, Action::PlanningStoreDegraded(None));
                    }
                    Err(error) => {
                        reduce(app, Action::PlanningStoreDegraded(Some(error)));
                    }
                }
            }
            Effect::SnoozeWorkCard {
                anchor,
                duration_ms,
            } => match store.snooze_work_card(anchor, duration_ms) {
                Ok(snapshot) => {
                    reduce(app, Action::PlanningSnapshotLoaded(snapshot));
                    reduce(
                        app,
                        Action::ReconcilePlanning {
                            now_unix_ms: now_unix_ms(),
                        },
                    );
                    reduce(app, Action::PlanningStoreDegraded(None));
                }
                Err(error) => {
                    reduce(app, Action::PlanningStoreDegraded(Some(error)));
                }
            },
            Effect::SaveSourceNote { owner, text } => {
                apply_planning_store_result(app, store.save_source_note(owner, text));
            }
            Effect::UpdateScratchNote { scratch_id, note } => {
                apply_planning_store_result(app, store.update_scratch_note(scratch_id, note));
            }
            Effect::CreateBookmark { source, label } => {
                apply_planning_store_result(app, store.create_bookmark(source, label));
            }
            Effect::UpdateScratchState { scratch_id, state } => {
                apply_planning_store_result(app, store.update_scratch_state(scratch_id, state));
            }
            Effect::DeleteScratch { scratch_id } => {
                apply_planning_store_result(app, store.delete_scratch(scratch_id));
            }
            Effect::SaveSavedView { view } => {
                apply_planning_store_result(app, store.save_view(view));
            }
            Effect::DeleteSavedView { view_id } => {
                apply_planning_store_result(app, store.delete_view(view_id));
            }
            Effect::SetHotSlot { slot, target } => match store.set_hot_slot(slot, target) {
                Ok(snapshot) => {
                    reduce(app, Action::PlanningSnapshotLoaded(snapshot));
                    reduce(
                        app,
                        Action::ReconcilePlanning {
                            now_unix_ms: now_unix_ms(),
                        },
                    );
                    reduce(app, Action::PlanningStoreDegraded(None));
                }
                Err(error) => {
                    reduce(app, Action::PlanningStoreDegraded(Some(error)));
                }
            },
            Effect::ApplyLocalBatch(plan) => {
                let preview = plan.preview();
                match store.apply_local_batch(&plan) {
                    Ok(snapshot) => {
                        reduce(app, Action::PlanningSnapshotLoaded(snapshot));
                        reduce(
                            app,
                            Action::ReconcilePlanning {
                                now_unix_ms: now_unix_ms(),
                            },
                        );
                        reduce(app, Action::PlanningStoreDegraded(None));
                        reduce(
                            app,
                            Action::MutationNotice(format!(
                                "{} · {preview}",
                                runtime_text(
                                    app.language,
                                    "local batch applied",
                                    "本地批量操作已应用",
                                )
                            )),
                        );
                    }
                    Err(error) => {
                        reduce(app, Action::PlanningStoreDegraded(Some(error)));
                    }
                }
            }
            Effect::LoadLaunchPresets {
                repo_root,
                thread_cwd,
            } => match codex_tui::launch::RepoLaunchConfig::load(Path::new(&repo_root)) {
                Ok(config) => {
                    reduce(
                        app,
                        Action::LaunchPresetsLoaded {
                            repo_root,
                            thread_cwd,
                            presets: config.launches,
                        },
                    );
                }
                Err(error) => {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "{}: {error:#}",
                            runtime_text(
                                app.language,
                                "launch preset config unavailable",
                                "启动预设配置不可用",
                            )
                        )),
                    );
                }
            },
            Effect::PrepareLaunchPreset {
                preset,
                repo_root,
                thread_cwd,
            } => {
                match codex_tui::launch::LaunchPlan::build(
                    &preset,
                    Path::new(&repo_root),
                    Path::new(&thread_cwd),
                ) {
                    Ok(plan) => {
                        reduce(app, Action::LaunchPlanPrepared(plan));
                    }
                    Err(error) => {
                        reduce(
                            app,
                            Action::MutationNotice(format!(
                                "{}: {error:#}",
                                runtime_text(
                                    app.language,
                                    "cannot create launch preset plan",
                                    "无法创建启动预设计划",
                                )
                            )),
                        );
                    }
                }
            }
            Effect::ExecuteLaunchPreset(plan) => match plan.execute() {
                Ok(pid) => {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "{} · {} · pid={pid}",
                            runtime_text(app.language, "launch preset started", "启动预设已启动"),
                            plan.name
                        )),
                    );
                }
                Err(error) => {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "{}: {error:#}",
                            runtime_text(app.language, "launch preset failed", "启动预设失败")
                        )),
                    );
                }
            },
            Effect::OpenTerminalDrawer { cwd } => {
                let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
                let size = ui::terminal_drawer_pty_size(cols, rows);
                match terminal_drawer.open(&cwd, size) {
                    Ok(()) => {
                        reduce(app, Action::TerminalSnapshot(terminal_drawer.snapshot()));
                    }
                    Err(error) => {
                        reduce(
                            app,
                            Action::MutationNotice(format!(
                                "{}: {error:#}",
                                runtime_text(
                                    app.language,
                                    "terminal drawer open failed",
                                    "终端抽屉打开失败",
                                )
                            )),
                        );
                        reduce(app, Action::CloseTerminalDrawer);
                    }
                }
            }
            Effect::CloseTerminalDrawer => {
                terminal_drawer.close();
            }
            Effect::TerminalInput(bytes) => {
                if let Err(error) = terminal_drawer.send_input(bytes) {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "{}: {error:#}",
                            runtime_text(
                                app.language,
                                "terminal input unavailable",
                                "终端输入不可用",
                            )
                        )),
                    );
                }
            }
            Effect::TerminalPaste(text) => {
                if let Err(error) = terminal_drawer.send_paste(text) {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "{}: {error:#}",
                            runtime_text(
                                app.language,
                                "terminal paste unavailable",
                                "终端粘贴不可用",
                            )
                        )),
                    );
                }
            }
            Effect::TerminalResize(size) => {
                if let Err(error) = terminal_drawer.resize(size) {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "{}: {error:#}",
                            runtime_text(
                                app.language,
                                "terminal resize unavailable",
                                "终端调整大小不可用",
                            )
                        )),
                    );
                } else {
                    reduce(app, Action::TerminalSnapshot(terminal_drawer.snapshot()));
                }
            }
            Effect::TerminalScroll(delta) => {
                if let Err(error) = terminal_drawer.scroll(delta) {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "{}: {error:#}",
                            runtime_text(
                                app.language,
                                "terminal scroll unavailable",
                                "终端滚动不可用",
                            )
                        )),
                    );
                } else {
                    reduce(app, Action::TerminalSnapshot(terminal_drawer.snapshot()));
                }
            }
            Effect::RefreshGoal(thread_id) => {
                if let Some(registry) = registry
                    && let Err(error) = registry.refresh_goal(thread_id)
                {
                    reduce(
                        app,
                        Action::BackendStatus(backend_error_status(format!(
                            "{}: {error}",
                            runtime_text(
                                app.language,
                                "Goal refresh command failed",
                                "Goal 刷新命令失败",
                            )
                        ))),
                    );
                }
            }
            Effect::SetGoal {
                thread_id,
                objective,
                status,
            } => {
                if let Some(registry) = registry
                    && let Err(error) = registry.set_goal(thread_id, objective, status)
                {
                    reduce(
                        app,
                        Action::BackendStatus(backend_error_status(format!(
                            "{}: {error}",
                            runtime_text(
                                app.language,
                                "Goal update command failed",
                                "Goal 更新命令失败",
                            )
                        ))),
                    );
                }
            }
            Effect::ClearGoal(thread_id) => {
                if let Some(registry) = registry
                    && let Err(error) = registry.clear_goal(thread_id)
                {
                    reduce(
                        app,
                        Action::BackendStatus(backend_error_status(format!(
                            "{}: {error}",
                            runtime_text(
                                app.language,
                                "Goal clear command failed",
                                "Goal 清除命令失败",
                            )
                        ))),
                    );
                }
            }
            Effect::RefreshManagedWorktrees => {
                if let Err(error) = mutations.refresh_inventory() {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "{}: {error}",
                            runtime_text(
                                app.language,
                                "managed-worktree inventory refresh failed",
                                "受管 worktree 清单刷新失败",
                            )
                        )),
                    );
                }
            }
            Effect::ExecuteOperation(plan) => {
                let request = MutationRequest {
                    plan: *plan,
                    active_scopes: app.active_mutation_scopes(),
                };
                if let Err(error) = mutations.execute(request) {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "{}: {error}",
                            runtime_text(
                                app.language,
                                "managed-worktree execution dispatch failed",
                                "受管 worktree 执行分派失败",
                            )
                        )),
                    );
                }
            }
            Effect::ExecuteForgeOperation(request) => {
                if let Err(error) = forge_mutations.execute(*request) {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "{}: {error}",
                            runtime_text(
                                app.language,
                                "forge mutation execution dispatch failed",
                                "Forge 变更执行分派失败",
                            )
                        )),
                    );
                }
            }
            Effect::ProbeGit { thread_id, cwd } => {
                if let Err(error) = git.probe(thread_id.clone(), cwd.clone()) {
                    let mut context = codex_tui::git::GitContext::pending(thread_id, cwd);
                    context.observed_at_unix_ms = now_unix_ms();
                    context.error = Some(error.to_string());
                    reduce(app, Action::GitContextLoaded(context));
                }
            }
            Effect::ProbeForge { thread_id, cwd } => {
                if let Err(error) = forge.probe(thread_id.clone(), cwd.clone()) {
                    reduce(
                        app,
                        Action::ForgeObservationLoaded(forge::ForgeObservation::unavailable(
                            thread_id,
                            cwd,
                            error.to_string(),
                        )),
                    );
                }
            }
            Effect::ProbeForgeReview(target) => {
                let fallback = target.clone();
                if let Err(error) = forge.probe_review(target) {
                    reduce(
                        app,
                        Action::ForgeReviewLoaded(forge::ForgeReviewSummary {
                            thread_id: fallback.thread_id,
                            cwd: fallback.cwd,
                            change_request_iid: fallback.change_request_iid,
                            approvals_required: None,
                            approvals_left: None,
                            approved_by_count: 0,
                            changes_requested_by_count: 0,
                            discussions_total: 0,
                            unresolved_discussions: 0,
                            approvals_available: false,
                            discussions_available: false,
                            observed_at_unix_ms: now_unix_ms(),
                            error: Some(error.to_string()),
                        }),
                    );
                }
            }
            Effect::LoadGitReview { thread_id, cwd } => {
                if let Err(error) = git.load_review(thread_id.clone(), cwd) {
                    reduce(
                        app,
                        Action::ReviewError {
                            thread_id,
                            error: error.to_string(),
                        },
                    );
                }
            }
            Effect::OpenExternalEditor {
                thread_id,
                cwd,
                path,
            } => {
                if let Err(error) = open_external_editor(&cwd, &path) {
                    reduce(
                        app,
                        Action::ReviewError {
                            thread_id,
                            error: error.to_string(),
                        },
                    );
                }
            }
            Effect::LoadConversation(thread_id) => {
                if let Some(registry) = registry {
                    if let Err(error) = registry.load_conversation(thread_id.clone()) {
                        reduce(
                            app,
                            Action::ConversationFailed {
                                thread_id,
                                error: error.to_string(),
                            },
                        );
                    }
                } else {
                    reduce(
                        app,
                        Action::ConversationFailed {
                            thread_id,
                            error: runtime_text(
                                app.language,
                                "conversation backend unavailable",
                                "会话后端不可用",
                            )
                            .into(),
                        },
                    );
                }
            }
            Effect::StopWatchingConversation(thread_id) => {
                if let Some(registry) = registry
                    && let Err(error) = registry.stop_watching_conversation(thread_id)
                {
                    reduce(
                        app,
                        Action::BackendStatus(backend_error_status(format!(
                            "{}: {error}",
                            runtime_text(
                                app.language,
                                "stop conversation watch failed",
                                "停止会话监听失败",
                            )
                        ))),
                    );
                }
            }
            Effect::LoadOlderConversation {
                thread_id,
                turn_cursor,
                item_cursor,
            } => {
                if let Some(registry) = registry
                    && let Err(error) = registry.load_older_conversation(
                        thread_id.clone(),
                        turn_cursor,
                        item_cursor,
                    )
                {
                    reduce(
                        app,
                        Action::ConversationFailed {
                            thread_id,
                            error: error.to_string(),
                        },
                    );
                }
            }
            Effect::SubmitPrompt {
                thread_id,
                text,
                active_turn_id,
            } => {
                if let Some(registry) = registry {
                    if let Err(error) =
                        registry.submit_prompt(thread_id.clone(), text, active_turn_id)
                    {
                        reduce(
                            app,
                            Action::ConversationFailed {
                                thread_id,
                                error: error.to_string(),
                            },
                        );
                    }
                } else {
                    reduce(
                        app,
                        Action::ConversationFailed {
                            thread_id,
                            error: runtime_text(
                                app.language,
                                "conversation backend unavailable",
                                "会话后端不可用",
                            )
                            .into(),
                        },
                    );
                }
            }
            Effect::ResolveInteractive {
                request_id,
                resolution,
            } => {
                if let Some(registry) = registry
                    && let Err(error) = registry.resolve_interactive(request_id.clone(), resolution)
                {
                    reduce(
                        app,
                        Action::BackendStatus(backend_error_status(format!(
                            "{}: {error}",
                            runtime_text(
                                app.language,
                                "resolve interactive request failed",
                                "处理交互请求失败",
                            )
                        ))),
                    );
                }
            }
            Effect::InterruptTurn { thread_id, turn_id } => {
                if let Some(registry) = registry
                    && let Err(error) = registry.interrupt_turn(thread_id.clone(), turn_id)
                {
                    reduce(
                        app,
                        Action::ConversationFailed {
                            thread_id,
                            error: error.to_string(),
                        },
                    );
                }
            }
        }
    }
    Ok(())
}

fn apply_planning_store_result(app: &mut AppState, result: Result<PlanningSnapshot, String>) {
    match result {
        Ok(snapshot) => {
            reduce(app, Action::PlanningSnapshotLoaded(snapshot));
            reduce(
                app,
                Action::ReconcilePlanning {
                    now_unix_ms: now_unix_ms(),
                },
            );
            reduce(app, Action::PlanningStoreDegraded(None));
        }
        Err(error) => {
            reduce(app, Action::PlanningStoreDegraded(Some(error)));
        }
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn open_external_editor(cwd: &str, relative_path: &str) -> Result<()> {
    let editor =
        std::env::var_os("CODEX_TUI_EDITOR").unwrap_or_else(|| std::ffi::OsString::from("code"));
    if editor.to_string_lossy().trim().is_empty() {
        anyhow::bail!("CODEX_TUI_EDITOR is empty");
    }
    let path = Path::new(cwd).join(relative_path);
    if !path.exists() {
        anyhow::bail!("selected path does not exist: {}", path.display());
    }
    std::process::Command::new(editor)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(Into::into)
}

fn handle_paste(app: &mut AppState, text: String) -> Vec<Effect> {
    if text.is_empty() {
        return vec![];
    }
    if app.terminal_focused {
        return vec![Effect::TerminalPaste(text)];
    }
    if app.input_mode != InputMode::Normal {
        return reduce(app, Action::InputText(text));
    }
    vec![]
}

fn is_terminal_release_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::F(6))
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char(']'))
        || key.code == KeyCode::Char('\u{001d}')
}

fn handle_key(app: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return vec![];
    }

    if app.terminal_focused {
        if is_terminal_release_key(key) {
            return reduce(app, Action::SetTerminalFocus(false));
        }
        if key.modifiers.contains(KeyModifiers::SHIFT) && key.code == KeyCode::PageUp {
            return reduce(app, Action::TerminalScroll(10));
        }
        if key.modifiers.contains(KeyModifiers::SHIFT) && key.code == KeyCode::PageDown {
            return reduce(app, Action::TerminalScroll(-10));
        }
        return terminal_key_bytes(key)
            .map(|bytes| vec![Effect::TerminalInput(bytes)])
            .unwrap_or_default();
    }

    if app.launch_menu_open {
        return match key.code {
            KeyCode::Esc => reduce(app, Action::CloseLaunchPresets),
            KeyCode::Char('j') | KeyCode::Down => reduce(app, Action::MoveLaunchPreset(1)),
            KeyCode::Char('k') | KeyCode::Up => reduce(app, Action::MoveLaunchPreset(-1)),
            KeyCode::Enter => reduce(app, Action::SelectLaunchPreset),
            _ => vec![],
        };
    }

    if app.context_open {
        return match key.code {
            KeyCode::Esc => reduce(app, Action::CloseContext),
            KeyCode::Char('j') | KeyCode::Down => reduce(app, Action::MoveContext(1)),
            KeyCode::Char('k') | KeyCode::Up => reduce(app, Action::MoveContext(-1)),
            KeyCode::Enter => reduce(app, Action::ExecuteContext),
            _ => vec![],
        };
    }

    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
        return handle_command(app, Command::QuitOrInterrupt);
    }

    if app.input_mode != InputMode::Normal {
        let action = match key.code {
            KeyCode::Esc => Action::CancelInput,
            KeyCode::Enter => Action::CommitInput,
            KeyCode::Backspace => Action::InputBackspace,
            KeyCode::Char(character)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                Action::InputChar(character)
            }
            _ => return vec![],
        };
        return reduce(app, action);
    }

    if app.pending_launch_plan.is_some() {
        return match key.code {
            KeyCode::Char('y') => reduce(app, Action::ConfirmPendingOperation),
            KeyCode::Char('c') | KeyCode::Esc => reduce(app, Action::CancelPendingOperation),
            _ => vec![],
        };
    }

    if app.pending_local_batch.is_some() {
        return match key.code {
            KeyCode::Char('y') => reduce(app, Action::ConfirmPendingOperation),
            KeyCode::Char('c') | KeyCode::Esc => reduce(app, Action::CancelPendingOperation),
            _ => vec![],
        };
    }

    if app.pending_forge_operation.is_some() {
        return match key.code {
            KeyCode::Char('y') => reduce(app, Action::ConfirmPendingOperation),
            KeyCode::Char('c') | KeyCode::Esc => reduce(app, Action::CancelPendingOperation),
            _ => vec![],
        };
    }

    if app.goal_actions_open {
        let action = match key.code {
            KeyCode::Esc => Some(Action::CloseGoalActions),
            KeyCode::Enter | KeyCode::Char('e') => Some(Action::BeginGoalObjective),
            KeyCode::Char('p') => Some(Action::SetGoalStatus(GoalStatus::Paused)),
            KeyCode::Char('r') => Some(Action::SetGoalStatus(GoalStatus::Active)),
            KeyCode::Char('c') => Some(Action::ClearGoal),
            _ => None,
        };
        if let Some(action) = action {
            return reduce(app, action);
        }
        return vec![];
    }

    if app.view_kind() == ViewKind::Thread
        && let Some(request) = app.current_pending_request()
    {
        let action = match (&request.kind, key.code) {
            (InteractiveRequestKind::UserInput { .. }, KeyCode::Enter | KeyCode::Char('i')) => {
                Some(Action::BeginUserInput)
            }
            (InteractiveRequestKind::UserInput { .. }, KeyCode::Char('n')) => {
                Some(Action::ResolvePending(InteractiveResolution::Decline))
            }
            (InteractiveRequestKind::UserInput { .. }, KeyCode::Char('c')) => {
                Some(Action::ResolvePending(InteractiveResolution::Cancel))
            }
            (_, KeyCode::Char('y')) => Some(Action::ResolvePending(InteractiveResolution::Accept)),
            (_, KeyCode::Char('n')) => Some(Action::ResolvePending(InteractiveResolution::Decline)),
            (_, KeyCode::Char('c')) => Some(Action::ResolvePending(InteractiveResolution::Cancel)),
            _ => None,
        };
        if let Some(action) = action {
            return reduce(app, action);
        }
    }

    let Some(command) = command_for_key(key, app.view_kind()) else {
        return vec![];
    };
    handle_command(app, command)
}

fn terminal_key_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    let mut bytes = match key.code {
        KeyCode::Char(character) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if character.is_ascii() {
                let value = character as u8;
                if (b'@'..=b'_').contains(&value.to_ascii_uppercase()) {
                    vec![value.to_ascii_uppercase() & 0x1f]
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        KeyCode::Char(character) => {
            let mut buffer = [0_u8; 4];
            character.encode_utf8(&mut buffer).as_bytes().to_vec()
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        _ => return None,
    };

    if key.modifiers.contains(KeyModifiers::ALT) {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn handle_command(app: &mut AppState, command: Command) -> Vec<Effect> {
    let action = match command {
        Command::QuitOrInterrupt => match app.view_kind() {
            ViewKind::Registry => Action::Quit,
            ViewKind::Thread => Action::InterruptCurrent,
            ViewKind::Review
            | ViewKind::Workspace
            | ViewKind::ManagedWorktrees
            | ViewKind::Board
            | ViewKind::Scratch => Action::Back,
        },
        Command::Back => Action::Back,
        Command::Help => Action::ToggleHelp,
        Command::Search => Action::BeginSearch,
        Command::ToggleHostLocalFilter => Action::ToggleHostLocalFilter,
        Command::ToggleRepoBackedFilter => Action::ToggleRepoBackedFilter,
        Command::ToggleAllHistory => Action::ToggleAllHistory,
        Command::Next => match app.view_kind() {
            ViewKind::Review => Action::MoveReview(1),
            ViewKind::ManagedWorktrees => Action::MoveManagedWorktree(1),
            ViewKind::Board => Action::MovePlanningSelection(1),
            _ => Action::MoveSelection(1),
        },
        Command::Previous => match app.view_kind() {
            ViewKind::Review => Action::MoveReview(-1),
            ViewKind::ManagedWorktrees => Action::MoveManagedWorktree(-1),
            ViewKind::Board => Action::MovePlanningSelection(-1),
            _ => Action::MoveSelection(-1),
        },
        Command::Open => {
            if app.view_kind() == ViewKind::Board {
                Action::OpenPlanningSelected
            } else {
                Action::OpenSelected
            }
        }
        Command::NextAttention => Action::NextAttention,
        Command::QuickPrompt => Action::QuickPrompt,
        Command::MarkUnread => Action::MarkUnread,
        Command::TogglePin => Action::TogglePin,
        Command::EditAlias => Action::BeginAlias,
        Command::AcknowledgeAttention => Action::AcknowledgeAttention,
        Command::ApprovePending => Action::ResolvePending(InteractiveResolution::Accept),
        Command::DeclinePending => Action::ResolvePending(InteractiveResolution::Decline),
        Command::CancelPending => Action::ResolvePending(InteractiveResolution::Cancel),
        Command::AnswerPending => Action::BeginUserInput,
        Command::Board => Action::OpenBoard,
        Command::BoardLeft => Action::MoveBoardColumn(-1),
        Command::BoardRight => Action::MoveBoardColumn(1),
        Command::CycleSavedView => Action::CycleSavedView(1),
        Command::Review => Action::OpenReview,
        Command::Workspace => Action::OpenWorkspace,
        Command::ManagedWorktrees => Action::OpenManagedWorktrees,
        Command::CreateWorktree => Action::BeginCreateWorktree,
        Command::AdoptWorktree => Action::BeginAdoptCurrentWorktree,
        Command::RemoveWorktree => Action::BeginRemoveManagedWorktree,
        Command::DeleteBranch => Action::BeginDeleteBranch,
        Command::ConfirmOperation => Action::ConfirmPendingOperation,
        Command::CancelOperation => Action::CancelPendingOperation,
        Command::New => Action::BeginScratch,
        Command::Snooze => Action::BeginSnooze,
        Command::BeginHotSlotBind => Action::BeginHotSlotBind,
        Command::PageUp => {
            if app.view_kind() == ViewKind::Review {
                Action::ScrollReviewBy(-10)
            } else {
                Action::ScrollBy(-5)
            }
        }
        Command::PageDown => {
            if app.view_kind() == ViewKind::Review {
                Action::ScrollReviewBy(10)
            } else {
                Action::ScrollBy(5)
            }
        }
        Command::ToggleWordDiff => Action::ToggleReviewWordDiff,
        Command::ExternalEditor => Action::OpenReviewExternalEditor,
        Command::TerminalDrawer => Action::ToggleTerminalDrawer,
        Command::CloseTerminalDrawer => Action::CloseTerminalDrawer,
        Command::HotSlot(slot) => Action::UseHotSlot(slot),
        Command::ContextActions => Action::OpenContext,
        Command::Goal => Action::OpenGoalActions,
        Command::CommandPalette | Command::OpenExternal => return vec![],
    };
    reduce(app, action)
}

#[cfg(test)]
mod operator_state_write_behind_tests {
    use super::*;

    #[test]
    fn deferred_operator_state_coalesces_without_extending_the_flush_deadline() {
        let started = Instant::now();
        let mut write_behind = OperatorStateWriteBehind::default();

        write_behind.mark_at(started);
        write_behind.mark_at(started + Duration::from_millis(100));

        assert!(!write_behind.is_due_at(started + Duration::from_millis(249)));
        assert!(write_behind.is_due_at(started + Duration::from_millis(250)));

        write_behind.clear();
        assert!(!write_behind.is_due_at(started + Duration::from_secs(1)));
    }
}

#[cfg(test)]
mod accessibility_input_tests {
    use super::*;

    fn app() -> AppState {
        AppState::new(FakeBackend::seeded().snapshot().threads)
    }

    #[test]
    fn terminal_focus_traps_normal_app_shortcuts() {
        let mut app = app();
        app.terminal_drawer_open = true;
        app.terminal_focused = true;

        let effects = handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        );
        assert_eq!(effects, vec![Effect::TerminalInput(vec![b'?'])]);
        assert!(!app.show_help);

        let effects = handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL),
        );
        assert!(effects.is_empty());
        assert!(!app.terminal_focused);
        assert!(app.terminal_drawer_open);
    }

    #[test]
    fn terminal_focus_release_accepts_f6_ctrl_bracket_and_raw_group_separator() {
        for release_key in [
            KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('\u{001d}'), KeyModifiers::NONE),
        ] {
            let mut app = app();
            app.terminal_drawer_open = true;
            app.terminal_focused = true;

            let effects = handle_key(&mut app, release_key);
            assert!(effects.is_empty());
            assert!(!app.terminal_focused);
            assert!(app.terminal_drawer_open);
        }
    }

    #[test]
    fn terminal_focus_has_priority_over_other_open_overlays() {
        let mut app = app();
        app.terminal_drawer_open = true;
        app.terminal_focused = true;
        app.launch_menu_open = true;
        app.context_open = true;

        let effects = handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        );
        assert_eq!(effects, vec![Effect::TerminalInput(vec![b'j'])]);
        assert_eq!(app.launch_selected, 0);
        assert_eq!(app.context_selected, 0);
    }

    #[test]
    fn bracketed_paste_routes_to_terminal_or_active_editor_only() {
        let mut app = app();
        app.terminal_drawer_open = true;
        app.terminal_focused = true;
        assert_eq!(
            handle_paste(&mut app, "echo hi\n".into()),
            vec![Effect::TerminalPaste("echo hi\n".into())]
        );

        app.terminal_focused = false;
        reduce(&mut app, Action::OpenSelected);
        reduce(&mut app, Action::QuickPrompt);
        assert_eq!(
            handle_paste(&mut app, "hello\nworld".into()),
            vec![Effect::PersistOperatorStateDeferred]
        );
        let thread_id = app.current_thread_id().expect("thread");
        assert_eq!(
            app.thread_ui.get(&thread_id.0).expect("thread ui").draft,
            "hello\nworld"
        );

        reduce(&mut app, Action::CancelInput);
        assert!(handle_paste(&mut app, "ignored".into()).is_empty());
    }

    #[test]
    fn context_overlay_traps_unrelated_global_navigation() {
        let mut app = app();
        app.context_open = true;
        let original_view = app.view.clone();
        let original_selected = app.selected;

        let effects = handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        );
        assert!(effects.is_empty());
        assert_eq!(app.view, original_view);
        assert_eq!(app.selected, original_selected);
    }

    #[test]
    fn search_editor_consumes_printable_keys_before_global_commands() {
        let mut app = app();
        reduce(&mut app, Action::BeginSearch);

        let effects = handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        );
        assert!(effects.is_empty());
        assert_eq!(app.input_mode, InputMode::Search);
        assert!(app.input_buffer.ends_with('b'));
        assert_eq!(app.view_kind(), ViewKind::Registry);
    }
}
