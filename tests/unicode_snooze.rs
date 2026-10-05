//! Invalid Unicode duration input must not unwind the production reducer.
use codex_tui::{
    app::{Action, AppState, InputMode, reduce},
    backend::{CodexBackend, FakeBackend},
};

fn reject_invalid(mode: InputMode) {
    for input in [
        "中",
        "1小时",
        "15分",
        "🔔",
        "é",
        "e\u{301}",
        "1m💤",
        "0m",
        "18446744073709551615d",
    ] {
        let mut state = AppState::new(FakeBackend::scaled(2).snapshot().threads);
        state.input_mode = mode;
        state.input_buffer = input.into();
        let before = state.to_local_state();
        assert!(
            reduce(&mut state, Action::CommitInput).is_empty(),
            "unexpected effect for {input:?}"
        );
        assert_eq!(state.input_mode, mode);
        assert_eq!(state.input_buffer, input);
        assert_eq!(state.to_local_state(), before);
        assert!(state.pending_local_batch.is_none());
    }
}

#[test]
fn invalid_unicode_snooze_is_rejected_without_losing_editor_or_state() {
    reject_invalid(InputMode::Snooze);
}

#[test]
fn invalid_unicode_batch_snooze_is_rejected_without_a_write_plan() {
    reject_invalid(InputMode::BatchSnooze);
}
