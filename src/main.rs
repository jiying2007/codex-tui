use anyhow::Result;
use codex_tui::{
    app::{Action, AppState, Effect, InputMode, ViewKind, reduce},
    app_server::{self, ConversationEvent, RegistryHandle},
    backend::{BackendStatus, CodexBackend, FakeBackend},
    conversation::{InteractiveRequestKind, InteractiveResolution},
    forge::{self, ForgeEvent, ForgeHandle},
    forge_mutation::{ForgeMutationEvent, ForgeMutationHandle},
    git::{self, GitEvent, GitHandle},
    goal::GoalStatus,
    keymap::{Command, command_for_key},
    planning::{
        LocalNote, PlanningSnapshot, SavedView, ScratchState, SourceKind, SourceRef, WorkCardRecord,
    },
    sqlite_store::SqliteStore,
    store::{AppConfig, LocalStateV1, LocalStore},
    terminal::TerminalSession,
    ui,
    worktree::{MutationEvent, MutationRequest, WorktreeMutationHandle},
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct RuntimeStore {
    sqlite: SqliteStore,
    writable: bool,
    error: Option<String>,
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
            },
            StoreBootstrap {
                config,
                local,
                planning,
            },
        ))
    }

    fn persist_operator_state(&mut self, state: &LocalStateV1) -> Option<String> {
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
            store,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
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
            snapshot.client_version.as_deref().unwrap_or("<unavailable>")
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
    } else if scope == Some("codex") {
        match app_server::probe(None).await {
            Ok(snapshot) => {
                print_backend_status(&snapshot.status);
                println!("threads: {}", snapshot.threads.len());
            }
            Err(error) => {
                println!("backend: codex-app-server");
                println!("connected: false");
                println!("error: {error:#}");
            }
        }
    } else {
        println!(
            "hint: run `codex-tui doctor codex`, `doctor git`, `doctor forge`, or `doctor store`"
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
    if let Err(error) = services.mutations.recover() {
        reduce(
            &mut app,
            Action::MutationNotice(format!("worktree recovery unavailable: {error}")),
        );
    }
    if let Err(error) = services.forge_mutations.recover() {
        reduce(
            &mut app,
            Action::MutationNotice(format!("forge mutation recovery unavailable: {error}")),
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
    let mut last_forge_reconcile = Instant::now();
    let mut needs_render = true;

    while !app.should_quit {
        if connect_task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
        {
            let task = connect_task.take().expect("finished connect task");
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
                    reduce(
                        &mut app,
                        Action::BackendStatus(backend_error_status(format!(
                            "App Server connection task failed: {error}"
                        ))),
                    );
                }
            }
            needs_render = true;
        }

        let registry_changed = drain_registry(&mut app, registry.as_mut(), &mut services.store);
        needs_render |= registry_changed;
        if registry_changed {
            let effects = reduce(&mut app, Action::RefreshGitProjections);
            apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
            reduce(
                &mut app,
                Action::ReconcilePlanning {
                    now_unix_ms: now_unix_ms(),
                },
            );
        }
        let git_changed = drain_git(&mut app, &mut services.git);
        needs_render |= git_changed;
        if git_changed {
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

        let forge_mutation_changed = drain_forge_mutations(&mut app, &mut services.forge_mutations);
        needs_render |= forge_mutation_changed;
        if forge_mutation_changed {
            let effects = reduce(&mut app, Action::RefreshForgeProjections);
            apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
            reduce(
                &mut app,
                Action::ReconcilePlanning {
                    now_unix_ms: now_unix_ms(),
                },
            );
        }

        let mutation_changed = drain_mutations(&mut app, &mut services.mutations);
        needs_render |= mutation_changed;
        if mutation_changed {
            let effects = reduce(&mut app, Action::RefreshGitProjections);
            apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
            reduce(
                &mut app,
                Action::ReconcilePlanning {
                    now_unix_ms: now_unix_ms(),
                },
            );
        }

        if last_forge_reconcile.elapsed() >= Duration::from_secs(15) {
            let effects = reduce(&mut app, Action::RefreshForgeProjections);
            apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
            reduce(
                &mut app,
                Action::ReconcilePlanning {
                    now_unix_ms: now_unix_ms(),
                },
            );
            last_forge_reconcile = Instant::now();
            needs_render = true;
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
                Event::Resize(_, _) => needs_render = true,
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

fn backend_error_status(error: String) -> BackendStatus {
    BackendStatus {
        source: "codex-app-server".into(),
        connected: false,
        version: None,
        platform: None,
        codex_home: None,
        capabilities: vec![],
        optional_capabilities_missing: vec![],
        last_refresh_unix_ms: None,
        error: Some(error),
    }
}

fn drain_registry(
    app: &mut AppState,
    registry: Option<&mut RegistryHandle>,
    store: &mut RuntimeStore,
) -> bool {
    let Some(registry) = registry else {
        return false;
    };
    let mut changed = false;
    while let Some(snapshot) = registry.try_recv() {
        reduce(app, Action::ReplaceThreads(snapshot.threads));
        reduce(app, Action::BackendStatus(snapshot.status));
        changed = true;
    }
    while let Some(event) = registry.try_recv_conversation() {
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
        changed = true;
    }
    changed
}

fn drain_git(app: &mut AppState, git: &mut GitHandle) -> bool {
    let mut changed = false;
    while let Some(event) = git.try_recv() {
        match event {
            GitEvent::Context(context) => {
                reduce(app, Action::GitContextLoaded(context));
            }
            GitEvent::Review(review) => {
                reduce(app, Action::GitReviewLoaded(review));
            }
        }
        changed = true;
    }
    changed
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

fn drain_forge_mutations(app: &mut AppState, mutations: &mut ForgeMutationHandle) -> bool {
    let mut changed = false;
    while let Some(event) = mutations.try_recv() {
        match event {
            ForgeMutationEvent::Receipt(receipt) => {
                reduce(app, Action::ForgeMutationReceipt(receipt));
            }
            ForgeMutationEvent::Notice(notice) => {
                reduce(app, Action::MutationNotice(notice));
            }
        }
        changed = true;
    }
    changed
}

fn drain_mutations(app: &mut AppState, mutations: &mut WorktreeMutationHandle) -> bool {
    let mut changed = false;
    while let Some(event) = mutations.try_recv() {
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
        changed = true;
    }
    changed
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
        store,
    } = services;
    for effect in effects {
        match effect {
            Effect::PersistOperatorState => {
                if let Some(error) = store.persist_operator_state(&app.to_local_state()) {
                    reduce(app, Action::PlanningStoreDegraded(Some(error)));
                }
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
            Effect::RefreshGoal(thread_id) => {
                if let Some(registry) = registry
                    && let Err(error) = registry.refresh_goal(thread_id)
                {
                    reduce(
                        app,
                        Action::BackendStatus(backend_error_status(format!(
                            "Goal refresh command failed: {error}"
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
                            "Goal update command failed: {error}"
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
                            "Goal clear command failed: {error}"
                        ))),
                    );
                }
            }
            Effect::RefreshManagedWorktrees => {
                if let Err(error) = mutations.refresh_inventory() {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "managed-worktree inventory refresh failed: {error}"
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
                            "managed-worktree execution dispatch failed: {error}"
                        )),
                    );
                }
            }
            Effect::ExecuteForgeOperation(request) => {
                if let Err(error) = forge_mutations.execute(*request) {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "forge mutation execution dispatch failed: {error}"
                        )),
                    );
                }
            }
            Effect::ProbeGit { thread_id, cwd } => {
                if let Err(error) = git.probe(thread_id.clone(), cwd.clone()) {
                    let mut context = codex_tui::git::GitContext::pending(thread_id, cwd);
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
            Effect::ProbeForgeReview {
                thread_id,
                cwd,
                provider,
                host,
                project_id,
                project_path,
                change_request_iid,
            } => {
                if let Err(error) = forge.probe_review(
                    thread_id.clone(),
                    cwd.clone(),
                    provider,
                    host,
                    project_id,
                    project_path,
                    change_request_iid,
                ) {
                    reduce(
                        app,
                        Action::ForgeReviewLoaded(forge::ForgeReviewSummary {
                            thread_id,
                            cwd,
                            change_request_iid,
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
                            error: "conversation backend unavailable".into(),
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
                            "stop conversation watch failed: {error}"
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
                            error: "conversation backend unavailable".into(),
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
                            "resolve interactive request failed: {error}"
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

fn handle_key(app: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return vec![];
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
        Command::HotSlot(slot) => Action::UseHotSlot(slot),
        Command::ContextActions => Action::OpenContext,
        Command::Goal => Action::OpenGoalActions,
        Command::CommandPalette | Command::OpenExternal => return vec![],
    };
    reduce(app, action)
}
