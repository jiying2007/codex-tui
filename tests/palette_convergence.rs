use codex_tui::{
    app::{Action, AppState, reduce},
    backend::{CodexBackend, FakeBackend},
    command::Command,
    i18n::UiLanguage,
    ui,
};
use ratatui::{Terminal, backend::TestBackend};

fn app() -> AppState {
    AppState::new(FakeBackend::seeded().snapshot().threads)
}

#[test]
fn empty_palette_prioritizes_mission_control_without_removing_commands() {
    let mut app = app();
    reduce(&mut app, Action::OpenCommandPalette);
    let visible = app.command_palette_choices();

    for primary in [
        Command::Search,
        Command::Board,
        Command::Review,
        Command::Workspace,
    ] {
        assert!(
            visible.contains(&primary),
            "missing primary action: {primary:?}"
        );
    }
    for duplicate in [
        Command::TranscriptSearch,
        Command::QuickPrompt,
        Command::TerminalDrawer,
        Command::New,
    ] {
        assert!(
            !visible.contains(&duplicate),
            "compatibility action is in default list: {duplicate:?}"
        );
    }
}

#[test]
fn typing_still_finds_upstream_overlapping_commands_in_both_languages() {
    let mut app = app();
    for (query, expected, language) in [
        ("transcript", Command::TranscriptSearch, UiLanguage::English),
        ("terminal", Command::TerminalDrawer, UiLanguage::English),
        ("quick prompt", Command::QuickPrompt, UiLanguage::English),
        (
            "终端抽屉",
            Command::TerminalDrawer,
            UiLanguage::SimplifiedChinese,
        ),
        (
            "快速消息",
            Command::QuickPrompt,
            UiLanguage::SimplifiedChinese,
        ),
    ] {
        app.language = language;
        reduce(&mut app, Action::CloseCommandPalette);
        reduce(&mut app, Action::OpenCommandPalette);
        reduce(&mut app, Action::CommandPaletteInputText(query.to_owned()));
        assert_eq!(
            app.command_palette_choice(),
            Some(expected),
            "query: {query}"
        );
    }
}

#[test]
fn empty_query_keeps_primary_order_and_full_command_context_on_reopen() {
    let mut app = app();
    reduce(&mut app, Action::OpenCommandPalette);
    assert_eq!(app.command_palette_choice(), Some(Command::Search));
    reduce(
        &mut app,
        Action::CommandPaletteInputText("terminal".to_owned()),
    );
    assert_eq!(app.command_palette_choice(), Some(Command::TerminalDrawer));
    reduce(&mut app, Action::CommandPaletteBackspace);
    assert!(app.command_palette_open);
    reduce(&mut app, Action::CloseCommandPalette);
    reduce(&mut app, Action::OpenCommandPalette);
    assert_eq!(app.command_palette_choice(), Some(Command::Search));
}

#[test]
fn palette_heading_is_visible_at_the_real_pty_test_size() {
    let mut app = app();
    reduce(&mut app, Action::OpenCommandPalette);
    assert!(app.command_palette_open);

    let mut terminal = Terminal::new(TestBackend::new(160, 40)).expect("terminal");
    terminal.draw(|frame| ui::render(frame, &app)).expect("render");
    let buffer = terminal.backend().buffer();
    let mut screen = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            screen.push_str(buffer[(x, y)].symbol());
        }
        screen.push('\n');
    }
    assert!(screen.contains("Command Palette"), "palette title must be rendered");
}
