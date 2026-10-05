//! Recovery inputs and failed staging must never lose committed or existing data.
use codex_tui::{
    sqlite_store::SqliteStore,
    store::{LocalStateV1, LocalStore},
};
use rusqlite::Connection;
use std::fs;

#[test]
fn restore_captures_committed_source_wal_not_only_the_main_file() {
    let source_root = tempfile::tempdir().unwrap();
    let source = SqliteStore::at(source_root.path());
    source.save_state(&LocalStateV1::default()).unwrap();
    let reader = Connection::open(source.db_path()).unwrap();
    reader
        .execute_batch("BEGIN; SELECT * FROM operator_state;")
        .unwrap();
    let mut expected = LocalStateV1::default();
    expected.pins.insert("committed-source-wal".into());
    source.save_state(&expected).unwrap();
    let destination_root = tempfile::tempdir().unwrap();
    let destination = SqliteStore::at(destination_root.path());
    destination
        .restore_recovery_backup(source.db_path())
        .unwrap();
    assert_eq!(destination.load_state().unwrap(), expected);
    assert_eq!(source.load_state().unwrap(), expected);
    reader.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn restore_rejects_live_database_alias_before_modifying_any_image() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteStore::at(root.path());
    store.save_state(&LocalStateV1::default()).unwrap();
    let before = fs::read(store.db_path()).unwrap();
    let alias = root.path().join("state/../state/state-v2.sqlite3");
    assert!(store.restore_recovery_backup(alias).is_err());
    assert_eq!(fs::read(store.db_path()).unwrap(), before);
    assert_eq!(
        fs::read_dir(store.db_path().parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn unexpected_sidecar_type_cannot_move_the_live_database() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteStore::at(root.path());
    store.save_state(&LocalStateV1::default()).unwrap();
    let backup = root.path().join("backup.sqlite3");
    store.create_recovery_backup(&backup).unwrap();
    let before = fs::read(store.db_path()).unwrap();
    let wal = store.db_path().with_file_name("state-v2.sqlite3-wal");
    fs::create_dir(&wal).unwrap();
    fs::write(wal.join("keep"), b"retained").unwrap();
    assert!(store.restore_recovery_backup(&backup).is_err());
    assert_eq!(fs::read(store.db_path()).unwrap(), before);
    assert_eq!(fs::read(wal.join("keep")).unwrap(), b"retained");
}

#[test]
fn orphan_sidecars_are_not_attached_to_a_new_restored_image() {
    let source_root = tempfile::tempdir().unwrap();
    let source = SqliteStore::at(source_root.path());
    source.save_state(&LocalStateV1::default()).unwrap();
    let backup = source_root.path().join("backup.sqlite3");
    source.create_recovery_backup(&backup).unwrap();
    let root = tempfile::tempdir().unwrap();
    let store = SqliteStore::at(root.path());
    fs::create_dir_all(store.db_path().parent().unwrap()).unwrap();
    let wal = store.db_path().with_file_name("state-v2.sqlite3-wal");
    fs::write(&wal, b"unpaired-wal-must-survive").unwrap();
    assert!(store.restore_recovery_backup(&backup).is_err());
    assert!(!store.db_path().exists());
    assert_eq!(fs::read(wal).unwrap(), b"unpaired-wal-must-survive");
}

#[test]
fn forward_operator_envelope_in_sqlite_is_not_loaded_or_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteStore::at(root.path());
    store.save_state(&LocalStateV1::default()).unwrap();
    let future = r#"{"schemaVersion":2,"pins":["future"],"newAuthority":"preserve"}"#;
    let conn = Connection::open(store.db_path()).unwrap();
    conn.execute("UPDATE operator_state SET value_json=?1", [future])
        .unwrap();
    assert!(store.load_state().is_err());
    assert!(store.save_state(&LocalStateV1::default()).is_err());
    assert!(store.health().is_err());
    let retained: String = conn
        .query_row("SELECT value_json FROM operator_state", [], |r| r.get(0))
        .unwrap();
    assert_eq!(retained, future);
}

#[test]
fn restore_rejects_forward_operator_envelope_before_preserving_live_state() {
    let root = tempfile::tempdir().unwrap();
    let live = SqliteStore::at(root.path());
    let mut state = LocalStateV1::default();
    state.pins.insert("keep-live".into());
    live.save_state(&state).unwrap();
    let backup = root.path().join("future.sqlite3");
    live.create_recovery_backup(&backup).unwrap();
    let conn = Connection::open(&backup).unwrap();
    conn.execute(
        "UPDATE operator_state SET value_json=?1",
        [r#"{"schemaVersion":2}"#],
    )
    .unwrap();
    conn.close().unwrap();
    let before = fs::read(live.db_path()).unwrap();
    assert!(live.restore_recovery_backup(&backup).is_err());
    assert_eq!(fs::read(live.db_path()).unwrap(), before);
    assert_eq!(live.load_state().unwrap(), state);
}
