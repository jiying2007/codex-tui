use codex_tui::{
    domain::ThreadId,
    git,
    hardening::{FAILURE_MATRIX, QualificationState, failure_case},
    pty::{PtyEvent, PtyHandle, TerminalSize},
    sqlite_store::SqliteStore,
    store::LocalStore,
};
use rusqlite::Connection;
use std::{
    fs, thread,
    time::{Duration, Instant},
};
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
        "operator-state-truncated",
        "forward-store-schema",
        "pty-child-abnormal-exit",
        "bounded-queue-backpressure",
        "registry-churn-during-hydration",
    ] {
        assert!(
            failure_case(id).is_some(),
            "missing retained failure case {id}"
        );
    }
    assert!(
        FAILURE_MATRIX
            .iter()
            .any(|case| case.expected == QualificationState::Blocked)
    );
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
    assert!(
        result.is_err(),
        "a missing cwd must never be treated as a valid repository"
    );
}

#[test]
fn sqlite_busy_write_fails_within_bounded_deadline_without_partial_state() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let original = store.load_state().expect("initialize SQLite state");

    let locker = Connection::open(store.db_path()).expect("open writer lock");
    locker
        .execute_batch("BEGIN IMMEDIATE;")
        .expect("hold SQLite writer lock");

    let mut candidate = original.clone();
    candidate.pins.insert("must-not-partially-persist".into());
    let started = Instant::now();
    let error = store
        .save_state(&candidate)
        .expect_err("busy SQLite writer must fail closed");
    let elapsed = started.elapsed();
    // The configured SQLite busy timeout is 2s, but open_ready + the write
    // transaction can encounter the same writer lock in more than one SQLite
    // operation on some VFS/OS combinations. Keep the release contract on the
    // end-to-end fail-closed liveness bound rather than assuming one wait.
    assert!(
        elapsed <= Duration::from_secs(5),
        "busy write exceeded bounded recovery window: {elapsed:?}"
    );
    assert!(
        error.to_string().contains("locked")
            || error.to_string().contains("busy")
            || format!("{error:#}").contains("locked")
            || format!("{error:#}").contains("busy"),
        "unexpected SQLite busy error: {error:#}"
    );

    locker
        .execute_batch("ROLLBACK;")
        .expect("release writer lock");
    assert_eq!(
        store.load_state().expect("state after busy failure"),
        original,
        "busy failure must not partially persist operator state"
    );
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
    assert_eq!(
        fs::read(store.db_path()).expect("read retained db"),
        corrupt
    );
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
        error
            .to_string()
            .contains("unsupported SQLite schema version 99"),
        "unexpected forward-schema error: {error:#}"
    );

    let conn = Connection::open(store.db_path()).expect("reopen retained sqlite");
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read retained version");
    assert_eq!(version, 99, "old binary must not rewrite a newer schema");
}

fn inject_operator_envelope(store: &SqliteStore, json: &str) {
    store
        .load_state()
        .expect("initialize SQLite before raw injection");
    let conn = Connection::open(store.db_path()).expect("raw SQLite");
    conn.execute(
        "INSERT INTO operator_state(key,value_json,updated_at_unix_ms)
         VALUES('operator-state-v1',?1,0)
         ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
        [json],
    )
    .expect("inject operator envelope");
}

fn read_operator_envelope(store: &SqliteStore) -> String {
    let conn = Connection::open(store.db_path()).expect("read SQLite");
    conn.query_row(
        "SELECT value_json FROM operator_state WHERE key='operator-state-v1'",
        [],
        |row| row.get(0),
    )
    .expect("read exact operator envelope")
}

#[test]
fn truncated_operator_envelope_is_refused_without_replacement() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let truncated = r#"{"schemaVersion":1,"pins":["thread-1"]"#;
    inject_operator_envelope(&store, truncated);

    let error = store
        .load_state()
        .expect_err("truncated live SQLite operator envelope must fail closed");
    assert!(
        format!("{error:#}").contains("operator envelope"),
        "unexpected operator corruption error: {error:#}"
    );
    assert_eq!(read_operator_envelope(&store), truncated);
}

#[test]
fn forward_operator_state_schema_is_refused_without_replacement() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let future = r#"{"schemaVersion":2,"pins":["keep-me"]}"#;
    inject_operator_envelope(&store, future);

    let error = store
        .load_state()
        .expect_err("future operator envelope must be refused");
    assert!(
        format!("{error:#}").contains("unsupported LocalStore schemaVersion 2"),
        "unexpected operator schema error: {error:#}"
    );
    assert_eq!(read_operator_envelope(&store), future);
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
        assert!(
            Instant::now() < deadline,
            "PTY failure must surface within a bounded time"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
