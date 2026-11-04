//! Keyboard confirmation regressions: no real backend, PTY, model or network.
use super::*;
use codex_tui::{
    app::{ContextChoice, View},
    backend::{CodexBackend, FakeBackend},
    command::Command,
    transcript_search::{
        ThreadSearchCandidate, TranscriptSearchResults, TranscriptSearchSource, thread_level_hit,
    },
};

fn fixture() -> AppState {
    AppState::new(FakeBackend::seeded().snapshot().threads)
}

fn forbidden(code: KeyCode) -> Vec<KeyEvent> {
    let mut keys = [
        KeyModifiers::SHIFT,
        KeyModifiers::ALT,
        KeyModifiers::CONTROL,
        KeyModifiers::SUPER,
    ]
    .into_iter()
    .map(|modifiers| KeyEvent::new(code, modifiers))
    .collect::<Vec<_>>();
    keys.push(KeyEvent::new_with_kind(
        code,
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    ));
    keys.push(KeyEvent::new_with_kind(
        code,
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    keys
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn palette_commit_requires_one_plain_press_and_preserves_open_intent_otherwise() {
    let mut app = fixture();
    reduce(&mut app, Action::OpenCommandPalette);
    let before_pin = app.threads[0].pinned;
    app.command_palette_selected = app
        .command_palette_choices()
        .iter()
        .position(|choice| *choice == Command::TogglePin)
        .expect("registry palette contains TogglePin");
    for key in forbidden(KeyCode::Enter) {
        assert!(handle_key(&mut app, key).is_empty());
        assert!(app.command_palette_open);
        assert_eq!(app.threads[0].pinned, before_pin);
    }
    assert_eq!(
        handle_key(&mut app, press(KeyCode::Enter)),
        vec![Effect::PersistOperatorState]
    );
    assert_ne!(app.threads[0].pinned, before_pin);
    assert!(!app.command_palette_open);
}

#[test]
fn context_menu_and_launch_picker_cannot_commit_modified_or_held_enter() {
    let mut app = fixture();
    reduce(&mut app, Action::OpenContext);
    assert_eq!(app.context_choice(), Some(ContextChoice::Snooze));
    for key in forbidden(KeyCode::Enter) {
        assert!(handle_key(&mut app, key).is_empty());
        assert!(app.context_open);
        assert_eq!(app.input_mode, InputMode::Normal);
    }
    assert!(handle_key(&mut app, press(KeyCode::Enter)).is_empty());
    assert_eq!(app.input_mode, InputMode::Snooze);

    let mut app = fixture();
    app.launch_menu_open = true;
    for key in forbidden(KeyCode::Enter) {
        assert!(handle_key(&mut app, key).is_empty());
        assert!(app.launch_menu_open);
    }
    assert!(handle_key(&mut app, press(KeyCode::Enter)).is_empty());
    assert!(!app.launch_menu_open);
}

#[test]
fn saved_view_edit_and_save_require_plain_press() {
    let mut app = fixture();
    reduce(&mut app, Action::OpenBoard);
    reduce(&mut app, Action::OpenContext);
    app.context_selected = app
        .context_choices()
        .iter()
        .position(|choice| *choice == ContextChoice::SaveCurrentView)
        .expect("Board allows saving a view");
    reduce(&mut app, Action::ExecuteContext);
    assert!(app.saved_view_editor.is_some());
    for code in [KeyCode::Enter, KeyCode::Char('s')] {
        for key in forbidden(code) {
            assert!(handle_key(&mut app, key).is_empty());
            assert!(app.saved_view_editor.is_some());
            assert!(app.saved_view_editor_error.is_none());
            assert_eq!(app.input_mode, InputMode::Normal);
        }
    }
    assert!(handle_key(&mut app, press(KeyCode::Enter)).is_empty());
    assert_eq!(app.input_mode, InputMode::SavedViewField);
}

#[test]
fn queue_mutation_keys_ignore_modifiers_and_repeat_but_plain_add_still_works() {
    let mut app = fixture();
    app.view = View::Thread(app.threads[0].id.clone());
    app.thread_queue_open = true;
    for code in [
        KeyCode::Char('n'),
        KeyCode::Char('e'),
        KeyCode::Char('x'),
        KeyCode::Char('s'),
        KeyCode::Char('['),
        KeyCode::Char(']'),
        KeyCode::Char('r'),
    ] {
        for key in forbidden(code) {
            assert!(handle_key(&mut app, key).is_empty());
            assert_eq!(app.input_mode, InputMode::Normal);
            assert!(!app.thread_queue_loading);
            assert!(app.pending_queue_confirmation.is_none());
        }
    }
    assert!(handle_key(&mut app, press(KeyCode::Char('n'))).is_empty());
    assert_eq!(app.input_mode, InputMode::ThreadQueueAdd);
}

#[test]
fn text_editor_commit_never_uses_modified_or_held_enter() {
    let mut app = fixture();
    reduce(&mut app, Action::BeginScratch);
    reduce(&mut app, Action::InputText("SCRATCH_DRAFT".into()));
    assert_eq!(app.input_mode, InputMode::ScratchTitle);
    for key in forbidden(KeyCode::Enter) {
        assert!(handle_key(&mut app, key).is_empty());
        assert_eq!(app.input_mode, InputMode::ScratchTitle);
        assert_eq!(app.input_buffer, "SCRATCH_DRAFT");
    }
    let result = handle_key(&mut app, press(KeyCode::Enter));
    assert!(
        !result.is_empty(),
        "plain Enter must still commit the local scratch"
    );
    // Local edits retain the draft until an authoritative write receipt arrives.
    assert_eq!(app.input_mode, InputMode::ScratchTitle);
}

#[test]
fn transcript_selection_requires_plain_enter_without_redirecting_on_repeat() {
    let mut app = fixture();
    let id = app.threads[0].id.clone();
    let mut results = TranscriptSearchResults::empty("needle", TranscriptSearchSource::LocalFts);
    results.hits.push(thread_level_hit(ThreadSearchCandidate {
        thread_id: id.clone(),
        snippet: "needle found".into(),
    }));
    app.transcript_search_open = true;
    app.transcript_search_results = Some(results);
    for key in forbidden(KeyCode::Enter) {
        assert!(handle_key(&mut app, key).is_empty());
        assert!(app.transcript_search_open);
        assert_eq!(app.view, View::Registry);
    }
    let effects = handle_key(&mut app, press(KeyCode::Enter));
    assert!(!app.transcript_search_open);
    assert_eq!(app.view, View::Thread(id));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadConversation(_)))
    );
}

