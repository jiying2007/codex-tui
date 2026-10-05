//! Real palette Enter path, with production state refresh between open and commit.
use super::*;
use crate::runtime_commands::handle_command;
use codex_tui::{
    app::{InputMode, View},
    backend::{CodexBackend, FakeBackend},
    command::Command,
    i18n::UiLanguage,
};

fn fixture() -> AppState {
    let mut threads = FakeBackend::scaled(2).snapshot().threads;
    for thread in &mut threads {
        thread.pinned = false;
    }
    let mut app = AppState::new(threads);
    app.host_local_only = false;
    app.repo_backed_only = false;
    app.show_all_history = true;
    app
}

fn choose(app: &mut AppState, command: Command) {
    reduce(app, Action::OpenCommandPalette);
    app.command_palette_selected = app.command_palette_choices().iter()
        .position(|candidate| *candidate == command).expect("available command");
}

fn enter(app: &mut AppState) -> Vec<Effect> {
    handle_command_palette_key(app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

fn refused(app: &mut AppState) {
    let before = app.to_local_state();
    let view = app.view.clone();
    assert!(enter(app).is_empty(), "stale command emitted an effect");
    assert_eq!(app.to_local_state(), before, "stale command changed operator state");
    assert_eq!(app.view, view, "stale command changed the active view");
    assert!(!app.command_palette_open);
    assert!(app.mutation_notice.is_some(), "missing explicit stale-target notice");
    assert_eq!(app.input_mode, InputMode::Normal);
}

#[test]
fn pin_does_not_follow_a_different_registry_row() {
    let mut app = fixture();
    choose(&mut app, Command::TogglePin);
    app.selected = 1;
    refused(&mut app);
    assert!(!app.threads[1].pinned);
}

#[test]
fn quick_prompt_does_not_follow_a_different_registry_row() {
    let mut app = fixture();
    choose(&mut app, Command::QuickPrompt);
    app.selected = 1;
    refused(&mut app);
}

#[test]
fn snooze_does_not_follow_a_different_registry_row() {
    let mut app = fixture();
    choose(&mut app, Command::Snooze);
    app.selected = 1;
    refused(&mut app);
    assert!(app.snooze_target.is_none());
}

#[test]
fn context_actions_do_not_open_for_a_different_target() {
    let mut app = fixture();
    choose(&mut app, Command::ContextActions);
    app.selected = 1;
    refused(&mut app);
    assert!(!app.context_open);
}

#[test]
fn review_rejects_same_thread_with_changed_cwd() {
    let mut app = fixture();
    choose(&mut app, Command::Review);
    let mut threads = app.threads.clone();
    threads[0].metadata.cwd = "/different/repository".into();
    reduce(&mut app, Action::ReplaceThreads(threads));
    refused(&mut app);
}

#[test]
fn scratch_does_not_inherit_a_changed_workspace() {
    let mut app = fixture();
    choose(&mut app, Command::New);
    let mut threads = app.threads.clone();
    threads[0].workspace = "another-workspace".into();
    reduce(&mut app, Action::ReplaceThreads(threads));
    refused(&mut app);
    assert!(app.new_scratch_workspace.is_none());
}

#[test]
fn stale_registry_pin_command_cannot_execute_in_thread_view() {
    let mut app = fixture();
    choose(&mut app, Command::TogglePin);
    app.view = View::Thread(app.threads[0].id.clone());
    refused(&mut app);
}

#[test]
fn terminal_open_cannot_become_a_focus_existing_drawer_action() {
    let mut app = fixture();
    choose(&mut app, Command::TerminalDrawer);
    app.terminal_drawer_open = true;
    app.terminal_focused = false;
    refused(&mut app);
    assert!(!app.terminal_focused);
}

#[test]
fn removed_thread_cannot_receive_a_queue_refresh() {
    let mut app = fixture();
    app.view = View::Thread(app.threads[0].id.clone());
    choose(&mut app, Command::ThreadQueue);
    let threads = app.threads[1..].to_vec();
    reduce(&mut app, Action::ReplaceThreads(threads));
    refused(&mut app);
    assert!(!app.thread_queue_open);
}

#[test]
fn closed_palette_cannot_dispatch_a_second_enter() {
    let mut app = fixture();
    choose(&mut app, Command::TogglePin);
    assert_eq!(enter(&mut app), vec![Effect::PersistOperatorState]);
    assert!(enter(&mut app).is_empty());
    assert_eq!(app.input_mode, InputMode::Normal);
}

#[test]
fn unchanged_target_pin_executes_once() {
    let mut app = fixture();
    choose(&mut app, Command::TogglePin);
    assert_eq!(enter(&mut app), vec![Effect::PersistOperatorState]);
    assert!(app.threads[0].pinned);
    assert!(!app.command_palette_open);
}

#[test]
fn identity_survives_reordering_when_same_item_remains_selected() {
    let mut app = fixture();
    let original = app.threads[0].id.clone();
    choose(&mut app, Command::TogglePin);
    let mut threads = app.threads.clone();
    threads.reverse();
    reduce(&mut app, Action::ReplaceThreads(threads));
    app.selected = app.threads.iter().position(|thread| thread.id == original).unwrap();
    assert_eq!(enter(&mut app), vec![Effect::PersistOperatorState]);
    assert!(app.threads[app.selected].pinned);
}

#[test]
fn metadata_only_refresh_does_not_block_pin() {
    let mut app = fixture();
    choose(&mut app, Command::TogglePin);
    app.threads[0].alias = Some("display text changed".into());
    assert_eq!(enter(&mut app), vec![Effect::PersistOperatorState]);
}

#[test]
fn global_help_remains_available_after_selection_removal() {
    let mut app = fixture();
    choose(&mut app, Command::Help);
    reduce(&mut app, Action::ReplaceThreads(vec![]));
    assert!(enter(&mut app).is_empty());
    assert!(app.show_help);
}

#[test]
fn cancel_and_reopen_capture_the_new_target() {
    let mut app = fixture();
    choose(&mut app, Command::TogglePin);
    reduce(&mut app, Action::CloseCommandPalette);
    app.selected = 1;
    choose(&mut app, Command::TogglePin);
    assert_eq!(enter(&mut app), vec![Effect::PersistOperatorState]);
    assert!(app.threads[1].pinned);
    assert!(!app.threads[0].pinned);
}

#[test]
fn direct_commands_remain_live_without_opening_a_palette() {
    let mut app = fixture();
    app.selected = 1;
    assert_eq!(handle_command(&mut app, Command::TogglePin), vec![Effect::PersistOperatorState]);
    assert!(app.threads[1].pinned);
}

#[test]
fn chinese_refusal_is_visible_and_executes_nothing() {
    let mut app = fixture();
    app.language = UiLanguage::SimplifiedChinese;
    choose(&mut app, Command::TogglePin);
    app.selected = 1;
    refused(&mut app);
    assert!(app.mutation_notice.as_ref().unwrap().contains("未执行"));
}
