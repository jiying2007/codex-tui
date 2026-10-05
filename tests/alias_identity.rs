//! An in-progress alias edit belongs to its original thread, not a list index.
use codex_tui::{
    app::{Action, AppState, Effect, InputMode, reduce},
    backend::{CodexBackend, FakeBackend},
};
fn fixture() -> AppState {
    let mut threads = FakeBackend::scaled(2).snapshot().threads;
    threads[0].title = "Alpha".into();
    threads[1].title = "Beta".into();
    AppState::new(threads)
}
fn begin(state: &mut AppState) {
    assert!(reduce(state, Action::BeginAlias).is_empty());
    assert_eq!(state.input_mode, InputMode::Alias);
    reduce(state, Action::InputText("原始目标的别名".into()));
}
#[test]
fn removed_alias_target_never_retargets_the_replacement_row() {
    let mut state = fixture(); begin(&mut state);
    let remaining = vec![state.threads[1].clone()];
    reduce(&mut state, Action::ReplaceThreads(remaining));
    let before = state.to_local_state();
    assert!(reduce(&mut state, Action::CommitInput).is_empty());
    assert_eq!(state.to_local_state(), before);
    assert_eq!(state.input_mode, InputMode::Alias);
    assert_eq!(state.input_buffer, "原始目标的别名");
    assert!(state.threads[0].alias.is_none());
    assert!(state.mutation_notice.is_some());
}
#[test]
fn refreshed_filter_cannot_redirect_an_existing_alias_editor() {
    let mut state = fixture(); let original = state.threads[0].id.clone(); begin(&mut state);
    state.filter = "Beta".into();
    let refreshed = state.threads.clone();
    reduce(&mut state, Action::ReplaceThreads(refreshed));
    assert_eq!(state.selected, 1);
    assert_eq!(reduce(&mut state, Action::CommitInput), vec![Effect::PersistOperatorState]);
    assert_eq!(state.threads.iter().find(|thread| thread.id == original).unwrap().alias.as_deref(), Some("原始目标的别名"));
    assert!(state.threads[1].alias.is_none());
}
#[test]
fn reordered_alias_target_and_explicit_cancel_keep_identity() {
    let mut state = fixture(); let original = state.threads[0].id.clone(); begin(&mut state);
    let mut refreshed = state.threads.clone(); refreshed.reverse();
    reduce(&mut state, Action::ReplaceThreads(refreshed));
    assert_eq!(reduce(&mut state, Action::CommitInput), vec![Effect::PersistOperatorState]);
    assert_eq!(state.threads.iter().find(|thread| thread.id == original).unwrap().alias.as_deref(), Some("原始目标的别名"));
    state.selected = 0; begin(&mut state);
    let before = state.to_local_state();
    assert!(reduce(&mut state, Action::CancelInput).is_empty());
    assert_eq!(state.to_local_state(), before);
    assert_eq!(state.input_mode, InputMode::Normal);
    assert!(state.input_buffer.is_empty());
}
