//! Exercise the production effect dispatcher, not a parallel submission model.
use super::*;
use codex_tui::{
    app::{Effect, InputMode},
    backend::{CodexBackend, FakeBackend},
    notification::NotificationMode,
    planning::{SourceKind, SourceRef, builtin_saved_views},
    saved_view_editor::SavedViewEditor,
    store::LocalStateV1,
};
use std::time::{Duration, Instant};

fn draft(app: &mut AppState, kind: usize, scratch_id: String) -> Vec<Effect> {
    match kind {
        0 => {
            reduce(app, Action::BeginScratch);
            reduce(app, Action::InputText("retained scratch".into()));
        }
        1 | 2 => {
            app.input_mode = InputMode::Note;
            app.input_buffer = "retained note".into();
            app.note_target = Some(if kind == 1 {
                SourceRef::codex_thread(&app.threads[0].id)
            } else {
                SourceRef {
                    kind: SourceKind::ScratchWork,
                    value: scratch_id,
                }
            });
        }
        3 => {
            app.saved_view_editor = Some(SavedViewEditor::create_from(&builtin_saved_views()[0]));
            return reduce(app, Action::SaveSavedViewEditor);
        }
        _ => panic!("unknown test case"),
    }
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
async fn all_four_editor_effects_clear_only_after_actual_sqlite_receipts() {
    for kind in 0..4 {
        let root = tempfile::tempdir().unwrap();
        let worker = StoreWorker::at_for_test(root.path());
        let sql = worker.sqlite_clone();
        let scratch = sql.create_scratch("existing", None, None, None).unwrap();
        let mut services = crate::RuntimeServices::new(worker, NotificationMode::Off);
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        let effects = draft(&mut app, kind, scratch.id.clone());
        assert_eq!(effects.len(), 1);
        assert!(app.pending_local_edit_ticket().is_some());
        crate::apply_effects(&mut app, None, &mut services, effects).unwrap();
        assert!(
            app.pending_local_edit_ticket().is_some(),
            "dispatch is not commit"
        );
        settle(&mut app, &mut services);
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.input_buffer.is_empty());
        assert!(app.saved_view_editor.is_none());
        assert!(app.note_target.is_none());
        let snapshot = sql.load_planning_snapshot().unwrap();
        match kind {
            0 => assert_eq!(snapshot.scratch.len(), 2),
            1 => assert_eq!(snapshot.notes[0].text, "retained note"),
            2 => assert_eq!(
                snapshot
                    .scratch
                    .iter()
                    .find(|s| s.id == scratch.id)
                    .unwrap()
                    .note
                    .as_deref(),
                Some("retained note")
            ),
            3 => assert_eq!(snapshot.saved_views.len(), 1),
            _ => unreachable!(),
        }
        services
            .store
            .flush_operator_state_on_exit(&LocalStateV1::default())
            .unwrap();
    }
}

#[tokio::test]
async fn all_four_editor_effects_retain_drafts_after_actual_sqlite_failure() {
    for kind in 0..4 {
        let root = tempfile::tempdir().unwrap();
        let blocked = root.path().join("not-a-directory");
        std::fs::write(&blocked, b"keep").unwrap();
        let mut services =
            crate::RuntimeServices::new(StoreWorker::at_for_test(&blocked), NotificationMode::Off);
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        let effects = draft(&mut app, kind, "existing".into());
        let before = (
            app.input_mode,
            app.input_buffer.clone(),
            app.note_target.clone(),
            app.saved_view_editor.clone(),
        );
        crate::apply_effects(&mut app, None, &mut services, effects).unwrap();
        settle(&mut app, &mut services);
        assert_eq!(
            (
                app.input_mode,
                app.input_buffer.clone(),
                app.note_target.clone(),
                app.saved_view_editor.clone()
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
}

#[test]
fn same_mode_different_owner_is_not_cleared_by_old_success() {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let effects = draft(&mut app, 1, String::new());
    let ticket = app.local_edit_ticket_for(&effects[0]).unwrap();
    app.note_target = Some(SourceRef::codex_thread(&app.threads[1].id));
    app.finish_local_edit_write(ticket, Ok(()));
    assert_eq!(app.input_mode, InputMode::Note);
    assert_eq!(app.input_buffer, "retained note");
}

#[test]
fn same_mode_replaced_view_is_not_closed_or_given_an_unrelated_error() {
    for success in [true, false] {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        let effects = draft(&mut app, 3, String::new());
        let ticket = app.local_edit_ticket_for(&effects[0]).unwrap();
        app.saved_view_editor.as_mut().unwrap().draft.name = "new view".into();
        app.finish_local_edit_write(
            ticket,
            if success {
                Ok(())
            } else {
                Err("older write".into())
            },
        );
        assert_eq!(app.saved_view_editor.unwrap().draft.name, "new view");
        assert!(app.saved_view_editor_error.is_none());
    }
}
