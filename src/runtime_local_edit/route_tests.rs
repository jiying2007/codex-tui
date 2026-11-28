//! Exercise the retained production note dispatcher and real SQLite receipts.
use super::*;
use codex_tui::{
    app::{ContextChoice, Effect, InputMode},
    backend::{CodexBackend, FakeBackend},
    notification::NotificationMode,
    planning::{SourceRef, builtin_saved_views},
    store::LocalStateV1,
};
use std::time::{Duration, Instant};

fn draft(app: &mut AppState) -> Vec<Effect> {
    reduce(app, Action::OpenContext);
    app.context_selected = app
        .context_choices()
        .iter()
        .position(|choice| *choice == ContextChoice::EditNote)
        .unwrap();
    reduce(app, Action::ExecuteContext);
    reduce(app, Action::InputText("retained note".into()));
    reduce(app, Action::CommitInput)
}

fn settle(app: &mut AppState, services: &mut crate::RuntimeServices) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.pending_local_edit_ticket().is_some() {
        if let Some(event) = services.store.try_event() {
            apply_event(app, event);
        }
        assert!(Instant::now() < deadline, "missing production save receipt");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[tokio::test]
async fn session_note_commits_without_mutating_existing_scratch_or_saved_views() {
    let root = tempfile::tempdir().unwrap();
    let worker = StoreWorker::at_for_test(root.path());
    let sql = worker.sqlite_clone();
    sql.create_scratch("existing", Some("historical"), None, None)
        .unwrap();
    let mut view = builtin_saved_views().remove(0);
    view.id.clear();
    view.name = "historical view".into();
    sql.save_view(&view).unwrap();
    let before = sql.load_planning_snapshot().unwrap();

    let mut services = crate::RuntimeServices::new(worker, NotificationMode::Off);
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let owner = SourceRef::codex_thread(&app.threads[0].id);
    let effects = draft(&mut app);
    assert_eq!(
        effects,
        vec![Effect::SaveSourceNote {
            owner: owner.clone(),
            text: "retained note".into(),
        }]
    );
    assert!(app.pending_local_edit_ticket().is_some());
    crate::apply_effects(&mut app, None, &mut services, effects).unwrap();
    assert!(
        app.pending_local_edit_ticket().is_some(),
        "dispatch is not commit"
    );
    settle(&mut app, &mut services);
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.input_buffer.is_empty());
    assert!(app.note_target.is_none());
    let after = sql.load_planning_snapshot().unwrap();
    assert_eq!(after.scratch, before.scratch);
    assert_eq!(after.saved_views, before.saved_views);
    assert_eq!(after.notes.len(), 1);
    assert_eq!(after.notes[0].owner, owner);
    assert_eq!(after.notes[0].text, "retained note");
    services
        .store
        .flush_operator_state_on_exit(&LocalStateV1::default())
        .unwrap();
}

#[tokio::test]
async fn session_note_retains_draft_after_actual_sqlite_failure() {
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("not-a-directory");
    std::fs::write(&blocked, b"keep").unwrap();
    let mut services =
        crate::RuntimeServices::new(StoreWorker::at_for_test(&blocked), NotificationMode::Off);
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let effects = draft(&mut app);
    let before = (
        app.input_mode,
        app.input_buffer.clone(),
        app.note_target.clone(),
    );
    crate::apply_effects(&mut app, None, &mut services, effects).unwrap();
    settle(&mut app, &mut services);
    assert_eq!(
        (
            app.input_mode,
            app.input_buffer.clone(),
            app.note_target.clone()
        ),
        before
    );
    assert!(app.planning_store_error.is_some());
    assert!(
        services
            .store
            .flush_operator_state_on_exit(&LocalStateV1::default())
            .is_err()
    );
    assert_eq!(std::fs::read(blocked).unwrap(), b"keep");
}

#[test]
fn same_mode_different_owner_is_not_cleared_by_old_success() {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let effects = draft(&mut app);
    let ticket = app.local_edit_ticket_for(&effects[0]).unwrap();
    app.note_target = Some(SourceRef::codex_thread(&app.threads[1].id));
    app.finish_local_edit_write(ticket, Ok(()));
    assert_eq!(app.input_mode, InputMode::Note);
    assert_eq!(app.input_buffer, "retained note");
}
