use anyhow::Result;
use codex_tui::{
    app::{Action, AppState, Effect, reduce},
    app_server::{self, ConversationEvent, RegistryHandle},
    app_server_target::ResolvedAppServerTarget,
    backend::{BackendStatus, CodexBackend, FakeBackend},
    forge::{self, ForgeEvent, ForgeHandle},
    git::{GitEvent, GitHandle},
    i18n::{UiLanguage, pick},
    notification::NotificationMode,
    planning::PlanningSnapshot,
    runtime_lifecycle,
    terminal::TerminalSession,
    terminal_drawer::TerminalDrawerRuntime,
    ui,
};
use crossterm::event::{self, Event, KeyEventKind};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod runtime_cli;
mod runtime_commands;
mod runtime_connection;
mod runtime_doctor;
mod runtime_external;
mod runtime_input;
mod runtime_local_edit;
mod runtime_notifications;
mod runtime_palette;
mod runtime_prompt;
mod runtime_shutdown;
mod runtime_startup;
mod runtime_store;
mod runtime_store_worker;
mod runtime_target_lookup;
mod runtime_user_response;

use runtime_doctor::doctor;
use runtime_external::{open_external_editor, open_external_url};
use runtime_input::{handle_key, handle_paste};
use runtime_notifications::RuntimeNotifications;
use runtime_store_worker::{StoreEvent, StoreWorker as RuntimeStore};

#[cfg(test)]
use codex_tui::{
    app::{InputMode, ViewKind},
    command::Command,
};
#[cfg(test)]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
#[cfg(test)]
use runtime_commands::{action_for_command, handle_command};
struct RuntimeServices {
    git: GitHandle,
    forge: ForgeHandle,
    notifications: RuntimeNotifications,
    terminal_drawer: TerminalDrawerRuntime,
    store: RuntimeStore,
}

impl RuntimeServices {
    fn new(store: RuntimeStore, notification_mode: NotificationMode) -> Self {
        Self {
            git: GitHandle::start(),
            forge: ForgeHandle::start(),
            notifications: RuntimeNotifications::start(notification_mode),
            terminal_drawer: TerminalDrawerRuntime::default(),
            store,
        }
    }
}

