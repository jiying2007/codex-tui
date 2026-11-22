use super::{apply_event, StoreEvent, StoreWorker};
use codex_tui::{
    app::{Action, AppState, Effect, InputMode, View, reduce},
    app_server_target::ResolvedAppServerTarget,
    backend::{BackendStatus, CodexBackend, FakeBackend},
    sqlite_store::SqliteStore,
    store::{LocalStateV1, LocalStore},
    workref::{SessionRef, WorkItem, WorkRefLedger},
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

#[test]
fn committed_workref_receipt_refreshes_the_tui_snapshot_only_after_commit() {
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
    assert!(
        app.mutation_notice
            .as_deref()
            .unwrap_or_default()
            .contains("robot-app")
    );

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
    assert_eq!(persisted, app.workref_ledger().clone());
    worker
        .flush_operator_state_on_exit(&LocalStateV1::default())
        .unwrap();
}

#[test]
fn workref_queue_admission_rejection_retains_key_then_retry_commits_once() {
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
        worker.sqlite_clone().load_workref_ledger().unwrap().items["BUG17"]
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
    assert!(
        app.mutation_notice
            .as_deref()
            .unwrap_or_default()
            .contains("unknown")
    );
}
