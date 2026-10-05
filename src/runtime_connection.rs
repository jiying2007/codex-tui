//! Install the initial asynchronous snapshot without losing operator intent.
use codex_tui::{
    app::{Action, AppState, reduce},
    backend::BackendSnapshot,
};
pub(crate) fn install(app: &mut AppState, snapshot: BackendSnapshot) {
    reduce(app, Action::ReplaceThreads(snapshot.threads));
    reduce(app, Action::BackendStatus(snapshot.status));
}
#[cfg(test)]
mod tests {
    use super::*;
    use codex_tui::backend::{CodexBackend, FakeBackend};
    use codex_tui::store::LocalStateV1;
    #[test]
    fn delayed_connection_preserves_operator_changes_after_bootstrap() {
        let bootstrap = LocalStateV1::default();
        let mut app = AppState::new(vec![]);
        app.apply_local_state(&bootstrap);
        reduce(&mut app, Action::ToggleHostLocalFilter);
        reduce(&mut app, Action::ToggleRepoBackedFilter);
        let expected = app.to_local_state();
        install(&mut app, FakeBackend::scaled(3).snapshot());
        assert_eq!(app.to_local_state(), expected);
    }
}
