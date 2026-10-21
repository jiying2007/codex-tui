//! Doctor's process exit and read-only admission for a non-ok SQLite result row.
use std::process::{Command, Output, Stdio};

fn doctor(root: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codex-tui"))
        .args(["doctor", "store"])
        .current_dir(root)
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("APPDATA", root.join("AppData/Roaming"))
        .env("LOCALAPPDATA", root.join("AppData/Local"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("CODEX_HOME", root.join("codex"))
        .env("PATH", root)
        .stdin(Stdio::null())
        .output()
        .expect("run doctor store")
}

#[test]
fn doctor_rejects_non_ok_integrity_without_initializing_derived_state() {
    // READ_ONLY SQLite handles omit CHECK expressions. A persistent NOT NULL
    // inconsistency yields the non-ok result row this process test requires.
    assert_doctor_rejects(
        "CREATE TABLE health_sentinel(value TEXT);
         INSERT INTO health_sentinel VALUES(NULL);
         PRAGMA writable_schema=ON;
         UPDATE sqlite_schema SET sql='CREATE TABLE health_sentinel(value TEXT NOT NULL)'
             WHERE name='health_sentinel';
         PRAGMA writable_schema=OFF;",
        "NULL value in health_sentinel.value",
    );
}

#[test]
fn doctor_rejects_application_check_violation_before_initializing_derived_state() {
    assert_doctor_rejects(
        "PRAGMA ignore_check_constraints=ON;
         INSERT INTO hot_slots VALUES(10, 'codex-thread', 'health-sentinel', 1);
         PRAGMA ignore_check_constraints=OFF;",
        "hot slot must be between 1 and 9",
    );
}

fn assert_doctor_rejects(corruption: &str, expected: &str) {
    let root = tempfile::tempdir().unwrap();
    #[cfg(windows)]
    for directory in ["AppData/Roaming", "AppData/Local"] {
        std::fs::create_dir_all(root.path().join(directory)).unwrap();
    }
    let healthy = doctor(root.path());
    assert!(healthy.status.success(), "{healthy:?}");
    let stdout = String::from_utf8(healthy.stdout).unwrap();
    let db = stdout
        .lines()
        .find_map(|line| line.strip_prefix("state: "))
        .unwrap();
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=DELETE;
         DROP TABLE transcript_fts;",
    )
    .unwrap();
    conn.execute_batch(corruption).unwrap();
    drop(conn);
    let original = std::fs::read(db).unwrap();
    let failed = doctor(root.path());
    assert_eq!(failed.status.code(), Some(3), "{failed:?}");
    let stdout = String::from_utf8(failed.stdout).unwrap();
    assert!(stdout.contains("integrity: DEGRADED"));
    assert!(stdout.contains(expected), "{stdout}");
    assert_eq!(std::fs::read(db).unwrap(), original);
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='transcript_fts'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0,
    );
    assert_eq!(
        conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "delete",
    );
}