fn cli_terminal_field(value: &str) -> String {
    codex_tui::text::sanitize_inline(value)
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    match runtime_cli::preflight(&args) {
        Ok(Some(help)) => {
            println!("{help}");
            return Ok(());
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!(
                "{}\nRun `codex-tui --help` for usage.",
                cli_terminal_field(&error.to_string())
            );
            std::process::exit(codex_tui::headless::EXIT_USAGE);
        }
    }

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

    if args.first().is_some_and(|arg| arg == "workref") {
        let code = codex_tui::workref::cli::run_cli(&args[1..])?;
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "soak") {
        let code = codex_tui::soak::run_cli(&args[1..])?;
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }

    if let Some(headless_args) = codex_tui::headless::top_level_alias(&args) {
        let code = codex_tui::headless::run(&headless_args).await?;
        if code != codex_tui::headless::EXIT_OK {
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
        && args.get(1).is_some_and(|arg| arg == "bundle")
    {
        let mut output = None;
        let mut offline = false;
        let mut index = 2usize;
        while index < args.len() {
            match args[index].as_str() {
                "--offline" => offline = true,
                "--output" => {
                    index += 1;
                    let Some(path) = args.get(index) else {
                        eprintln!("usage: codex-tui doctor bundle [--offline] [--output PATH]");
                        std::process::exit(codex_tui::headless::EXIT_USAGE);
                    };
                    output = Some(std::path::PathBuf::from(path));
                }
                _ => {
                    eprintln!("usage: codex-tui doctor bundle [--offline] [--output PATH]");
                    std::process::exit(codex_tui::headless::EXIT_USAGE);
                }
            }
            index += 1;
        }
        let path = if offline {
            codex_tui::support_bundle::create_offline(output)?
        } else {
            codex_tui::support_bundle::create(output).await?
        };
        println!(
            "support-bundle: {}",
            cli_terminal_field(&path.display().to_string())
        );
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

    let target_override = take_value_flag(&mut args, "--target")?;

    if args.first().is_some_and(|arg| arg == "doctor") {
        let code = match doctor(args.get(1).map(String::as_str), target_override.as_deref()).await {
            Ok(code) => code,
            Err(error) => {
                eprintln!(
                    "doctor: DEGRADED · {}",
                    cli_terminal_field(&format!("{error:#}"))
                );
                codex_tui::headless::EXIT_DEGRADED
            }
        };
        if code != codex_tui::headless::EXIT_OK {
            std::process::exit(code);
        }
        return Ok(());
    }

    let fake_mode = args.iter().any(|arg| arg == "--fake");
    // Keep the interactive state machine out of the entry future's stack frame,
    // including --help and invalid usage on native Windows debug builds.
    Box::pin(run_app(fake_mode, target_override.as_deref())).await
}

fn take_value_flag(args: &mut Vec<String>, flag: &str) -> Result<Option<String>> {
    let Some(index) = args.iter().position(|arg| arg == flag) else {
        return Ok(None);
    };
    anyhow::ensure!(
        args.iter().skip(index + 1).all(|arg| arg != flag),
        "{flag} may be provided only once"
    );
    anyhow::ensure!(index + 1 < args.len(), "{flag} requires a target name");
    let value = args.remove(index + 1);
    args.remove(index);
    anyhow::ensure!(
        !value.trim().is_empty(),
        "{flag} target name must not be empty"
    );
    Ok(Some(value))
}

async fn run_app(fake_mode: bool, target_override: Option<&str>) -> Result<()> {
    let (store, bootstrap) = RuntimeStore::discover()?;
    let config = bootstrap.config;
    let local = bootstrap.local;
    let app_server_target = if fake_mode {
        None
    } else {
        Some(ResolvedAppServerTarget::resolve(
            &config.app_server,
            target_override,
        )?)
    };
    let notification_mode = config.notifications.mode;
    let presentation_mode = config.ui.presentation;
    let mut fake_backend = fake_mode.then(FakeBackend::seeded);
    let mut registry: Option<RegistryHandle> = None;
    let mut startup_connected = false;
    let mut connect_task = None;
    let mut app = if let Some(fake) = &fake_backend {
        let snapshot = fake.snapshot();
        let mut app = AppState::new(snapshot.threads);
        app.backend_status = snapshot.status;
        app
    } else {
        let target = app_server_target
            .as_ref()
            .expect("non-fake mode target")
            .clone();
        let status_source = target.status_source();
        connect_task = Some(tokio::spawn(app_server::start_target(target)));
        let mut app = AppState::new(vec![]);
        app.backend_status = BackendStatus::starting(status_source);
        app
    };
    app.language = config.ui.language.resolve();
    if let Some(target) = app_server_target.as_ref() {
        app.configure_operator_target(target);
    }
    app.apply_local_state(&local);
    app.install_workrefs(bootstrap.workrefs);
    reduce(&mut app, Action::PlanningSnapshotLoaded(bootstrap.planning));
    reduce(&mut app, Action::PlanningStoreDegraded(store.error()));
    let mut planning_worker = codex_tui::app::planning_worker::PlanningWorker::start()?;
    let mut services = RuntimeServices::new(store, notification_mode);
    let mut target_lookup =
        runtime_target_lookup::LookupWorker::new(app_server_target.as_ref().map(|target| {
            let mut lookup_config = config.app_server.clone();
            lookup_config.active.clone_from(&target.name);
            lookup_config
        }));
    let language = app.language;
    let initial_git_effects = reduce(&mut app, Action::RefreshGitProjections);
    apply_effects(
        &mut app,
        registry.as_ref(),
        &mut services,
        initial_git_effects,
    )?;
    app.advance_session_attention_clock(now_unix_ms());
    services.notifications.seed(&app);

    let mut exit_signal = runtime_shutdown::ExitSignal::install()?;
    let mut terminal = TerminalSession::enter()?;
    let mut last_fake_tick = Instant::now();
    let mut last_git_reconcile = Instant::now();
    let mut last_forge_reconcile = Instant::now();
    let mut needs_render = true;
    let mut urgent_render = true;
    let mut last_render = Instant::now();

    let run_result: Result<()> = async {
        while !app.should_quit {
            for _ in 0..64 {
                if !event::poll(Duration::ZERO)? {
                    break;
                }
                match event::read()? {
                    Event::Key(key) => {
                        if runtime_startup::try_retry(
                            &mut app,
                            key,
                            app_server_target.as_ref(),
                            startup_connected || registry.is_some(),
                            &mut connect_task,
                        ) {
                            needs_render = true;
                            urgent_render = true;
                            continue;
                        }
                        let effects = handle_key(&mut app, key);
                        if !effects.is_empty()
                            || matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
                        {
                            needs_render = true;
                            urgent_render = true;
                        }
                        apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
                    }
                    Event::Paste(text) => {
                        let effects = handle_paste(&mut app, text);
                        needs_render = true;
                        urgent_render = true;
                        apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
                    }
                    Event::Resize(cols, rows) => {
                        needs_render = true;
                        urgent_render = true;
                        if app.terminal_drawer_open {
                            let effects = vec![Effect::TerminalResize(
                                ui::terminal_drawer_pty_size(cols, rows),
                            )];
                            apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
                        }
                    }
                    _ => {}
                }
            }
            if app.should_quit {
                break;
            }
            needs_render |= target_lookup.advance(&mut app).await;
            let mut planning_dirty = false;
            for _ in 0..32 {
                let Some(event) = services.store.try_event() else {
                    break;
                };
                planning_dirty |= runtime_local_edit::apply_event(&mut app, event);
                needs_render = true;
            }

            if let Some(result) = runtime_startup::take_finished(&mut connect_task).await {
                match result {
                    Ok(started) => {
                        startup_connected = true;
                        runtime_connection::install(&mut app, started.initial);
                        registry = Some(started.handle);
                        let effects = reduce(&mut app, Action::RefreshGitProjections);
                        apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
                        planning_dirty = true;
                    }
                    Err(error) => {
                        let hint = runtime_startup::retry_hint(app.language);
                        let status = backend_error_status(format!("{hint} · {error}"));
                        reduce(&mut app, Action::BackendStatus(status));
                    }
                }
                needs_render = true;
            }

            let mut registry_effects = Vec::new();
            let registry_changes = drain_registry(
                &mut app,
                registry.as_mut(),
                &mut services.store,
                &mut registry_effects,
            );
            apply_effects(&mut app, registry.as_ref(), &mut services, registry_effects)?;
            needs_render |= registry_changes.any;
            if registry_changes.registry_projection {
                let effects = reduce(&mut app, Action::RefreshGitProjections);
                apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
            }
            planning_dirty |= registry_changes.planning;
            let git_changes = drain_git(&mut app, &mut services.git);
            needs_render |= git_changes.any;
            if git_changes.planning_projection {
                let effects = reduce(&mut app, Action::RefreshForgeProjections);
                apply_effects(&mut app, registry.as_ref(), &mut services, effects)?;
                planning_dirty = true;
            }

            let forge_changed = drain_forge(&mut app, &mut services.forge);
            needs_render |= forge_changed;
            planning_dirty |= forge_changed;

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
                    planning_dirty = true;
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
                planning_dirty = true;
                last_fake_tick = Instant::now();
                needs_render = true;
            }

            let attention_now = now_unix_ms();
            let attention_clock_changed = app.advance_session_attention_clock(attention_now);
            let planning_applied =
                match planning_worker.advance(&mut app, planning_dirty, attention_now) {
                    Ok(applied) => applied,
                    Err(error) => {
                        reduce(&mut app, Action::MutationNotice(error));
                        false
                    }
                };
            if planning_applied
                || planning_dirty
                || registry_changes.notification
                || attention_clock_changed
            {
                needs_render = true;
                if let Err(error) = services.notifications.observe(&app) {
                    reduce(
                        &mut app,
                        Action::MutationNotice(format!(
                            "{}: {error}",
                            runtime_text(
                                language,
                                "notification dispatch unavailable",
                                "通知分派不可用",
                            )
                        )),
                    );
                }
            }

            if let Some(recent) = services.notifications.take_recent() {
                reduce(&mut app, Action::RecentNotificationObserved(recent));
                needs_render = true;
            }
            if app
                .recent_notification
                .as_ref()
                .is_some_and(|recent| recent.visible_until <= Instant::now())
            {
                reduce(&mut app, Action::ClearRecentNotification);
                needs_render = true;
            }
            if let Some(notice) = services.notifications.try_notice() {
                reduce(
                    &mut app,
                    Action::MutationNotice(format!(
                        "{}: {notice}",
                        runtime_text(language, "notification delivery degraded", "通知投递已降级",)
                    )),
                );
                needs_render = true;
            }

            if needs_render && presentation_mode.should_render(last_render.elapsed(), urgent_render)
            {
                terminal
                    .terminal_mut()
                    .draw(|frame| ui::render(frame, &app))?;
                needs_render = false;
                urgent_render = false;
                last_render = Instant::now();
            }

            tokio::select! {
                biased;
                _ = exit_signal.recv() => break,
                _ = tokio::time::sleep(Duration::from_millis(16)) => {},
            }
        }

        Ok(())
    }
    .await;

    if let Some(task) = connect_task {
        task.abort();
    }
    runtime_shutdown::finish(terminal, run_result, || {
        services
            .store
            .flush_operator_state_on_exit(&app.to_local_state())
    })
}

#[cfg(test)]
fn reconcile_planning_if_dirty(app: &mut AppState, planning_dirty: bool, now_unix_ms: u64) -> bool {
    if !planning_dirty {
        return false;
    }
    reduce(app, Action::ReconcilePlanning { now_unix_ms });
    true
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
    notification: bool,
}

fn conversation_event_changes_planning(event: &ConversationEvent) -> bool {
    match event {
        ConversationEvent::GoalObserved(_)
        | ConversationEvent::GoalCleared(_)
        | ConversationEvent::ThreadCreated { .. } => true,
        ConversationEvent::ThreadLifecycleFailed { .. }
        | ConversationEvent::Loaded(_)
        | ConversationEvent::OlderLoaded { .. }
        | ConversationEvent::OlderFailed { .. }
        | ConversationEvent::TranscriptSearchLoaded(_)
        | ConversationEvent::TranscriptSearchJumpLoaded { .. }
        | ConversationEvent::TranscriptSearchFailed { .. }
        | ConversationEvent::InteractiveRequested(_)
        | ConversationEvent::InteractiveResolved { .. }
        | ConversationEvent::UserResponse { .. }
        | ConversationEvent::PromptSubmitted { .. }
        | ConversationEvent::PromptFailed { .. }
        | ConversationEvent::Failed { .. } => false,
    }
}

fn conversation_event_changes_notification(event: &ConversationEvent) -> bool {
    matches!(
        event,
        ConversationEvent::InteractiveRequested(_)
            | ConversationEvent::InteractiveResolved { .. }
            | ConversationEvent::UserResponse { .. }
            | ConversationEvent::GoalObserved(_)
            | ConversationEvent::GoalCleared(_)
    )
}

fn drain_registry(
    app: &mut AppState,
    registry: Option<&mut RegistryHandle>,
    store: &mut RuntimeStore,
    effects: &mut Vec<Effect>,
) -> RegistryDrainChanges {
    let Some(registry) = registry else {
        return RegistryDrainChanges::default();
    };
    let mut changes = RegistryDrainChanges::default();
    // The Registry is a latest-value watch, not an event FIFO. Applying only
    // one full snapshot per UI turn leaves time for input and ordered events.
    if let Some(snapshot) = registry.try_recv() {
        effects.extend(reduce(app, Action::ReplaceThreads(snapshot.threads)));
        effects.extend(reduce(app, Action::BackendStatus(snapshot.status)));
        changes.any = true;
        changes.registry_projection = true;
        changes.planning = true;
        changes.notification = true;
    }
    for _ in 0..32 {
        let Some(event) = registry.try_recv_conversation() else {
            if registry.response_actor_finished() {
                changes.any |= app.settle_stopped_actor(effects);
                changes.notification |= changes.any;
            }
            break;
        };
        changes.planning |= conversation_event_changes_planning(&event);
        changes.notification |= conversation_event_changes_notification(&event);
        changes.any = true;
        let action = match event {
            ConversationEvent::ThreadCreated {
                thread_id,
                operation,
            } => Action::ThreadCreated {
                thread_id,
                operation,
            },
            ConversationEvent::ThreadLifecycleFailed { operation, error } => {
                Action::ThreadLifecycleFailed { operation, error }
            }
            ConversationEvent::Loaded(page) => {
                if let Some(error) = store.index_conversation_page(&page) {
                    effects.extend(reduce(app, Action::MutationNotice(error)));
                }
                Action::ConversationLoaded(page)
            }
            ConversationEvent::OlderLoaded {
                page,
                turn_cursor,
                item_cursor,
            } => {
                if let Some(error) = store.index_conversation_page(&page) {
                    effects.extend(reduce(app, Action::MutationNotice(error)));
                }
                Action::OlderConversationLoaded {
                    page,
                    turn_cursor,
                    item_cursor,
                }
            }
            ConversationEvent::OlderFailed {
                thread_id,
                turn_cursor,
                item_cursor,
                error,
            } => Action::OlderConversationFailed {
                thread_id,
                turn_cursor,
                item_cursor,
                error,
            },
            ConversationEvent::TranscriptSearchLoaded(results) => {
                Action::TranscriptSearchLoaded(results)
            }
            ConversationEvent::TranscriptSearchJumpLoaded { page, item_id } => {
                if let Some(error) = store.index_conversation_page(&page) {
                    effects.extend(reduce(app, Action::MutationNotice(error)));
                }
                Action::TranscriptSearchLoadedPage { page, item_id }
            }
            ConversationEvent::TranscriptSearchFailed { query, error } => {
                Action::TranscriptSearchServerFailed { query, error }
            }
            ConversationEvent::InteractiveRequested(request) => {
                Action::InteractiveRequested(request)
            }
            ConversationEvent::UserResponse { ticket, outcome } => {
                app.finish_user_response(ticket, outcome);
                continue;
            }
            ConversationEvent::InteractiveResolved { request_id } => {
                Action::InteractiveResolved { request_id }
            }
            ConversationEvent::GoalObserved(goal) => Action::GoalObserved(goal),
            ConversationEvent::GoalCleared(thread_id) => Action::GoalCleared(thread_id),
            ConversationEvent::PromptSubmitted {
                thread_id,
                request_id,
                ..
            } => Action::PromptSubmitted {
                thread_id,
                request_id,
            },
            ConversationEvent::PromptFailed {
                thread_id,
                request_id,
                error,
            } => Action::PromptFailed {
                thread_id,
                request_id,
                error,
            },
            ConversationEvent::Failed { thread_id, error } => {
                Action::ConversationFailed { thread_id, error }
            }
        };
        // Event-driven reducers produce real effects too. Preserve their order
        // and route ALL of them through the same dispatcher as keyboard input.
        effects.extend(reduce(app, action));
    }
    changes
}

#[cfg(test)]
mod registry_drain_classification_tests {
    use super::*;

    #[test]
    fn native_request_events_observe_without_planning() {
        use codex_tui::conversation::{InteractiveRequest, InteractiveRequestKind, RpcRequestId};
        let request = InteractiveRequest {
            request_id: RpcRequestId::Integer(71),
            thread_id: codex_tui::domain::ThreadId::new("thread"),
            turn_id: "turn".into(),
            item_id: "item".into(),
            kind: InteractiveRequestKind::UserInput { questions: vec![] },
        };
        for event in [
            ConversationEvent::InteractiveRequested(request),
            ConversationEvent::InteractiveResolved {
                request_id: RpcRequestId::Integer(71),
            },
            ConversationEvent::UserResponse {
                ticket: 1,
                outcome: codex_tui::user_response::UserResponseOutcome::Written,
            },
        ] {
            assert!(!conversation_event_changes_planning(&event));
            assert!(conversation_event_changes_notification(&event));
        }
        assert!(!conversation_event_changes_notification(
            &ConversationEvent::Failed {
                thread_id: codex_tui::domain::ThreadId::new("thread"),
                error: "read failed".into(),
            }
        ));
    }

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
                request_id: 1,
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
    for _ in 0..32 {
        let Some(event) = git.try_recv() else {
            break;
        };
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
            codex_tui::git::GitReview::pending(codex_tui::domain::ThreadId::new("thread"), "/repo",)
        )));
    }
}

