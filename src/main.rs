use anyhow::Result;
use codex_tui::{
    app::{Action, AppState, Effect, InputMode, ViewKind, reduce},
    app_server::{self, RegistryHandle},
    backend::{BackendSnapshot, BackendStatus, CodexBackend, FakeBackend},
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
    let (initial, mut registry) = if let Some(fake) = &fake_backend {
        (fake.snapshot(), None)
    } else {
        match app_server::start(None).await {
            Ok(started) => (started.initial, Some(started.handle)),
            Err(error) => (
                BackendSnapshot {
                    generation: 0,
                    threads: vec![],
                    status: BackendStatus {
                        source: "codex-app-server".into(),
                        connected: false,
                        version: None,
                        platform: None,
                        codex_home: None,
                        capabilities: vec![],
                        optional_capabilities_missing: vec![],
                        last_refresh_unix_ms: None,
                        error: Some(error.to_string()),
                    },
                },
                None,
            ),
        }
    };

    let mut app = AppState::new(initial.threads);
    app.backend_status = initial.status;
    app.apply_local_state(&local);

    let mut terminal = TerminalSession::enter(config.ui.mouse)?;
    let mut last_fake_tick = Instant::now();

    while !app.should_quit {
        drain_registry(&mut app, registry.as_mut());

        if let Some(fake) = fake_backend.as_mut()
            && last_fake_tick.elapsed() >= Duration::from_millis(900)
        {
            let snapshot = fake.tick();
            reduce(&mut app, Action::ReplaceThreads(snapshot.threads));
            reduce(&mut app, Action::BackendStatus(snapshot.status));
            last_fake_tick = Instant::now();
        }

        terminal
            .terminal_mut()
            .draw(|frame| ui::render(frame, &app))?;

        while event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                for effect in handle_key(&mut app, key) {
                    if effect == Effect::PersistOperatorState {
                        store.save_state(&app.to_local_state())?;
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(16)).await;
    }

    store.save_state(&app.to_local_state())?;
    Ok(())
}

fn drain_registry(app: &mut AppState, registry: Option<&mut RegistryHandle>) {
    let Some(registry) = registry else {
        return;
    };
    while let Some(snapshot) = registry.try_recv() {
        reduce(app, Action::ReplaceThreads(snapshot.threads));
        reduce(app, Action::BackendStatus(snapshot.status));
    }
}

fn handle_key(app: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return vec![];
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
                Action::Back
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
