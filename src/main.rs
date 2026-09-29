use anyhow::Result;
use codex_tui::{
    app::{Action, AppState, Effect, ViewKind, reduce},
    backend::{CodexBackend, FakeBackend},
    keymap::{Command, command_for_key},
    store::{FileStore, LocalStore},
    terminal::TerminalSession,
    ui,
};
use crossterm::event::{self, Event};
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "doctor") {
        return doctor();
    }

    run_demo().await
}

fn doctor() -> Result<()> {
    let store = FileStore::discover()?;
    let config = store.load_config()?;
    let state = store.load_state()?;
    let backend = FakeBackend::seeded();
    let fingerprint = backend.fingerprint();

    println!("codex-tui {}", env!("CARGO_PKG_VERSION"));
    println!("backend: {} {}", fingerprint.name, fingerprint.version);
    println!("capabilities: {}", fingerprint.capabilities.join(", "));
    println!("config: {}", store.config_path().display());
    println!("state: {}", store.state_path().display());
    println!("mouse: {}", config.ui.mouse);
    println!("schemaVersion: {}", state.schema_version);
    println!("mode: M0 fake backend");
    Ok(())
}

async fn run_demo() -> Result<()> {
    let store = FileStore::discover()?;
    let config = store.load_config()?;
    let local = store.load_state()?;

    let mut backend = FakeBackend::seeded();
    let mut app = AppState::new(backend.snapshot().threads);
    app.apply_local_state(&local);

    let mut terminal = TerminalSession::enter(config.ui.mouse)?;
    let mut last_backend_tick = Instant::now();

    while !app.should_quit {
        terminal
            .terminal_mut()
            .draw(|frame| ui::render(frame, &app))?;

        while event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()?
                && let Some(command) = command_for_key(key, app.view_kind())
            {
                for effect in handle_command(&mut app, command) {
                    if effect == Effect::PersistOperatorState {
                        store.save_state(&app.to_local_state())?;
                    }
                }
            }
        }

        if last_backend_tick.elapsed() >= Duration::from_millis(900) {
            let snapshot = backend.tick();
            reduce(&mut app, Action::ReplaceThreads(snapshot.threads));
            last_backend_tick = Instant::now();
        }

        tokio::time::sleep(Duration::from_millis(16)).await;
    }

    store.save_state(&app.to_local_state())?;
    Ok(())
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
        Command::Next => Action::MoveSelection(1),
        Command::Previous => Action::MoveSelection(-1),
        Command::Open => Action::OpenSelected,
        Command::NextAttention => Action::NextAttention,
        Command::QuickPrompt => Action::QuickPrompt,
        Command::MarkUnread => Action::MarkUnread,
        Command::PageUp => Action::ScrollBy(-5),
        Command::PageDown => Action::ScrollBy(5),
        Command::CommandPalette
        | Command::Search
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
