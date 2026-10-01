use codex_tui::{
    domain::ThreadId,
    git,
    hardening::{FAILURE_MATRIX, QualificationState, failure_case},
    pty::{PtyEvent, PtyHandle, TerminalSize},
    sqlite_store::SqliteStore,
    store::{FileStore, LocalStore},
};
use rusqlite::Connection;
use std::{fs, thread, time::{Duration, Instant}};
use tempfile::tempdir;

#[test]
fn retained_failure_matrix_contains_the_release_blocking_cases() {
    for id in [
        "app-server-exit-during-hydration",
        "rpc-invalid-json",
        "rpc-response-timeout",
        "git-cwd-disappears",
        "forge-unauthenticated",
        "forge-command-timeout",
        "sqlite-busy-or-write-failure",
        "sqlite-corrupt",
        "legacy-state-truncated",
        "forward-store-schema",
        "pty-child-abnormal-exit",
        "bounded-queue-backpressure",
        "registry-churn-during-hydration",
    ] {
        assert!(failure_case(id).is_some(), "missing retained failure case {id}");
    }
    assert!(FAILURE_MATRIX.iter().any(|case| case.expected == QualificationState::Blocked));
}

#[tokio::test]
async fn missing_git_cwd_fails_closed_within_the_probe_deadline() {
    let root = tempdir().expect("tempdir");
    let missing = root.path().join("deleted-during-probe");
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        git::probe_context(
            ThreadId::new("failure-matrix-git"),
            missing.to_string_lossy().into_owned(),
        ),
    )
    .await
    .expect("git probe must have a finite deadline");
    assert!(result.is_err(), "a missing cwd must never be treated as a valid repository");
}

#[test]
fn corrupt_sqlite_is_not_reinitialized_or_overwritten() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    fs::create_dir_all(store.db_path().parent().expect("db parent")).expect("create db dir");
    let corrupt = b"not-a-sqlite-database\nretained-user-state";
    fs::write(store.db_path(), corrupt).expect("write corrupt fixture");

    let error = store.health().expect_err("corrupt DB must block");
    assert!(
        error.to_string().contains("SQLite") || error.to_string().contains("database"),
        "unexpected corruption error: {error:#}"
    );
    assert_eq!(fs::read(store.db_path()).expect("read retained db"), corrupt);
}

#[test]
fn forward_sqlite_schema_is_refused_without_downgrade() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    fs::create_dir_all(store.db_path().parent().expect("db parent")).expect("create db dir");
    let conn = Connection::open(store.db_path()).expect("create sqlite fixture");
    conn.pragma_update(None, "user_version", 99_i64)
        .expect("set forward schema");
    drop(conn);

    let error = store.health().expect_err("forward schema must be refused");
    assert!(
        error.to_string().contains("unsupported SQLite schema version 99"),
        "unexpected forward-schema error: {error:#}"
    );

    let conn = Connection::open(store.db_path()).expect("reopen retained sqlite");
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read retained version");
    assert_eq!(version, 99, "old binary must not rewrite a newer schema");
}

#[test]
fn truncated_legacy_state_is_preserved_for_recovery() {
    let root = tempdir().expect("tempdir");
    let legacy = FileStore::at(root.path());
    fs::create_dir_all(
        legacy
            .state_path()
            .parent()
            .expect("legacy state parent"),
    )
    .expect("create state dir");
    let truncated = br#"{"schemaVersion":1,"pins":["thread-1"]"#;
    fs::write(legacy.state_path(), truncated).expect("write truncated fixture");

    let store = SqliteStore::at(root.path());
    let error = store.load_state().expect_err("truncated legacy state must block migration");
    assert!(
        error.to_string().contains("legacy") || error.to_string().contains("state-v1"),
        "unexpected legacy error: {error:#}"
    );
    assert_eq!(
        fs::read(store.legacy_state_path()).expect("read retained legacy state"),
        truncated
    );
    assert!(
        !store.legacy_backup_path().exists(),
        "a backup must only be declared valid after the legacy document parses"
    );
}

#[test]
fn forward_operator_state_schema_is_refused_without_replacement() {
    let root = tempdir().expect("tempdir");
    let store = FileStore::at(root.path());
    fs::create_dir_all(store.state_path().parent().expect("state parent"))
        .expect("create state dir");
    let future = br#"{"schemaVersion":2,"pins":["keep-me"]}"#;
    fs::write(store.state_path(), future).expect("write forward operator fixture");

    let error = store.load_state().expect_err("forward operator schema must be refused");
    assert!(
        error.to_string().contains("unsupported LocalStore schemaVersion 2"),
        "unexpected operator schema error: {error:#}"
    );
    assert_eq!(fs::read(store.state_path()).expect("read retained state"), future);
}

#[test]
fn invalid_pty_cwd_becomes_an_error_event_without_blocking_the_caller() {
    let root = tempdir().expect("tempdir");
    let missing = root.path().join("gone");
    let handle = match PtyHandle::start(missing, TerminalSize { rows: 24, cols: 80 }) {
        Ok(handle) => handle,
        Err(_) => return,
    };

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(event) = handle.try_recv() {
            match event {
                PtyEvent::Error(message) => {
                    assert!(
                        message.contains("resolve PTY cwd")
                            || message.contains("directory")
                            || message.contains("cwd"),
                        "unexpected PTY error: {message}"
                    );
                    break;
                }
                PtyEvent::Exited { .. } | PtyEvent::ReaderClosed => break,
                PtyEvent::Ready { .. } | PtyEvent::Output(_) => {}
            }
        }
        assert!(Instant::now() < deadline, "PTY failure must surface within a bounded time");
        thread::sleep(Duration::from_millis(10));
    }
}
