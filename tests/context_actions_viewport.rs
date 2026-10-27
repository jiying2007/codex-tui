//! Render-only context menu viewport regression; never execute a context operation.
use codex_tui::{
    app::{Action, AppState, ContextChoice, reduce},
    backend::{CodexBackend, FakeBackend},
    i18n::UiLanguage,
    planning::WorkflowStage,
    ui,
};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, style::Modifier};
use unicode_width::UnicodeWidthStr;

fn fixture(language: UiLanguage) -> AppState {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.language = language;
    reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 10 });
    reduce(&mut app, Action::OpenBoard);
    for stage in 0..WorkflowStage::ALL.len() {
        app.board_stage_index = stage;
        if !app.visible_planning_cards().is_empty() {
            break;
        }
    }
    assert!(
        !app.visible_planning_cards().is_empty(),
        "fixture needs a populated Board column"
    );
    reduce(&mut app, Action::OpenContext);
    assert!(app.context_open);
    assert!(
        app.context_choices().len() > 10,
        "Board context needs enough actions to exceed a short viewport"
    );
    assert_eq!(app.context_selected, 0);
    app
}

fn draw(terminal: &mut Terminal<TestBackend>, app: &AppState) -> Buffer {
    let before = (app.context_selected, app.context_choices());
    terminal.draw(|frame| ui::render(frame, app)).expect("draw");
    assert_eq!(before, (app.context_selected, app.context_choices()));
    terminal.backend().buffer().clone()
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

fn assert_selected_visible(buffer: &Buffer, label: &str) {
    let marker = format!("> {label}");
    let mut visible = 0;
    for y in 0..buffer.area.height {
        let line = row(buffer, y);
        if let Some(start) = line.find(&marker) {
            let x = line[..start].width() as u16;
            if buffer[(x, y)].modifier.contains(Modifier::REVERSED) {
                visible += 1;
                assert!(buffer[(x + 1, y)].modifier.contains(Modifier::REVERSED));
            }
        }
    }
    assert_eq!(visible, 1, "selected context action {marker:?} is clipped");
}

fn navigate(app: &mut AppState, delta: i32) {
    assert!(
        reduce(app, Action::MoveContext(delta)).is_empty(),
        "context navigation must not execute an action"
    );
}

#[test]
fn selected_first_and_last_action_remain_visible_in_short_bilingual_terminals() {
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        for (width, height) in [(80, 12), (40, 12), (80, 24)] {
            let mut app = fixture(language);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
            let first = if language.is_simplified_chinese() {
                "稍后提醒…"
            } else {
                "Snooze attention…"
            };
            let last = if language.is_simplified_chinese() {
                "保存当前视图…"
            } else {
                "Save current view as…"
            };
            assert_eq!(app.context_choice(), Some(ContextChoice::Snooze));
            assert_selected_visible(&draw(&mut terminal, &app), first);

            navigate(&mut app, -1);
            assert_eq!(app.context_choice(), Some(ContextChoice::SaveCurrentView));
            assert_selected_visible(&draw(&mut terminal, &app), last);

            navigate(&mut app, 1);
            assert_eq!(app.context_choice(), Some(ContextChoice::Snooze));
            assert_selected_visible(&draw(&mut terminal, &app), first);
        }
    }
}

#[test]
fn resizing_one_terminal_retains_selection_without_mutating_menu_state() {
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        let mut app = fixture(language);
        navigate(&mut app, -1);
        let selected = app.context_choice();
        let choices = app.context_choices();
        let last = if language.is_simplified_chinese() {
            "保存当前视图…"
        } else {
            "Save current view as…"
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        for (width, height) in [(80, 24), (80, 12), (40, 12), (80, 24)] {
            terminal.backend_mut().resize(width, height);
            terminal
                .resize(Rect::new(0, 0, width, height))
                .expect("resize");
            assert_selected_visible(&draw(&mut terminal, &app), last);
            assert_eq!(app.context_choice(), selected);
            assert_eq!(app.context_choices(), choices);
        }
    }
}

#[test]
fn stale_selection_is_bounded_without_panicking_or_mutating_actions() {
    let mut app = fixture(UiLanguage::English);
    let len = app.context_choices().len();
    for stale in [len, usize::MAX] {
        app.context_selected = stale;
        for (width, height) in [(80, 24), (40, 12), (1, 1)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
            draw(&mut terminal, &app);
            assert_eq!(app.context_selected, stale);
            assert_eq!(app.context_choice(), None);
        }
    }
}
