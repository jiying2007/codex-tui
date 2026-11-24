use codex_tui::{
    app::{Action, AppState, ContextChoice, Effect, InputMode, reduce},
    backend::{CodexBackend, FakeBackend},
    planning::{SourceKind, SourceRef},
};

fn app() -> AppState {
    AppState::new(FakeBackend::seeded().snapshot().threads)
}
fn note(app: &mut AppState) {
    reduce(app, Action::OpenContext);
    app.context_selected = app
        .context_choices()
        .iter()
        .position(|choice| *choice == ContextChoice::EditNote)
        .unwrap();
    reduce(app, Action::ExecuteContext);
    reduce(app, Action::InputText("retain note draft".into()));
}

#[test]
fn note_draft_and_owner_survive_dispatch() {
    let mut app = app();
    note(&mut app);
    let owner = app.note_target.clone();
    assert!(matches!(
        &reduce(&mut app, Action::CommitInput)[..],
        [Effect::SaveSourceNote { .. }]
    ));
    assert_eq!(app.note_target, owner);
    assert_eq!(app.input_mode, InputMode::Note);
    assert_eq!(app.input_buffer, "retain note draft");
}

#[test]
fn historical_scratch_note_cannot_emit_a_write() {
    let mut app = app();
    app.input_mode = InputMode::Note;
    app.input_buffer = "historical note".into();
    app.note_target = Some(SourceRef {
        kind: SourceKind::ScratchWork,
        value: "scratch:1".into(),
    });
    let before = app.planning_snapshot.clone();
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert!(app.pending_local_edit_ticket().is_none());
    assert_eq!(app.planning_snapshot, before);
    assert_eq!(app.input_buffer, "historical note");
}

#[test]
fn unavailable_store_refuses_note_without_losing_draft() {
    let mut app = app();
    note(&mut app);
    reduce(
        &mut app,
        Action::PlanningStoreDegraded(Some("read-only".into())),
    );
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(app.input_buffer, "retain note draft");
}

#[test]
fn unsubmitted_draft_can_be_explicitly_cancelled() {
    let mut app = app();
    note(&mut app);
    assert!(reduce(&mut app, Action::CancelInput).is_empty());
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.note_target.is_none());
    assert!(app.input_buffer.is_empty());
}
