use super::*;
use codex_tui::{app::View, conversation::parse_interactive_request};
use serde_json::json;

fn app() -> AppState {
    let request = parse_interactive_request(&json!({
        "id": 1, "method": "item/commandExecution/requestApproval", "params": {
            "threadId": "details", "turnId": "turn", "itemId": "item",
            "command": "echo safe", "cwd": "/repo"
        }
    }))
    .unwrap()
    .unwrap();
    let mut app = AppState::new(vec![]);
    app.view = View::Thread(request.thread_id.clone());
    app.pending_requests.push(request);
    app
}

#[test]
fn detail_keys_and_paste_never_submit_or_cancel_and_escape_only_returns() {
    let mut app = app();
    let pending = app.pending_requests.clone();
    let view = app.view.clone();
    assert!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)
        )
        .is_empty()
    );
    assert!(app.request_details_open());
    for code in [
        KeyCode::Char('y'),
        KeyCode::Char('n'),
        KeyCode::Char('c'),
        KeyCode::Char('i'),
        KeyCode::Char('a'),
        KeyCode::Enter,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
    ] {
        assert!(handle_key(&mut app, KeyEvent::new(code, KeyModifiers::NONE)).is_empty());
        assert!(app.request_details_open());
        assert_eq!(app.pending_requests, pending);
    }
    for code in [KeyCode::Char('c'), KeyCode::Char('k')] {
        assert!(handle_key(&mut app, KeyEvent::new(code, KeyModifiers::CONTROL)).is_empty());
    }
    assert!(handle_paste(&mut app, "y\nn\nc".into()).is_empty());
    assert!(app.input_buffer.is_empty());
    assert!(handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)).is_empty());
    assert!(!app.request_details_open());
    assert_eq!(app.pending_requests, pending);
    assert_eq!(app.view, view);
}

#[test]
fn d_remains_input_in_an_editor_and_does_not_open_over_goal_actions() {
    let mut app = app();
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    );
    assert_eq!(app.input_mode, InputMode::Composer);
    assert_eq!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)
        ),
        vec![Effect::PersistOperatorStateDeferred]
    );
    assert_eq!(app.thread_ui["details"].draft, "d");
    assert!(app.input_buffer.is_empty());
    assert!(!app.request_details_open());
    handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.input_mode, InputMode::Normal);
    app.goal_actions_open = true;
    assert!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)
        )
        .is_empty()
    );
    assert!(!app.request_details_open());
}

#[test]
fn modified_or_repeated_confirmation_keys_cannot_resolve_a_live_request() {
    let mut app = app();
    let pending = app.pending_requests.clone();
    for code in [
        KeyCode::Char('y'),
        KeyCode::Char('n'),
        KeyCode::Char('i'),
        KeyCode::Char('c'),
    ] {
        for modifiers in [KeyModifiers::SHIFT, KeyModifiers::ALT, KeyModifiers::SUPER] {
            assert!(handle_key(&mut app, KeyEvent::new(code, modifiers)).is_empty());
            assert_eq!(app.pending_requests, pending);
        }
        let repeat = KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Repeat);
        assert!(handle_key(&mut app, repeat).is_empty());
        assert_eq!(app.pending_requests, pending);
    }
    for code in [KeyCode::Char('y'), KeyCode::Char('n'), KeyCode::Char('i')] {
        assert!(handle_key(&mut app, KeyEvent::new(code, KeyModifiers::CONTROL)).is_empty());
        assert_eq!(app.pending_requests, pending);
    }
}

#[test]
fn modified_goal_actions_are_not_executed_behind_the_goal_dialog() {
    let mut app = app();
    app.goal_actions_open = true;
    for code in [KeyCode::Char('c'), KeyCode::Char('p'), KeyCode::Char('r')] {
        assert!(handle_key(&mut app, KeyEvent::new(code, KeyModifiers::ALT)).is_empty());
        assert!(app.goal_actions_open);
    }
}
