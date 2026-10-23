use super::*;
use crate::app::{Action, ViewKind, reduce};
use crate::backend::{CodexBackend, FakeBackend};
use crate::command::Command;
use crate::conversation::{InteractiveRequestKind, RpcRequestId};
use crate::keymap::command_for_key;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

fn render_app(app: &AppState, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
    terminal.draw(|frame| render(frame, app)).expect("draw");
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

fn app() -> AppState {
    let mut source = FakeBackend::scaled(3).snapshot().threads;
    for thread in &mut source {
        thread.attention.clear();
        thread.pinned = false;
        thread.runtime = RuntimeStatus::Working;
    }
    source[2].attention.push(AttentionReason::ApprovalRequired);
    let mut app = AppState::new(vec![]);
    app.backend_status = FakeBackend::seeded().snapshot().status;
    reduce(&mut app, Action::ReplaceThreads(source));
    app
}

#[test]
fn compact_attention_and_delivery_hints_survive_drawer_in_both_languages() {
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        for width in [20, 40, 79, 80] {
            let mut app = app();
            app.language = language;
            app.terminal_drawer_open = true;
            let snapshot = render_app(&app, width);
            assert!(
                snapshot.contains(attention_reason_label(
                    &AttentionReason::ApprovalRequired,
                    language
                )),
                "width={width} language={language:?}: {snapshot}"
            );
            for key in ['b', 'r', 'w'] {
                assert!(
                    snapshot.contains(key),
                    "missing {key}, width={width}: {snapshot}"
                );
            }
            assert!(snapshot.contains(if language.is_simplified_chinese() {
                "待处理 1"
            } else {
                "1 need attention"
            }));
        }
    }
}

#[test]
fn pending_request_alone_sorts_first_and_renders_interaction_with_drawer() {
    let mut app = app();
    app.threads[2].attention.clear();
    app.selected = 0;
    let request = InteractiveRequest {
        request_id: RpcRequestId::Integer(81),
        thread_id: app.threads[2].id.clone(),
        turn_id: "pending-turn".into(),
        item_id: "pending-input".into(),
        kind: InteractiveRequestKind::UserInput { questions: vec![] },
    };
    assert!(reduce(&mut app, Action::InteractiveRequested(request)).is_empty());
    assert!(app.threads.iter().all(|thread| thread.attention.is_empty()));
    assert_eq!(app.visible_indices(), [2, 0, 1]);
    assert_eq!(
        app.selected, 0,
        "pending arrival must retain manual selection"
    );
    app.terminal_drawer_open = true;
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        app.language = language;
        for width in [20, 40, 79] {
            let snapshot = render_app(&app, width);
            let first_row = snapshot.lines().nth(2).expect("first registry row");
            let label = if language.is_simplified_chinese() {
                "交互"
            } else {
                "interac…"
            };
            assert!(first_row.contains(label), "width={width}: {snapshot}");
            assert!(!first_row.contains("WORKING"));
        }
    }
}

#[test]
fn a_snoozed_thread_without_attention_still_displays_its_runtime() {
    let mut app = app();
    app.threads.truncate(1);
    app.selected = 0;
    let mut local = crate::planning::WorkCardRecord::implicit_thread(&app.threads[0].id);
    local.overlay.snooze_until_unix_ms = Some(10_000);
    app.planning_snapshot.cards.push(local);
    reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1 });
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        app.language = language;
        let snapshot = render_app(&app, 40);
        assert!(snapshot.contains(runtime_status_label(&RuntimeStatus::Working, language)));
        assert!(!snapshot.contains(if language.is_simplified_chinese() {
            "稍后"
        } else {
            "snoozed"
        }));
    }
}

#[test]
fn footer_delivery_keys_match_executable_routes() {
    let app = app();
    for (key, command, label) in [
        ('b', Command::Board, "b Board"),
        ('r', Command::Review, "r Review"),
        ('w', Command::Workspace, "w Workspace"),
    ] {
        assert_eq!(
            command_for_key(
                KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE),
                ViewKind::Registry
            ),
            Some(command)
        );
        assert!(registry_footer(&app, 100).to_string().contains(label));
    }
    assert!(render_app(&app, 100).contains("t terminal"));
}
