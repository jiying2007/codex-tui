use codex_tui::{
    domain::{ThreadId, ThreadUiState},
    planning::{LocalNote, SavedView, SavedViewLayout, SourceKind, SourceRef, WorkCardRecord},
    sqlite_store::SqliteStore,
    store::{LocalStateV1, LocalStore},
};
use rusqlite::Connection;
use std::fs;
use tempfile::tempdir;

fn seed_operator(store: &SqliteStore) -> LocalStateV1 {
    let mut state = LocalStateV1::default();
    state.pins.insert("thread-1".into());
    state.aliases.insert("thread-1".into(), "primary".into());
    state.thread_ui.insert(
        "thread-1".into(),
        ThreadUiState {
            draft: "fresh first-deployment draft".into(),
            scroll: 7,
            follow: false,
        },
    );
    store.save_state(&state).expect("seed fresh SQLite state");
    state
}

fn populate_planning(store: &SqliteStore) {
    let mut card = WorkCardRecord::implicit_thread(&ThreadId::new("thread-1"));
    card.overlay.note = Some("retain work-card note".into());
    card.overlay.tags.insert("qualification".into());
    card.overlay.priority = Some(1);
    store.upsert_work_card(&card).expect("work card");

    store
        .create_scratch(
            "Retain scratch",
            Some("scratch note"),
            Some("qualification"),
            Some(1),
        )
        .expect("scratch");

    store
        .save_view(&SavedView {
            id: String::new(),
            name: "Retained View".into(),
            source_scope: "all".into(),
            filter: "tag:qualification".into(),
            group_by: Some("workspace".into()),
            order_by: Some("priority".into()),
            layout: SavedViewLayout::Board,
            visible_fields: vec!["stage".into(), "attention".into()],
        })
        .expect("saved view");

    let owner = SourceRef {
        kind: SourceKind::CodexThread,
        value: "thread-1".into(),
    };
    store
        .upsert_note(&LocalNote {
            owner: owner.clone(),
            text: "retain local note".into(),
            updated_at_unix_ms: 42,
        })
        .expect("local note");
    store.set_hot_slot(7, owner).expect("hot slot");
}

#[test]
fn first_install_and_reopen_preserve_sqlite_operator_and_planning_state() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    assert_eq!(
        store.load_state().expect("fresh state"),
        LocalStateV1::default()
    );
    let expected_operator = seed_operator(&store);
    populate_planning(&store);
    let expected_planning = store
        .load_planning_snapshot()
        .expect("planning after first install");

    let health = store.health().expect("fresh SQLite schema");
    assert_eq!(health.schema_version, 4);
    assert_eq!(health.integrity, "ok");
    {
        let conn = Connection::open(store.db_path()).expect("inspect fresh schema");
        let transcript_tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE name IN ('transcript_documents', 'transcript_fts')",
                [],
                |row| row.get(0),
            )
            .expect("count transcript tables");
        assert_eq!(transcript_tables, 2);
    }

    let reopened = SqliteStore::at(root.path());
    assert_eq!(
        reopened.load_state().expect("state after reopen"),
        expected_operator
    );
    assert_eq!(
        reopened
            .load_planning_snapshot()
            .expect("planning after reopen"),
        expected_planning
    );
    assert_eq!(
        reopened.health().expect("idempotent reopen").schema_version,
        4
    );
}

#[test]
fn backup_restore_round_trip_preserves_operator_and_planning_state() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let expected_operator = seed_operator(&store);
    populate_planning(&store);
    let expected_planning = store
        .load_planning_snapshot()
        .expect("planning before backup");

    let backup = root.path().join("recovery").join("pre-upgrade.sqlite3");
    let receipt = store
        .create_recovery_backup(&backup)
        .expect("create recovery backup");
    assert_eq!(receipt.schema_version, 4);
    assert!(receipt.bytes > 0);
    assert!(backup.is_file());

    let mut mutated = LocalStateV1::default();
    mutated.pins.insert("post-backup-mutation".into());
    store.save_state(&mutated).expect("mutate operator state");
    let scratch_id = store.load_planning_snapshot().expect("planning").scratch[0]
        .id
        .clone();
    store.delete_scratch(&scratch_id).expect("mutate scratch");
    store.clear_hot_slot(7).expect("mutate hot slot");

    let restore = store
        .restore_recovery_backup(&backup)
        .expect("restore recovery backup");
    assert_eq!(restore.schema_version, 4);
    let previous = restore
        .previous_database
        .expect("pre-restore database retained");
    assert!(previous.is_file());

    assert_eq!(
        store.load_state().expect("restored operator"),
        expected_operator
    );
    assert_eq!(
        store.load_planning_snapshot().expect("restored planning"),
        expected_planning
    );

    let reopened = SqliteStore::at(root.path());
    assert_eq!(
        reopened.load_state().expect("reopen operator"),
        expected_operator
    );
    assert_eq!(
        reopened.load_planning_snapshot().expect("reopen planning"),
        expected_planning
    );
}

