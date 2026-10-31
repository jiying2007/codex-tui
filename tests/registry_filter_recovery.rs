//! Filter recovery is presentation-only and must not invent an empty upstream inventory.
use codex_tui::{
    app::{AppState, ViewKind},
    backend::{CodexBackend, FakeBackend},
    command::Command,
    i18n::UiLanguage,
    keymap::command_for_key,
    ui,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

fn app() -> AppState {
    let snapshot = FakeBackend::scaled(3).snapshot();
    let mut app = AppState::new(snapshot.threads);
    app.backend_status = snapshot.status;
    app
}

fn screen(app: &AppState, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            let mut line = String::new();
            let mut x = 0;
            while x < buffer.area.width {
                let symbol = buffer[(x, y)].symbol();
                line.push_str(symbol);
                x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn hint(language: UiLanguage) -> &'static str {
    if language.is_simplified_chinese() {
        "无匹配 · / l g ?"
    } else {
        "No matches · / l g ?"
    }
}

#[test]
fn unmatched_search_explains_recovery_in_narrow_bilingual_windows() {
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        let mut app = app();
        app.language = language;
        app.filter = "__unmatched_recovery_fixture__".into();
        let threads = app.threads.clone();
        let selected = app.selected;
        assert!(app.visible_indices().is_empty());
        for (width, height) in [(24, 12), (40, 12), (80, 24)] {
            let rendered = screen(&app, width, height);
            assert!(
                rendered.contains(hint(language)),
                "{width}x{height}: {rendered}"
            );
        }
        assert_eq!(app.threads, threads);
        assert_eq!(app.selected, selected);
        assert_eq!(app.filter, "__unmatched_recovery_fixture__");
    }
}

#[test]
fn repository_scope_can_be_recovered_without_changing_the_search() {
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        let mut app = app();
        app.language = language;
        app.repo_backed_only = true;
        assert!(app.git_contexts.is_empty());
        assert!(app.visible_indices().is_empty());
        assert!(screen(&app, 40, 12).contains(hint(language)));
        assert!(app.repo_backed_only);
        assert!(app.filter.is_empty());
        app.repo_backed_only = false;
        assert_eq!(app.visible_indices().len(), 3);
        assert!(!screen(&app, 40, 12).contains(hint(language)));
    }
}

#[test]
fn unobserved_empty_or_failed_inventory_is_not_called_a_filter_miss() {
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        for (connected, complete, failed) in [
            (false, false, false),
            (false, false, true),
            (true, false, false),
            (true, true, false),
        ] {
            let mut app = AppState::new(vec![]);
            app.language = language;
            app.backend_status.connected = connected;
            app.backend_status.registry_complete = complete;
            app.backend_status.error = failed.then(|| "fixture unavailable".into());
            app.filter = "__unmatched_recovery_fixture__".into();
            assert!(!screen(&app, 40, 12).contains(hint(language)));
        }
    }
}

#[test]
fn clearing_search_and_resizing_restore_rows_without_a_phantom_hint() {
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        let mut app = app();
        app.language = language;
        app.filter = "__unmatched_recovery_fixture__".into();
        assert!(screen(&app, 40, 12).contains(hint(language)));
        app.filter.clear();
        for width in [80, 24, 40] {
            assert_eq!(app.visible_indices().len(), 3);
            assert!(!screen(&app, width, 24).contains(hint(language)));
        }
        app.filter = "__unmatched_recovery_fixture__".into();
        app.terminal_drawer_open = true;
        assert!(screen(&app, 40, 30).contains(hint(language)));
    }
}

#[test]
fn suggested_keys_use_existing_registry_commands_only() {
    for (key, expected) in [
        ('/', Command::Search),
        ('l', Command::ToggleHostLocalFilter),
        ('g', Command::ToggleRepoBackedFilter),
        ('?', Command::Help),
    ] {
        let key = KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE);
        assert_eq!(command_for_key(key, ViewKind::Registry), Some(expected));
    }
}