fn drain_forge(app: &mut AppState, forge: &mut ForgeHandle) -> bool {
    let mut changed = false;
    for _ in 0..32 {
        let Some(event) = forge.try_recv() else {
            break;
        };
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
        notifications: _,
        terminal_drawer,
        store,
    } = services;
    for effect in effects {
        let local_ticket = app.local_edit_ticket_for(&effect);
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
                runtime_local_edit::submit(app, store, local_ticket, move |store| {
                    store.create_scratch(title, workspace)
                });
            }
            Effect::SnoozeWorkCard {
                anchor,
                duration_ms,
            } => {
                submit_planning(
                    app,
                    store.planning(
                        move |store| store.snooze_work_card(anchor, duration_ms),
                        None,
                    ),
                );
            }
            Effect::SaveSourceNote { owner, text } => {
                runtime_local_edit::submit(app, store, local_ticket, move |store| {
                    store.save_source_note(owner, text)
                });
            }
            Effect::UpdateScratchNote { scratch_id, note } => {
                runtime_local_edit::submit(app, store, local_ticket, move |store| {
                    store.update_scratch_note(scratch_id, note)
                });
            }
            Effect::CreateBookmark { source, label } => {
                submit_planning(
                    app,
                    store.planning(move |store| store.create_bookmark(source, label), None),
                );
            }
            Effect::UpdateScratchState { scratch_id, state } => {
                submit_planning(
                    app,
                    store.planning(
                        move |store| store.update_scratch_state(scratch_id, state),
                        None,
                    ),
                );
            }
            Effect::DeleteScratch { scratch_id } => {
                submit_planning(
                    app,
                    store.planning(move |store| store.delete_scratch(scratch_id), None),
                );
            }
            Effect::SaveSavedView { view } => {
                runtime_local_edit::submit(app, store, local_ticket, move |store| {
                    store.save_view(view)
                });
            }
            Effect::DeleteSavedView { view_id } => {
                submit_planning(
                    app,
                    store.planning(move |store| store.delete_view(view_id), None),
                );
            }
            Effect::SetHotSlot { slot, target } => {
                submit_planning(
                    app,
                    store.planning(move |store| store.set_hot_slot(slot, target), None),
                );
            }
            Effect::ApplyLocalBatch(plan) => {
                let notice = format!(
                    "{} · {}",
                    runtime_text(app.language, "local batch applied", "本地批量操作已应用"),
                    plan.preview()
                );
                submit_planning(
                    app,
                    store.planning(move |store| store.apply_local_batch(&plan), Some(notice)),
                );
            }
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
                        Action::ForgeReviewLoaded(Box::new(forge::ForgeReviewResult::unavailable(
                            fallback,
                            error.to_string(),
                        ))),
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
                worktree_root,
                path,
            } => {
                if let Err(error) = open_external_editor(&cwd, &worktree_root, &path) {
                    reduce(
                        app,
                        Action::ReviewError {
                            thread_id,
                            error: error.to_string(),
                        },
                    );
                }
            }
            Effect::OpenExternalUrl { url } => {
                if let Err(error) = open_external_url(&url) {
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "{}: {error:#}",
                            runtime_text(
                                app.language,
                                "open external target failed",
                                "打开外部目标失败",
                            )
                        )),
                    );
                }
            }
            Effect::LinkWorkrefSession { key, session } => {
                if let Err(error) = store.submit(move |store| {
                    let result = store.link_workref_session(&key, session);
                    StoreEvent::WorkrefLinked { key, result }
                }) {
                    reduce(app, Action::WorkrefLinkSubmissionRejected(error));
                }
            }
            Effect::SearchTranscript { query } => {
                let local_query = query.clone();
                if let Err(error) = store.submit(move |store| {
                    StoreEvent::Search(store.search_transcript(
                        &local_query,
                        codex_tui::transcript_search::TRANSCRIPT_SEARCH_RESULT_LIMIT,
                    ))
                }) {
                    reduce(app, Action::MutationNotice(error));
                }
                if let Some(registry) = registry {
                    if let Err(error) = registry.search_transcript(query.clone()) {
                        reduce(
                            app,
                            Action::TranscriptSearchServerFailed {
                                query,
                                error: error.to_string(),
                            },
                        );
                    }
                } else {
                    reduce(
                        app,
                        Action::TranscriptSearchServerFailed {
                            query,
                            error: runtime_text(
                                app.language,
                                "App Server full-history search unavailable; showing local indexed history",
                                "App Server 全历史搜索不可用；正在显示本地已索引历史",
                            )
                            .into(),
                        },
                    );
                }
            }
            Effect::JumpToTranscriptHit(hit) => {
                let thread_id = hit.thread_id.clone();
                if let Some(registry) = registry {
                    if let Err(error) =
                        registry.jump_to_transcript_hit_checked(&app.backend_status, hit)
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
            Effect::StartThread { cwd } => {
                runtime_lifecycle::start_thread(app, registry, cwd);
            }
            Effect::ForkThread { thread_id } => {
                runtime_lifecycle::fork_thread(app, registry, thread_id);
            }
            Effect::LoadConversation(thread_id) => {
                if let Some(registry) = registry {
                    if let Err(error) =
                        registry.load_conversation_checked(&app.backend_status, thread_id.clone())
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
                    && let Err(error) = registry.load_older_conversation_checked(
                        &app.backend_status,
                        thread_id.clone(),
                        turn_cursor.clone(),
                        item_cursor.clone(),
                    )
                {
                    reduce(
                        app,
                        Action::OlderConversationFailed {
                            thread_id,
                            turn_cursor,
                            item_cursor,
                            error: error.to_string(),
                        },
                    );
                }
            }
            Effect::SubmitPrompt {
                thread_id,
                request_id,
                text,
                active_turn_id,
            } => {
                runtime_prompt::submit(app, registry, thread_id, request_id, text, active_turn_id);
            }
            Effect::ResolveInteractive {
                request_id,
                resolution,
            } => {
                runtime_user_response::submit(app, registry, &request_id, &resolution);
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
fn submit_planning(app: &mut AppState, result: Result<(), String>) {
    if let Err(error) = result {
        reduce(app, Action::PlanningStoreDegraded(Some(error)));
    }
}
fn apply_planning_store_result(app: &mut AppState, result: Result<PlanningSnapshot, String>) {
    match result {
        Ok(snapshot) => {
            reduce(app, Action::PlanningSnapshotLoaded(snapshot));
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
#[cfg(test)]
mod app_server_target_cli_tests {
    use super::*;
    #[test]
    fn target_flag_is_removed_and_preserves_other_arguments() {
        let mut args = vec![
            "doctor".to_string(),
            "codex".to_string(),
            "--target".to_string(),
            "remote-dev".to_string(),
        ];
        assert_eq!(
            take_value_flag(&mut args, "--target").expect("target"),
            Some("remote-dev".into())
        );
        assert_eq!(args, vec!["doctor", "codex"]);
    }
    #[test]
    fn target_flag_requires_exactly_one_non_empty_value() {
        let mut missing = vec!["--target".to_string()];
        assert!(take_value_flag(&mut missing, "--target").is_err());
        let mut duplicate = vec![
            "--target".to_string(),
            "one".to_string(),
            "--target".to_string(),
            "two".to_string(),
        ];
        assert!(take_value_flag(&mut duplicate, "--target").is_err());
    }
}

#[cfg(test)]
mod command_palette_input_tests {
    use super::*;

    fn app() -> AppState {
        AppState::new(FakeBackend::seeded().snapshot().threads)
    }

    #[test]
    fn help_advertised_commands_have_a_reducer_action() {
        let app = app();
        for binding in codex_tui::keymap::HELP_BINDINGS {
            assert!(
                action_for_command(&app, binding.command).is_some(),
                "advertised command {:?} has no reducer action",
                binding.command
            );
        }
    }

    #[test]
    fn help_and_search_complete_key_to_state_transitions() {
        let mut app = app();

        let effects = handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        );
        assert!(effects.is_empty());
        assert!(app.show_help);

        let effects = handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        );
        assert!(effects.is_empty());
        assert_eq!(app.input_mode, InputMode::Search);
        assert!(!app.show_help, "search must not start behind help");
    }

    #[test]
    fn help_can_open_the_palette_without_leaving_an_invisible_help_overlay() {
        let mut app = app();
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        );
        assert!(app.show_help);
        assert!(
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)
            )
            .is_empty()
        );
        assert!(!app.show_help);
        assert!(app.command_palette_open);
    }

    #[test]
    fn command_palette_traps_navigation_and_executes_selected_command() {
        let mut app = app();
        let registry_selected = app.selected;

        handle_command(&mut app, Command::CommandPalette);
        assert!(app.command_palette_open);

        let effects = handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(effects.is_empty());
        assert_eq!(app.selected, registry_selected);
        assert_eq!(app.command_palette_selected, 1);

        let board_index = app
            .command_palette_choices()
            .iter()
            .position(|choice| *choice == Command::Board)
            .expect("Board palette entry");
        app.command_palette_selected = board_index;

        let effects = handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(effects.is_empty());
        assert!(!app.command_palette_open);
        assert_eq!(app.view_kind(), ViewKind::Board);
    }

    #[test]
    fn ctrl_k_toggles_command_palette_when_application_owns_input() {
        let mut app = app();
        let ctrl_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL);

        assert!(handle_key(&mut app, ctrl_k).is_empty());
        assert!(app.command_palette_open);

        assert!(handle_key(&mut app, ctrl_k).is_empty());
        assert!(!app.command_palette_open);
    }
}

#[cfg(test)]
mod background_planning_coalescing_tests {
    use super::*;

    #[test]
    fn background_planning_reconciles_only_when_marked_dirty() {
        let mut app = AppState::new(FakeBackend::scaled(8).snapshot().threads);
        assert!(app.work_cards.is_empty());

        assert!(!reconcile_planning_if_dirty(&mut app, false, 1));
        assert!(app.work_cards.is_empty());

        assert!(reconcile_planning_if_dirty(&mut app, true, 1));
        assert_eq!(app.work_cards.len(), 8);
    }
}

#[cfg(test)]
mod accessibility_input_tests {
    use super::*;

    fn app() -> AppState {
        AppState::new(FakeBackend::seeded().snapshot().threads)
    }

    #[test]
    fn help_modal_does_not_switch_tasks_or_consume_recent_alerts() {
        use codex_tui::{
            app::View,
            notification::{NotificationEvent, NotificationKind, RecentNotification},
            planning::{HotSlot, SourceRef},
        };

        let mut app = app();
        let destination = app.threads[1].id.clone();
        app.planning_snapshot.hot_slots.push(HotSlot {
            slot: 1,
            target: SourceRef::codex_thread(&destination),
            updated_at_unix_ms: 1,
        });
        app.recent_notification = Some(RecentNotification::new(
            NotificationEvent {
                key: format!("completion:{}", destination.0),
                item_id: destination.0.clone(),
                kind: NotificationKind::Completion,
                subject: "important work finished".into(),
            },
            Some(destination.clone()),
            0,
        ));
        app.show_help = true;
        for (code, modifiers) in [
            (KeyCode::Char('!'), KeyModifiers::SHIFT),
            (KeyCode::Char('1'), KeyModifiers::SHIFT),
            (KeyCode::Char('1'), KeyModifiers::NONE),
            (KeyCode::Char('='), KeyModifiers::NONE),
            (KeyCode::Char('j'), KeyModifiers::NONE),
        ] {
            assert!(handle_key(&mut app, KeyEvent::new(code, modifiers)).is_empty());
            assert_eq!(app.view, View::Registry);
            assert_eq!(app.selected, 0);
            assert!(app.show_help);
            assert!(app.recent_notification.is_some());
        }
        assert!(handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)).is_empty());
        assert!(!app.show_help);

        let effects = handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::SHIFT),
        );
        assert!(effects.contains(&Effect::LoadConversation(destination.clone())));
        assert_eq!(app.view, View::Thread(destination));
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
    fn terminal_focus_release_accepts_alt_t_ctrl_bracket_and_raw_group_separator() {
        for release_key in [
            KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT),
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
        app.context_open = true;

        let effects = handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        );
        assert_eq!(effects, vec![Effect::TerminalInput(vec![b'j'])]);
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