#[test]
fn f6_toggles_drawer_focus_both_ways_without_restarting_pty() {
    let mut app = fixture();
    app.threads[0].metadata.cwd = std::env::current_dir()
        .expect("local cwd")
        .to_string_lossy()
        .into_owned();

    let opened = handle_key(&mut app, press(KeyCode::F(6)));
    assert!(matches!(
        opened.as_slice(),
        [Effect::OpenTerminalDrawer { .. }]
    ));
    assert!(app.terminal_drawer_open);
    assert!(app.terminal_focused);

    // Holding the key or using a modified F6 must never steal PTY focus.
    for key in [
        KeyEvent::new_with_kind(KeyCode::F(6), KeyModifiers::NONE, KeyEventKind::Repeat),
        KeyEvent::new_with_kind(KeyCode::F(6), KeyModifiers::NONE, KeyEventKind::Release),
        KeyEvent::new(KeyCode::F(6), KeyModifiers::SHIFT),
    ] {
        assert!(handle_key(&mut app, key).is_empty());
        assert!(app.terminal_focused);
    }

    assert!(handle_key(&mut app, press(KeyCode::F(6))).is_empty());
    assert!(app.terminal_drawer_open);
    assert!(!app.terminal_focused);

    // Focus the same shell again. No new OpenTerminalDrawer effect is emitted.
    assert!(handle_key(&mut app, press(KeyCode::F(6))).is_empty());
    assert!(app.terminal_focused);
    assert!(app.terminal_drawer_open);

    // Ordinary Ctrl+C still belongs to the PTY while it is focused.
    assert_eq!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
        ),
        vec![Effect::TerminalInput(vec![3])]
    );
    // Preserve the existing Ctrl+] escape route.
    assert!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL)
        )
        .is_empty()
    );
    assert!(!app.terminal_focused);

    // Closing the drawer remains an explicit action; focus changes don't kill it.
    assert_eq!(
        handle_key(&mut app, press(KeyCode::Char('T'))),
        vec![Effect::CloseTerminalDrawer]
    );
    assert!(!app.terminal_drawer_open);
}

#[test]
fn f6_does_not_bypass_modal_or_open_a_drawer_without_a_codex_thread() {
    let mut app = fixture();
    app.view = View::Scratch("scratch:test".into());
    assert!(handle_key(&mut app, press(KeyCode::F(6))).is_empty());
    assert!(!app.terminal_drawer_open);

    app.view = View::Registry;
    reduce(&mut app, Action::ToggleHelp);
    assert!(app.show_help);
    assert!(handle_key(&mut app, press(KeyCode::F(6))).is_empty());
    assert!(app.show_help);
    assert!(!app.terminal_drawer_open);

    reduce(&mut app, Action::ToggleHelp);
    reduce(&mut app, Action::BeginScratch);
    assert_eq!(app.input_mode, InputMode::ScratchTitle);
    assert!(handle_key(&mut app, press(KeyCode::F(6))).is_empty());
    assert!(!app.terminal_drawer_open);
}
