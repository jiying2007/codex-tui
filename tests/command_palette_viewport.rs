//! Pure reducer + rendered-buffer checks; never execute an Effect or a real backend.
use codex_tui::{
    app::{Action, AppState, reduce},
    backend::{CodexBackend, FakeBackend},
    command::Command,
    i18n::UiLanguage,
    ui,
};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, style::Modifier};
use unicode_width::UnicodeWidthStr;

fn apply(app: &mut AppState, action: Action) {
    assert!(
        reduce(app, action).is_empty(),
        "palette navigation emitted an effect"
    );
}

fn fixture(language: UiLanguage) -> AppState {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.language = language;
    apply(&mut app, Action::OpenCommandPalette);
    app
}

fn row(buffer: &Buffer, y: u16) -> String {
    let mut text = String::new();
    let mut x = 0;
    while x < buffer.area.width {
        let symbol = buffer[(x, y)].symbol();
        text.push_str(symbol);
        x += symbol.width().max(1) as u16;
    }
    text
}

fn screen(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| format!("{}\n", row(buffer, y)))
        .collect()
}

fn draw(terminal: &mut Terminal<TestBackend>, app: &AppState) -> Buffer {
    let before = (
        app.command_palette_choices(),
        app.command_palette_selected,
        app.command_palette_query.clone(),
    );
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    assert_eq!(
        before,
        (
            app.command_palette_choices(),
            app.command_palette_selected,
            app.command_palette_query.clone()
        )
    );
    terminal.backend().buffer().clone()
}

fn assert_selected(buffer: &Buffer, app: &AppState) {
    let command = app.command_palette_choice().expect("selected command");
    let label = command
        .palette_label(app.language.is_simplified_chinese())
        .unwrap();
    let marker = format!("> {label}");
    let mut found = 0;
    for y in 0..buffer.area.height {
        let line = row(buffer, y);
        if let Some(start) = line.find(&marker) {
            let x = line[..start].width() as u16;
            if !buffer[(x, y)].modifier.contains(Modifier::REVERSED) {
                continue; // Query text is not a selected command row.
            }
            found += 1;
            assert!(buffer[(x + 2, y)].modifier.contains(Modifier::REVERSED));
        }
    }
    assert_eq!(
        found,
        1,
        "selected {marker:?} missing or duplicated:\n{}",
        screen(buffer)
    );
}

#[test]
fn selection_stays_visible_through_bilingual_first_last_wrap_and_navigation() {
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        for (width, height) in [(80, 12), (40, 12), (80, 24)] {
            let mut app = fixture(language);
            let choices = app.command_palette_choices();
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            assert_selected(&draw(&mut terminal, &app), &app);
            apply(&mut app, Action::MoveCommandPalette(-1));
            assert_eq!(app.command_palette_choice(), Some(Command::Help));
            assert_selected(&draw(&mut terminal, &app), &app);
            apply(&mut app, Action::MoveCommandPalette(1));
            assert_eq!(app.command_palette_selected, 0);
            for _ in 0..choices.len() * 2 {
                assert_selected(&draw(&mut terminal, &app), &app);
                apply(&mut app, Action::MoveCommandPalette(1));
                assert_eq!(app.command_palette_choices(), choices);
                assert!(app.command_palette_query.is_empty());
            }
        }
    }
}

#[test]
fn same_terminal_resize_keeps_the_selected_command_visible() {
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        let mut app = fixture(language);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        apply(&mut app, Action::MoveCommandPalette(-1));
        for (width, height) in [(80, 24), (80, 12), (40, 12), (80, 24)] {
            terminal.backend_mut().resize(width, height);
            terminal.resize(Rect::new(0, 0, width, height)).unwrap();
            assert_selected(&draw(&mut terminal, &app), &app);
            assert_eq!(app.command_palette_choice(), Some(Command::Help));
        }
    }
}

#[test]
fn filtering_resets_selection_and_restores_query_then_close_reopen_resets_viewport() {
    for (language, query) in [
        (UiLanguage::English, "Help"),
        (UiLanguage::SimplifiedChinese, "帮助"),
    ] {
        let mut app = fixture(language);
        let choices = app.command_palette_choices();
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        apply(&mut app, Action::MoveCommandPalette(-1));
        assert_selected(&draw(&mut terminal, &app), &app);
        apply(&mut app, Action::CommandPaletteInputText(query.into()));
        assert_eq!(app.command_palette_selected, 0);
        assert_eq!(app.command_palette_choice(), Some(Command::Help));
        let buffer = draw(&mut terminal, &app);
        assert_selected(&buffer, &app);
        let prefix = if language.is_simplified_chinese() {
            "搜索"
        } else {
            "Query"
        };
        assert!(screen(&buffer).contains(&format!("{prefix}> {query}")));
        assert!(buffer.content.iter().any(|cell| {
            cell.modifier
                .contains(Modifier::BOLD | Modifier::UNDERLINED | Modifier::REVERSED)
        }));
        for _ in query.chars() {
            apply(&mut app, Action::CommandPaletteBackspace);
        }
        assert_eq!(app.command_palette_choices(), choices);
        assert_selected(&draw(&mut terminal, &app), &app);
        apply(
            &mut app,
            Action::CommandPaletteInputText("not-a-command".into()),
        );
        assert!(app.command_palette_choices().is_empty());
        assert_eq!(app.command_palette_choice(), None);
        let empty = screen(&draw(&mut terminal, &app));
        assert!(empty.contains(if language.is_simplified_chinese() {
            "没有匹配的命令。"
        } else {
            "No matching commands."
        }));
        apply(&mut app, Action::CloseCommandPalette);
        assert!(!app.command_palette_open);
        let closed = screen(&draw(&mut terminal, &app));
        assert!(!closed.contains("Command Palette") && !closed.contains("命令面板"));
        apply(&mut app, Action::OpenCommandPalette);
        assert!(app.command_palette_query.is_empty());
        assert_eq!(app.command_palette_selected, 0);
        assert_eq!(app.command_palette_choices(), choices);
        assert_selected(&draw(&mut terminal, &app), &app);
    }
}

#[test]
fn stale_selection_never_highlights_an_unselected_command_or_panics() {
    let mut app = fixture(UiLanguage::English);
    let choices = app.command_palette_choices();
    for stale in [choices.len(), usize::MAX] {
        app.command_palette_selected = stale;
        for (width, height) in [(80, 24), (40, 12), (1, 1)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let rendered = screen(&draw(&mut terminal, &app));
            for command in &choices {
                assert!(
                    !rendered.contains(&format!("> {}", command.palette_label(false).unwrap()))
                );
            }
            assert_eq!(app.command_palette_selected, stale);
            assert_eq!(app.command_palette_choice(), None);
        }
    }
}

#[test]
fn long_untrusted_query_does_not_hide_the_empty_result_or_mutate_source() {
    let mut app = fixture(UiLanguage::English);
    let query = format!("query\u{202e}spoof{}", "x".repeat(220));
    apply(&mut app, Action::CommandPaletteInputText(query.clone()));
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    let rendered = screen(&draw(&mut terminal, &app));
    assert!(rendered.contains("query\u{fffd}spoof"));
    assert!(!rendered.contains('\u{202e}'));
    assert!(rendered.contains("No matching commands."));
    assert_eq!(app.command_palette_query, query);
    assert_eq!(app.command_palette_choice(), None);
}
