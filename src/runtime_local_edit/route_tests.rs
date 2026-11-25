//! Exercise the retained production note dispatcher and real SQLite receipts.
use super::*;
use codex_tui::{
    app::{ContextChoice, Effect, InputMode},
    backend::{CodexBackend, FakeBackend},
    notification::NotificationMode,
    planning::{LocalNote, SourceKind, SourceRef, WorkCardRecord, builtin_saved_views},
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

#[tokio::test]
async fn scoped_note_and_snooze_are_durable_without_activating_legacy_preferences() {
    use codex_tui::{
        app_server_target::ResolvedAppServerTarget,
        backend::BackendStatus,
        store::{operator_scope_key, target_instance_identity},
    };
    let root = tempfile::tempdir().unwrap();
    let worker = StoreWorker::at_for_test(root.path());
    let sql = worker.sqlite_clone();
    let mut services = crate::RuntimeServices::new(worker, NotificationMode::Off);
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let target = ResolvedAppServerTarget::implicit_local(None);
    let id = app.threads[0].id.clone();
    sql.upsert_note(&LocalNote {
        owner: SourceRef::codex_thread(&id),
        text: "historical private note".into(),
        updated_at_unix_ms: 1,
    })
    .unwrap();
    let mut card = WorkCardRecord::implicit_thread(&id);
    card.overlay.snooze_until_unix_ms = Some(100);
    card.overlay.done_at_unix_ms = Some(1);
    sql.upsert_work_card(&card).unwrap();
    let before = sql.load_planning_snapshot().unwrap();
    app.configure_operator_target(&target);
    app.apply_local_state(&LocalStateV1::default());
    let mut status = BackendStatus::starting(target.status_source());
    status.connected = true;
    status.codex_home = Some("/home/a".into());
    reduce(&mut app, Action::BackendStatus(status.clone()));
    reduce(&mut app, Action::PlanningSnapshotLoaded(before.clone()));
    assert!(app.session_note(&id).is_none());
    assert!(!app.session_attention_snoozed(&app.threads[0]));
    let effects = draft(&mut app);
    let scope = operator_scope_key(&target_instance_identity(&target), Some("/home/a"));
    let owner = SourceRef::scoped_session(&scope, &id.0);
    assert_eq!(owner.kind, SourceKind::ScopedSession);
    assert_eq!(
        effects,
        vec![Effect::SaveSourceNote {
            owner: owner.clone(),
            text: "retained note".into(),
        }]
    );
    crate::apply_effects(&mut app, None, &mut services, effects).unwrap();
    settle(&mut app, &mut services);
    assert_eq!(app.session_note(&id), Some("retained note"));

    reduce(&mut app, Action::BeginSnooze);
    let effects = reduce(&mut app, Action::CommitInput);
    assert_eq!(app.input_mode, InputMode::Snooze);
    assert!(app.pending_local_edit_ticket().is_some());
    crate::apply_effects(&mut app, None, &mut services, effects).unwrap();
    settle(&mut app, &mut services);
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.session_attention_snoozed(&app.threads[0]));
    let after = sql.load_planning_snapshot().unwrap();
    assert!(after.notes.contains(&before.notes[0]));
    assert!(after.cards.contains(&before.cards[0]));
    assert!(after.notes.iter().any(|note| note.owner == owner));
    assert!(after.cards.iter().any(|card| card.anchor == owner));

    reduce(&mut app, Action::BeginSnooze);
    status.codex_home = Some("/home/b".into());
    reduce(&mut app, Action::BackendStatus(status.clone()));
    assert!(app.session_note(&id).is_none());
    assert!(!app.session_attention_snoozed(&app.threads[0]));
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(app.input_mode, InputMode::Snooze);
    assert!(app.pending_local_edit_ticket().is_none());
    reduce(&mut app, Action::CancelInput);
    status.codex_home = Some("/home/a".into());
    reduce(&mut app, Action::BackendStatus(status.clone()));
    assert_eq!(app.session_note(&id), Some("retained note"));
    status.connected = false;
    reduce(&mut app, Action::BackendStatus(status));
    assert!(app.session_note(&id).is_none());
    assert!(!app.session_attention_snoozed(&app.threads[0]));
    assert_eq!(sql.load_planning_snapshot().unwrap(), after);
}