#[cfg(test)]
mod cli_terminal_presentation_tests {
    use super::cli_terminal_field;

    #[test]
    fn external_cli_error_and_bundle_path_text_cannot_spoof_terminal() {
        let original = "path\u{001b}[2J\u{202e}\n中文";
        assert_eq!(cli_terminal_field(original), "path\u{fffd}[2J\u{fffd} 中文");
        assert_eq!(original, "path\u{001b}[2J\u{202e}\n中文");
        assert_eq!(cli_terminal_field("normal 中文"), "normal 中文");
    }
}

#[cfg(test)]
mod compact_help_navigation_tests {
    use super::*;

    #[test]
    fn help_scrolls_to_all_entries_without_navigating_the_background() {
        let mut app = AppState::new(vec![]);
        assert!(!app.show_help);
        assert_eq!(app.help_top, 0);
        let press = |app: &mut AppState, code: KeyCode| {
            let _ = handle_key(app, KeyEvent::new(code, KeyModifiers::NONE));
        };

        press(&mut app, KeyCode::Char('?'));
        assert!(app.show_help);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.help_top, 3);
        press(&mut app, KeyCode::End);
        assert_eq!(app.help_top, codex_tui::app::HELP_ENTRY_COUNT - 1);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.help_top, codex_tui::app::HELP_ENTRY_COUNT - 1);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.help_top, codex_tui::app::HELP_ENTRY_COUNT - 4);
        press(&mut app, KeyCode::Home);
        assert_eq!(app.help_top, 0);
        assert_eq!(app.view_kind(), ViewKind::Registry);
        assert_eq!(app.selected, 0);

        press(&mut app, KeyCode::Esc);
        assert!(!app.show_help);
        press(&mut app, KeyCode::Char('?'));
        assert!(app.show_help);
        assert_eq!(app.help_top, 0);
    }
}
