use codex_tui::{
    domain::ThreadId,
    planning::{LocalNote, SavedView, SavedViewLayout, SourceKind, SourceRef, WorkCardRecord},
    sqlite_store::SqliteStore,
    store::{FileStore, LocalStateV1, LocalStore},
};
use rusqlite::Connection;
use std::fs;
use tempfile::tempdir;

fn v1_operator_fixture() -> LocalStateV1 {
    serde_json::from_str(include_str!("fixtures/v1.0/state-v1.json"))
        .expect("v1.0 operator fixture")
}

fn install_v1_operator_fixture(root: &std::path::Path) -> LocalStateV1 {
    let expected = v1_operator_fixture();
    let legacy = FileStore::at(root);
    fs::create_dir_all(legacy.state_path().parent().expect("state parent"))
        .expect("create state dir");
    fs::write(
        legacy.state_path(),
        include_str!("fixtures/v1.0/state-v1.json"),
    )
    .expect("install v1 operator fixture");
    expected
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
fn v1_operator_and_sqlite_schema_upgrade_preserve_user_state_idempotently() {
    let root = tempdir().expect("tempdir");
    let expected_operator = install_v1_operator_fixture(root.path());
    let store = SqliteStore::at(root.path());

    assert_eq!(
        store.load_state().expect("import v1 operator state"),
        expected_operator
    );
    populate_planning(&store);
    let expected_planning = store
        .load_planning_snapshot()
        .expect("planning before downgrade");

    {
        let conn = Connection::open(store.db_path()).expect("raw sqlite");
        conn.execute_batch(
            "DROP TABLE managed_worktrees;
             DROP TABLE operation_receipts;
             DROP TABLE forge_mutation_receipts;
             PRAGMA user_version = 1;",
        )
        .expect("create retained schema-v1 fixture");
    }

    let reopened = SqliteStore::at(root.path());
    let health = reopened.health().expect("migrate schema 1 -> latest");
    assert_eq!(health.schema_version, 3);
    assert_eq!(health.integrity, "ok");
    assert_eq!(
        reopened.load_state().expect("operator after migration"),
        expected_operator
    );
    assert_eq!(
        reopened
            .load_planning_snapshot()
            .expect("planning after migration"),
        expected_planning
    );

    let reopened_again = SqliteStore::at(root.path());
    assert_eq!(
        reopened_again
            .health()
            .expect("idempotent reopen")
            .schema_version,
        3
    );
    assert_eq!(
        reopened_again.load_state().expect("idempotent operator"),
        expected_operator
    );
    assert_eq!(
        reopened_again
            .load_planning_snapshot()
            .expect("idempotent planning"),
        expected_planning
    );
}

#[test]
fn backup_restore_round_trip_preserves_operator_and_planning_state() {
    let root = tempdir().expect("tempdir");
    let expected_operator = install_v1_operator_fixture(root.path());
    let store = SqliteStore::at(root.path());
    assert_eq!(
        store.load_state().expect("import operator"),
        expected_operator
    );
    populate_planning(&store);
    let expected_planning = store
        .load_planning_snapshot()
        .expect("planning before backup");

    let backup = root.path().join("recovery").join("pre-upgrade.sqlite3");
    let receipt = store
        .create_recovery_backup(&backup)
        .expect("create recovery backup");
    assert_eq!(receipt.schema_version, 3);
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
    assert_eq!(restore.schema_version, 3);
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
    let expected_operator = install_v1_operator_fixture(root.path());
    let store = SqliteStore::at(root.path());
    assert_eq!(
        store.load_state().expect("import operator"),
        expected_operator
    );

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
    let expected_operator = install_v1_operator_fixture(root.path());
    let store = SqliteStore::at(root.path());
    assert_eq!(
        store.load_state().expect("import operator"),
        expected_operator
    );
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
        let sidecar = std::path::PathBuf::from(format!(
            "{}{suffix}",
            store.db_path().to_string_lossy()
        ));
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
        let sidecar = std::path::PathBuf::from(format!(
            "{}{suffix}",
            previous.to_string_lossy()
        ));
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
