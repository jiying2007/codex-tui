use anyhow::{Context, Result};
use rusqlite::Connection;

pub(crate) const DB_SCHEMA_VERSION: i64 = 4;

pub(crate) fn configure_connection(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 2000;",
    )
    .context("configure SQLite connection")?;

    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .context("read SQLite journal mode")?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        let configured: String = conn
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .context("enable SQLite WAL journal mode")?;
        anyhow::ensure!(
            configured.eq_ignore_ascii_case("wal"),
            "SQLite refused WAL journal mode: {configured}"
        );
    }
    Ok(())
}

pub(crate) fn ensure_schema(conn: &mut Connection) -> Result<()> {
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    anyhow::ensure!(
        (0..=DB_SCHEMA_VERSION).contains(&version),
        "unsupported SQLite schema version {version}"
    );

    if version == 0 {
        let tx = conn
            .transaction()
            .context("begin SQLite schema v1 migration")?;
        tx.execute_batch(
            "CREATE TABLE metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );
             CREATE TABLE operator_state (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL,
                updated_at_unix_ms INTEGER NOT NULL
             );
             CREATE TABLE work_cards (
                local_id TEXT PRIMARY KEY,
                anchor_kind TEXT NOT NULL,
                anchor_ref TEXT NOT NULL,
                title_override TEXT,
                note TEXT,
                pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0,1)),
                tags_json TEXT NOT NULL DEFAULT '[]',
                priority INTEGER,
                manual_ready INTEGER NOT NULL DEFAULT 0 CHECK (manual_ready IN (0,1)),
                done_at_unix_ms INTEGER,
                snooze_until_unix_ms INTEGER,
                updated_at_unix_ms INTEGER NOT NULL,
                UNIQUE(anchor_kind, anchor_ref)
             );
             CREATE TABLE work_card_links (
                work_card_id TEXT NOT NULL REFERENCES work_cards(local_id) ON DELETE CASCADE,
                role TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                source_ref TEXT NOT NULL,
                PRIMARY KEY(work_card_id, role, source_kind, source_ref)
             );
             CREATE TABLE scratch_work (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                note TEXT,
                workspace TEXT,
                priority INTEGER,
                state TEXT NOT NULL,
                created_at_unix_ms INTEGER NOT NULL,
                updated_at_unix_ms INTEGER NOT NULL
             );
             CREATE TABLE saved_views (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                source_scope TEXT NOT NULL,
                filter TEXT NOT NULL DEFAULT '',
                group_by TEXT,
                order_by TEXT,
                layout TEXT NOT NULL,
                visible_fields_json TEXT NOT NULL DEFAULT '[]'
             );
             CREATE TABLE notes (
                owner_kind TEXT NOT NULL,
                owner_ref TEXT NOT NULL,
                text TEXT NOT NULL,
                updated_at_unix_ms INTEGER NOT NULL,
                PRIMARY KEY(owner_kind, owner_ref)
             );
             CREATE TABLE bookmarks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_kind TEXT NOT NULL,
                source_ref TEXT NOT NULL,
                label TEXT,
                note TEXT,
                created_at_unix_ms INTEGER NOT NULL
             );
             CREATE TABLE hot_slots (
                slot INTEGER PRIMARY KEY CHECK(slot BETWEEN 1 AND 9),
                target_kind TEXT NOT NULL,
                target_ref TEXT NOT NULL,
                updated_at_unix_ms INTEGER NOT NULL
             );
             PRAGMA user_version = 1;",
        )
        .context("create SQLite schema v1")?;
        tx.commit().context("commit SQLite schema v1")?;
        version = 1;
    }

    if version == 1 {
        let tx = conn
            .transaction()
            .context("begin SQLite schema v2 migration")?;
        tx.execute_batch(
            "CREATE TABLE managed_worktrees (
                repo_common_dir TEXT NOT NULL,
                repo_primary_root TEXT NOT NULL,
                canonical_path TEXT NOT NULL,
                branch TEXT,
                created_by_operation_id TEXT NOT NULL,
                adopted INTEGER NOT NULL DEFAULT 0 CHECK (adopted IN (0,1)),
                created_at_unix_ms INTEGER NOT NULL,
                last_verified_at_unix_ms INTEGER NOT NULL,
                PRIMARY KEY(repo_common_dir, canonical_path)
             );
             CREATE TABLE operation_receipts (
                operation_id TEXT PRIMARY KEY,
                plan_json TEXT NOT NULL,
                state TEXT NOT NULL,
                started_at_unix_ms INTEGER,
                completed_at_unix_ms INTEGER,
                result_ref TEXT,
                verification TEXT,
                failure TEXT,
                updated_at_unix_ms INTEGER NOT NULL
             );
             CREATE INDEX operation_receipts_state_idx
                 ON operation_receipts(state, updated_at_unix_ms DESC);
             PRAGMA user_version = 2;",
        )
        .context("upgrade SQLite schema v1 -> v2")?;
        tx.commit().context("commit SQLite schema v2")?;
        version = 2;
    }

    if version == 2 {
        let tx = conn
            .transaction()
            .context("begin SQLite schema v3 migration")?;
        tx.execute_batch(
            "CREATE TABLE forge_mutation_receipts (
                operation_id TEXT PRIMARY KEY,
                plan_json TEXT NOT NULL,
                state TEXT NOT NULL,
                started_at_unix_ms INTEGER,
                completed_at_unix_ms INTEGER,
                result_ref TEXT,
                verification TEXT,
                failure TEXT,
                updated_at_unix_ms INTEGER NOT NULL
             );
             CREATE INDEX forge_mutation_receipts_state_idx
                 ON forge_mutation_receipts(state, updated_at_unix_ms DESC);
             PRAGMA user_version = 3;",
        )
        .context("upgrade SQLite schema v2 -> v3")?;
        tx.commit().context("commit SQLite schema v3")?;
        version = 3;
    }

    if version == 3 {
        let tx = conn
            .transaction()
            .context("begin SQLite schema v4 migration")?;
        tx.execute_batch(
            "CREATE TABLE transcript_documents (
                thread_id TEXT NOT NULL,
                turn_id TEXT NOT NULL,
                item_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                title TEXT,
                text TEXT NOT NULL,
                observed_at_unix_ms INTEGER NOT NULL,
                PRIMARY KEY(thread_id, turn_id, item_id)
             );
             CREATE INDEX transcript_documents_thread_idx
                 ON transcript_documents(thread_id);
             PRAGMA user_version = 4;",
        )
        .context("upgrade SQLite schema v3 -> v4")?;
        tx.commit().context("commit SQLite schema v4")?;
    }

    // FTS is a derived acceleration layer. The bundled distribution includes FTS5,
    // but a system SQLite without the extension must not make operator state unusable.
    // Search falls back to bounded LIKE over transcript_documents when this is absent.
    let _ = conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS transcript_fts USING fts5(
            thread_id UNINDEXED,
            turn_id UNINDEXED,
            item_id UNINDEXED,
            kind UNINDEXED,
            title,
            text,
            tokenize='trigram'
         );",
    );

    Ok(())
}
