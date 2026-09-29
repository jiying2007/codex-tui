use anyhow::Result;
use codex_tui::{
    app::{Action, AppState, Effect, InputMode, ViewKind, reduce},
    app_server::{self, ConversationEvent, RegistryHandle},
    backend::{BackendStatus, CodexBackend, FakeBackend},
    conversation::{InteractiveRequestKind, InteractiveResolution},
    keymap::{Command, command_for_key},
    store::{FileStore, LocalStore},
    terminal::TerminalSession,
    ui,
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::time::{Duration, Instant};

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
    let store = FileStore::discover()?;
    let config = store.load_config()?;
    let state = store.load_state()?;

    println!("codex-tui {}", env!("CARGO_PKG_VERSION"));
    println!("config: {}", store.config_path().display());
    println!("state: {}", store.state_path().display());
    println!("mouse: {}", config.ui.mouse);
    println!("schemaVersion: {}", state.schema_version);

    if scope == Some("codex") {
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
        println!("hint: run `codex-tui doctor codex` to probe the live App Server");
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
    let store = FileStore::discover()?;
    let config = store.load_config()?;
    let local = store.load_state()?;

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

    let mut terminal = TerminalSession::enter(config.ui.mouse)?;
    let mut last_fake_tick = Instant::now();
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

        needs_render |= drain_registry(&mut app, registry.as_mut(), &store);

        if let Some(fake) = fake_backend.as_mut()
            && last_fake_tick.elapsed() >= Duration::from_millis(900)
        {
            let snapshot = fake.tick();
            reduce(&mut app, Action::ReplaceThreads(snapshot.threads));
            reduce(&mut app, Action::BackendStatus(snapshot.status));
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
                    apply_effects(&mut app, registry.as_ref(), &store, effects)?;
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
    store.save_state(&app.to_local_state())?;
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
    store: &FileStore,
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
            ConversationEvent::PromptSubmitted { thread_id, .. } => {
                let effects = reduce(app, Action::PromptSubmitted { thread_id });
                for effect in effects {
                    if effect == Effect::PersistOperatorState {
                        if let Err(error) = store.save_state(&app.to_local_state()) {
                            reduce(
                                app,
                                Action::BackendStatus(backend_error_status(format!(
                                    "persist draft state failed: {error}"
                                ))),
                            );
                        }
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

fn apply_effects(
    app: &mut AppState,
    registry: Option<&RegistryHandle>,
    store: &FileStore,
    effects: Vec<Effect>,
) -> Result<()> {
    for effect in effects {
        match effect {
            Effect::PersistOperatorState => store.save_state(&app.to_local_state())?,
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

fn handle_key(app: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return vec![];
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

    if let Some(request) = app.current_pending_request() {
        let action = match (&request.kind, key.code) {
            (InteractiveRequestKind::UserInput { .. }, KeyCode::Enter | KeyCode::Char('a')) => {
                Some(Action::BeginUserInput)
            }
            (InteractiveRequestKind::UserInput { .. }, KeyCode::Char('n')) => {
                Some(Action::ResolvePending(InteractiveResolution::Decline))
            }
            (InteractiveRequestKind::UserInput { .. }, KeyCode::Char('c')) => {
                Some(Action::ResolvePending(InteractiveResolution::Cancel))
            }
            (_, KeyCode::Char('y')) => {
                Some(Action::ResolvePending(InteractiveResolution::Accept))
            }
            (_, KeyCode::Char('n')) => {
                Some(Action::ResolvePending(InteractiveResolution::Decline))
            }
            (_, KeyCode::Char('c')) => {
                Some(Action::ResolvePending(InteractiveResolution::Cancel))
            }
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
        Command::QuitOrInterrupt => {
            if app.view_kind() == ViewKind::Registry {
                Action::Quit
            } else {
                Action::InterruptCurrent
            }
        }
        Command::Back => Action::Back,
        Command::Help => Action::ToggleHelp,
        Command::Search => Action::BeginSearch,
        Command::Next => Action::MoveSelection(1),
        Command::Previous => Action::MoveSelection(-1),
        Command::Open => Action::OpenSelected,
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
        Command::PageUp => Action::ScrollBy(-5),
        Command::PageDown => Action::ScrollBy(5),
        Command::CommandPalette
        | Command::ContextActions
        | Command::Board
        | Command::Review
        | Command::Workspace
        | Command::Snooze
        | Command::New
        | Command::Goal
        | Command::ToggleWordDiff
        | Command::ExternalEditor
        | Command::OpenExternal
        | Command::HotSlot(_)
        | Command::BeginHotSlotBind => return vec![],
    };
    reduce(app, action)
}
