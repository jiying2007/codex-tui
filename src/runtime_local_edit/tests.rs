use super::*;
use codex_tui::{
    app::{Effect, InputMode},
    backend::{CodexBackend, FakeBackend},
    store::LocalStateV1,
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

fn draft() -> (AppState, Vec<Effect>) {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    reduce(&mut app, Action::BeginScratch);
    reduce(&mut app, Action::InputText("durable draft".into()));
    let effects = reduce(&mut app, Action::CommitInput);
    (app, effects)
}
fn save(app: &mut AppState, worker: &mut StoreWorker, effect: Effect) {
    let ticket = app.local_edit_ticket_for(&effect);
    let Effect::CreateScratch { title, workspace } = effect else {
        panic!("scratch effect")
    };
    submit(app, worker, ticket, move |store| {
        store.create_scratch(title, workspace)
    });
}
fn settle(app: &mut AppState, worker: &mut StoreWorker) {
    settle_until(app, worker, Instant::now() + Duration::from_secs(5));
}
fn settle_until(app: &mut AppState, worker: &mut StoreWorker, deadline: Instant) {
    while app.pending_local_edit_ticket().is_some() {
        if let Some(event) = worker.try_event() {
            apply_event(app, event);
        }
        // A received completion wins even if scheduling crossed the deadline.
        // Only a still-pending write can be missing its receipt.
        if app.pending_local_edit_ticket().is_none() {
            return;
        }
        assert!(Instant::now() < deadline, "missing local save receipt");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn real_worker_receipt_retires_only_the_committed_draft() {
    let root = tempfile::tempdir().unwrap();
    let mut worker = StoreWorker::at_for_test(root.path());
    let (mut app, effects) = draft();
    assert_eq!(app.input_buffer, "durable draft");
    save(&mut app, &mut worker, effects[0].clone());
    settle(&mut app, &mut worker);
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.input_buffer.is_empty());
    assert_eq!(app.planning_snapshot.scratch.len(), 1);
    assert_eq!(
        worker
            .sqlite_clone()
            .load_planning_snapshot()
            .unwrap()
            .scratch[0]
            .title,
        "durable draft"
    );
    worker
        .flush_operator_state_on_exit(&LocalStateV1::default())
        .unwrap();
}
#[test]
fn queue_pressure_preserves_draft_and_does_not_poison_the_store() {
    let root = tempfile::tempdir().unwrap();
    let mut worker = StoreWorker::at_for_test(root.path());
    let (started, ready) = mpsc::sync_channel(0);
    let (release, wait) = mpsc::sync_channel(0);
    worker
        .submit(move |_| {
            started.send(()).unwrap();
            wait.recv().unwrap();
            StoreEvent::Notice(None)
        })
        .unwrap();
    ready.recv_timeout(Duration::from_secs(3)).unwrap();
    let mut accepted = 0;
    while worker.submit(|_| StoreEvent::Notice(None)).is_ok() {
        accepted += 1;
        assert!(accepted < 128);
    }
    assert_eq!(accepted, 32);
    let (mut app, effects) = draft();
    save(&mut app, &mut worker, effects[0].clone());
    assert!(app.pending_local_edit_ticket().is_none());
    assert_eq!(app.input_buffer, "durable draft");
    assert!(app.planning_store_error.is_none());
    assert!(
        app.mutation_notice
            .as_ref()
            .unwrap()
            .contains("not accepted")
    );
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = 0;
    while seen < accepted + 1 {
        if let Some(event) = worker.try_event() {
            apply_event(&mut app, event);
            seen += 1;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let retry = reduce(&mut app, Action::CommitInput);
    save(&mut app, &mut worker, retry[0].clone());
    settle(&mut app, &mut worker);
    assert_eq!(app.planning_snapshot.scratch.len(), 1);
    worker
        .flush_operator_state_on_exit(&LocalStateV1::default())
        .unwrap();
}
#[test]
fn real_storage_failure_keeps_draft_and_is_not_misreported_as_rollback() {
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("blocked");
    std::fs::write(&blocked, b"retain parent").unwrap();
    let mut worker = StoreWorker::at_for_test(&blocked);
    let (mut app, effects) = draft();
    save(&mut app, &mut worker, effects[0].clone());
    settle(&mut app, &mut worker);
    assert_eq!(app.input_buffer, "durable draft");
    assert_eq!(app.input_mode, InputMode::ScratchTitle);
    assert!(app.planning_store_error.is_some());
    assert!(
        app.mutation_notice
            .as_ref()
            .unwrap()
            .contains("not confirmed")
    );
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert!(
        worker
            .flush_operator_state_on_exit(&LocalStateV1::default())
            .is_err()
    );
    assert_eq!(std::fs::read(blocked).unwrap(), b"retain parent");
}
#[test]
fn stopped_worker_unblocks_pending_draft_without_claiming_failure_before_commit() {
    let root = tempfile::tempdir().unwrap();
    let mut worker = StoreWorker::at_for_test(root.path());
    let (mut app, effects) = draft();
    let ticket = app.local_edit_ticket_for(&effects[0]);
    submit(&mut app, &mut worker, ticket, |_| {
        panic!("intentional worker fault injection")
    });
    settle(&mut app, &mut worker);
    assert_eq!(app.input_buffer, "durable draft");
    assert!(app.planning_store_error.is_some());
    assert!(
        app.mutation_notice
            .as_ref()
            .unwrap()
            .contains("unconfirmed")
    );
    assert!(worker.try_event().is_none());
}
#[test]
fn buffered_editor_failure_is_retained_by_the_exit_barrier() {
    let root = tempfile::tempdir().unwrap();
    let mut worker = StoreWorker::at_for_test(root.path());
    worker
        .planning_with_ticket(
            |_| Err("accepted editor write failed".into()),
            None,
            Some(99),
        )
        .unwrap();
    let error = worker
        .flush_operator_state_on_exit(&LocalStateV1::default())
        .unwrap_err();
    assert!(error.to_string().contains("accepted editor write failed"));
}
#[test]
fn old_receipts_do_not_clear_newer_submissions() {
    let (mut app, effects) = draft();
    let old = app.local_edit_ticket_for(&effects[0]).unwrap();
    app.finish_local_edit_write(old, Err("not accepted".into()));
    let effects = reduce(&mut app, Action::CommitInput);
    let new = app.local_edit_ticket_for(&effects[0]).unwrap();
    assert_ne!(old, new);
    app.finish_local_edit_write(old, Ok(()));
    assert_eq!(app.pending_local_edit_ticket(), Some(new));
    assert_eq!(app.input_buffer, "durable draft");
    app.finish_local_edit_write(new, Ok(()));
    assert!(app.input_buffer.is_empty());
}
#[test]
fn changed_then_undone_draft_is_not_cleared_by_old_success() {
    let (mut app, effects) = draft();
    let ticket = app.local_edit_ticket_for(&effects[0]).unwrap();
    reduce(&mut app, Action::InputChar('x'));
    reduce(&mut app, Action::InputBackspace);
    app.finish_local_edit_write(ticket, Ok(()));
    assert_eq!(app.input_buffer, "durable draft");
    assert_eq!(app.input_mode, InputMode::ScratchTitle);
}
#[test]
fn cancelled_and_identically_reopened_editor_survives_old_success() {
    let (mut app, effects) = draft();
    let ticket = app.local_edit_ticket_for(&effects[0]).unwrap();
    reduce(&mut app, Action::CancelInput);
    reduce(&mut app, Action::BeginScratch);
    reduce(&mut app, Action::InputText("durable draft".into()));
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    app.finish_local_edit_write(ticket, Ok(()));
    assert_eq!(app.input_buffer, "durable draft");
    assert!(app.pending_local_edit_ticket().is_none());
    assert!(!reduce(&mut app, Action::CommitInput).is_empty());
}
#[test]
fn pending_editor_can_be_closed_without_cancelling_the_committed_write() {
    let (mut app, effects) = draft();
    let ticket = app.local_edit_ticket_for(&effects[0]).unwrap();
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    reduce(&mut app, Action::CancelInput);
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.input_buffer.is_empty());
    app.finish_local_edit_write(ticket, Err("unconfirmed".into()));
    assert!(app.input_buffer.is_empty());
    assert_eq!(app.input_mode, InputMode::Normal);
}
#[test]
fn committed_workref_receipt_refreshes_the_tui_snapshot_only_after_commit() {
    use codex_tui::workref::{WorkItem, WorkRefLedger};
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.install_workrefs(None);
    let mut ledger = WorkRefLedger::default();
    ledger
        .add_item(
            "BUG17".into(),
            WorkItem::new(
                "local-draft".into(),
                "robot".into(),
                "technical".into(),
                "BUG17".into(),
                None,
            )
            .unwrap(),
        )
        .unwrap();
    assert!(app.workref_unavailable);
    apply_event(
        &mut app,
        StoreEvent::WorkrefLinked {
            key: "BUG17".into(),
            result: Ok((ledger, "robot-app".into())),
        },
    );
    assert!(!app.workref_unavailable);
    assert!(app.workref_ledger().items.contains_key("BUG17"));
    assert!(app
        .mutation_notice
        .as_deref()
        .unwrap_or_default()
        .contains("robot-app"));

    apply_event(
        &mut app,
        StoreEvent::WorkrefLinked {
            key: "BUG17".into(),
            result: Err("SQLite workref write failed: injected".into()),
        },
    );
    assert!(app.planning_store_error.is_some());
    assert!(app.workref_unavailable);
    assert!(app.workref_ledger().items.is_empty());
}

#[test]
fn actual_store_worker_returns_receipt_only_after_existing_item_link_is_committed() {
    use codex_tui::{store::LocalStore, workref::{SessionRef, WorkItem, WorkRefLedger}};
    let root = tempfile::tempdir().unwrap();
    let sqlite = codex_tui::sqlite_store::SqliteStore::at(root.path());
    sqlite.load_config().unwrap();
    sqlite.health().unwrap();
    let mut initial = WorkRefLedger::default();
    initial
        .add_item(
            "BUG17".into(),
            WorkItem::new(
                "local-draft".into(),
                "robot".into(),
                "technical".into(),
                "BUG17".into(),
                None,
            )
            .unwrap(),
        )
        .unwrap();
    sqlite.save_workref_ledger(&initial).unwrap();
    drop(sqlite);

    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.install_workrefs(Some(initial));
    app.input_mode = InputMode::WorkrefLink;
    app.input_buffer = "BUG17".into();
    app.workref_link_pending = true;

    let mut worker = StoreWorker::at_for_test(root.path());
    worker
        .submit(|store| {
            let session = SessionRef {
                target_identity: "target-A".into(),
                codex_home: "/home/a/.codex".into(),
                thread_id: "thread-a".into(),
                repository: "/workspace/robot-app".into(),
                role: "development".into(),
            };
            let result = store.link_workref_session("BUG17", session);
            StoreEvent::WorkrefLinked {
                key: "BUG17".into(),
                result,
            }
        })
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while app.workref_link_pending {
        if let Some(event) = worker.try_event() {
            apply_event(&mut app, event);
        }
        assert!(Instant::now() < deadline, "missing workref commit receipt");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(!app.workref_unavailable);
    assert_eq!(
        app.workref_ledger().items["BUG17"].sessions["robot-app"].thread_id,
        "thread-a"
    );
    let persisted = worker.sqlite_clone().load_workref_ledger().unwrap();
    assert_eq!(persisted, app.workref_ledger());
    worker
        .flush_operator_state_on_exit(&LocalStateV1::default())
        .unwrap();
}

#[test]
fn workref_queue_admission_rejection_retains_key_then_retry_commits_once() {
    use codex_tui::{
        app::{Action, Effect, View, reduce},
        app_server_target::ResolvedAppServerTarget,
        backend::BackendStatus,
        sqlite_store::SqliteStore,
        store::LocalStore,
        workref::{SessionRef, WorkItem, WorkRefLedger},
    };
    let root = tempfile::tempdir().unwrap();
    let sqlite = SqliteStore::at(root.path());
    sqlite.load_config().unwrap();
    sqlite.health().unwrap();
    let mut ledger = WorkRefLedger::default();
    ledger
        .add_item(
            "BUG17".into(),
            WorkItem::new(
                "local-draft".into(),
                "robot".into(),
                "technical".into(),
                "BUG17".into(),
                None,
            )
            .unwrap(),
        )
        .unwrap();
    sqlite.save_workref_ledger(&ledger).unwrap();
    drop(sqlite);

    let target = ResolvedAppServerTarget::implicit_local(None);
    let mut app = AppState::new(FakeBackend::scaled(3).snapshot().threads);
    app.configure_operator_target(&target);
    app.apply_local_state(&LocalStateV1::default());
    let mut status = BackendStatus::starting(target.status_source());
    status.connected = true;
    status.codex_home = Some("/home/test/.codex".into());
    reduce(&mut app, Action::BackendStatus(status));
    app.view = View::Thread(app.threads[0].id.clone());
    app.install_workrefs(Some(ledger));

    let mut worker = StoreWorker::at_for_test(root.path());
    let (started, ready) = mpsc::sync_channel(0);
    let (release, wait) = mpsc::sync_channel(0);
    worker
        .submit(move |_| {
            started.send(()).unwrap();
            wait.recv().unwrap();
            StoreEvent::Notice(None)
        })
        .unwrap();
    ready.recv_timeout(Duration::from_secs(3)).unwrap();
    let mut queued = 0;
    while worker.submit(|_| StoreEvent::Notice(None)).is_ok() {
        queued += 1;
        assert!(queued < 128);
    }
    assert_eq!(queued, 32);

    reduce(&mut app, Action::BeginWorkrefLink);
    reduce(&mut app, Action::InputText("BUG17".into()));
    let effect = reduce(&mut app, Action::CommitInput).pop().unwrap();
    let Effect::LinkWorkrefSession { key, session } = effect else {
        panic!("expected workref link effect");
    };
    let rejected = worker.submit(move |store| {
        let result = store.link_workref_session(&key, session);
        StoreEvent::WorkrefLinked { key, result }
    });
    let error = rejected.expect_err("full queue rejects without accepting the write");
    reduce(&mut app, Action::WorkrefLinkSubmissionRejected(error));
    assert!(!app.workref_link_pending);
    assert_eq!(app.input_mode, InputMode::WorkrefLink);
    assert_eq!(app.input_buffer, "BUG17");

    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = 0;
    while seen < queued + 1 {
        if let Some(event) = worker.try_event() {
            apply_event(&mut app, event);
            seen += 1;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }

    let effect = reduce(&mut app, Action::CommitInput).pop().unwrap();
    let Effect::LinkWorkrefSession { key, session } = effect else {
        panic!("expected retried workref link effect");
    };
    worker
        .submit(move |store| {
            let result = store.link_workref_session(&key, session);
            StoreEvent::WorkrefLinked { key, result }
        })
        .unwrap();
    while app.workref_link_pending {
        if let Some(event) = worker.try_event() {
            apply_event(&mut app, event);
        }
        assert!(Instant::now() < deadline, "missing retry commit receipt");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.input_mode, InputMode::Normal);
    assert_eq!(app.workref_ledger().items["BUG17"].sessions.len(), 1);
    assert_eq!(
        worker
            .sqlite_clone()
            .load_workref_ledger()
            .unwrap()
            .items["BUG17"]
            .sessions
            .len(),
        1
    );
    worker
        .flush_operator_state_on_exit(&LocalStateV1::default())
        .unwrap();
}

#[test]
fn stopped_store_worker_marks_accepted_workref_write_as_unknown() {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.input_mode = InputMode::WorkrefLink;
    app.input_buffer = "BUG17".into();
    app.workref_link_pending = true;
    apply_event(&mut app, StoreEvent::Stopped);
    assert!(!app.workref_link_pending);
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.workref_unavailable);
    assert!(app.planning_store_error.is_some());
    assert!(app.mutation_notice.as_deref().unwrap_or_default().contains("unknown"));
}

#[test]
fn unrelated_planning_receipt_does_not_retire_local_editor() {
    let (mut app, effects) = draft();
    let ticket = app.local_edit_ticket_for(&effects[0]).unwrap();
    apply_event(
        &mut app,
        StoreEvent::Planning(Ok(PlanningSnapshot::default()), None, None),
    );
    assert_eq!(app.pending_local_edit_ticket(), Some(ticket));
    assert_eq!(app.input_buffer, "durable draft");
}

#[test]
fn buffered_save_receipt_wins_over_elapsed_wait_deadline() {
    let root = tempfile::tempdir().unwrap();
    let mut worker = StoreWorker::at_for_test(root.path());
    let (mut app, effects) = draft();
    save(&mut app, &mut worker, effects[0].clone());
    // The worker sends each receipt before accepting the next queued job.
    // This barrier guarantees the real committed save receipt is buffered.
    let (sent, ready) = mpsc::sync_channel(0);
    worker
        .submit(move |_| {
            sent.send(()).unwrap();
            StoreEvent::Notice(None)
        })
        .unwrap();
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    settle_until(&mut app, &mut worker, Instant::now());
    assert!(app.pending_local_edit_ticket().is_none());
    assert!(app.input_buffer.is_empty());
    assert_eq!(app.planning_snapshot.scratch.len(), 1);
    worker
        .flush_operator_state_on_exit(&LocalStateV1::default())
        .unwrap();
}

#[test]
fn elapsed_wait_deadline_still_rejects_a_missing_receipt() {
    let root = tempfile::tempdir().unwrap();
    let mut worker = StoreWorker::at_for_test(root.path());
    let (mut app, _effects) = draft();
    let ticket = app.pending_local_edit_ticket();
    assert!(ticket.is_some());
    // No write was submitted, so no receipt can retire this pending editor.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        settle_until(&mut app, &mut worker, Instant::now());
    }));
    let failure = result.expect_err("missing receipt must still time out");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"missing local save receipt")
    );
    assert_eq!(app.pending_local_edit_ticket(), ticket);
    assert_eq!(app.input_buffer, "durable draft");
    worker
        .flush_operator_state_on_exit(&LocalStateV1::default())
        .unwrap();
}