#[test]
fn corrupt_restore_source_is_refused_without_touching_live_state() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let expected_operator = seed_operator(&store);

    let corrupt = root.path().join("corrupt.sqlite3");
    fs::write(&corrupt, b"not a database").expect("corrupt fixture");
    let error = store
        .restore_recovery_backup(&corrupt)
        .expect_err("corrupt recovery source must fail");
    assert!(
        error.to_string().contains("SQLite")
            || error.to_string().contains("database")
            || error.to_string().contains("recovery")
    );
    assert_eq!(
        store.load_state().expect("live state retained"),
        expected_operator
    );
}

#[test]
fn validated_backup_restores_over_corrupt_live_database_and_preserves_raw_image() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let expected_operator = seed_operator(&store);
    populate_planning(&store);
    let expected_planning = store
        .load_planning_snapshot()
        .expect("planning before backup");

    let backup = root.path().join("recovery").join("known-good.sqlite3");
    store
        .create_recovery_backup(&backup)
        .expect("create known-good backup");

    let corrupt_bytes = b"corrupt-live-database-retained-for-recovery";
    fs::write(store.db_path(), corrupt_bytes).expect("corrupt live database");
    for suffix in ["-wal", "-shm"] {
        let sidecar =
            std::path::PathBuf::from(format!("{}{suffix}", store.db_path().to_string_lossy()));
        fs::write(&sidecar, format!("retained{suffix}")).expect("write retained sidecar");
    }

    let restore = store
        .restore_recovery_backup(&backup)
        .expect("validated backup must restore over corrupt live state");
    let previous = restore
        .previous_database
        .expect("corrupt live database must be preserved");
    assert_eq!(
        fs::read(&previous).expect("read preserved corrupt database"),
        corrupt_bytes
    );
    for suffix in ["-wal", "-shm"] {
        let sidecar = std::path::PathBuf::from(format!("{}{suffix}", previous.to_string_lossy()));
        assert!(
            sidecar.is_file(),
            "pre-restore {suffix} sidecar must be preserved"
        );
    }

    assert_eq!(
        store.load_state().expect("restored operator"),
        expected_operator
    );
    assert_eq!(
        store.load_planning_snapshot().expect("restored planning"),
        expected_planning
    );
}

#[test]
fn online_backup_includes_committed_wal_while_an_old_reader_pins_checkpoint() {
    let root = tempdir().unwrap();
    let store = SqliteStore::at(root.path());
    store.save_state(&LocalStateV1::default()).unwrap();
    let reader = Connection::open(store.db_path()).unwrap();
    reader
        .execute_batch("BEGIN; SELECT * FROM operator_state;")
        .unwrap();
    let mut expected = LocalStateV1::default();
    expected.pins.insert("committed-in-wal".into());
    store.save_state(&expected).unwrap();
    let backup = root.path().join("online.sqlite3");
    store.create_recovery_backup(&backup).unwrap();
    reader.execute_batch("ROLLBACK").unwrap();
    // Backup is online; restore is offline and must not rename an open database.
    reader.close().unwrap();
    store.save_state(&LocalStateV1::default()).unwrap();
    store.restore_recovery_backup(&backup).unwrap();
    assert_eq!(store.load_state().unwrap(), expected);
    assert!(!root.path().join("online.sqlite3-wal").exists());
}

#[test]
fn backup_never_overwrites_an_existing_destination() {
    let root = tempdir().unwrap();
    let store = SqliteStore::at(root.path());
    store.save_state(&LocalStateV1::default()).unwrap();
    let path = root.path().join("keep-me.sqlite3");
    fs::write(&path, b"preserve this file").unwrap();
    assert!(store.create_recovery_backup(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"preserve this file");
}
