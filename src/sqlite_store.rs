use crate::planning::{
    LinkRole, PlanningSnapshot, SavedView, SavedViewLayout, ScratchState, ScratchWork, SourceKind,
    SourceRef, WorkCardLink, WorkCardOverlay, WorkCardRecord,
};
use crate::store::{AppConfig, FileStore, LocalStateV1, LocalStore};
use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DB_SCHEMA_VERSION: i64 = 1;
const OPERATOR_STATE_KEY: &str = "operator-state-v1";
const LEGACY_IMPORT_KEY: &str = "legacy-state-v1-imported";

#[derive(Clone, Debug)]
pub struct SqliteStore {
    legacy: FileStore,
    db_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreHealth {
    pub schema_version: i64,
    pub integrity: String,
    pub db_path: PathBuf,
    pub legacy_import: Option<String>,
}

impl SqliteStore {
    pub fn discover() -> Result<Self> {
        Self::from_legacy(FileStore::discover()?)
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let legacy = FileStore::at(&root);
        Self {
            db_path: root.join("state").join("state-v2.sqlite3"),
            legacy,
        }
    }

    pub fn from_legacy(legacy: FileStore) -> Result<Self> {
        let state_path = legacy.state_path();
        let state_dir = state_path
            .parent()
            .context("legacy state path has no parent")?;
        Ok(Self {
            db_path: state_dir.join("state-v2.sqlite3"),
            legacy,
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn legacy_state_path(&self) -> PathBuf {
        self.legacy.state_path()
    }

    pub fn legacy_backup_path(&self) -> PathBuf {
        self.legacy
            .state_path()
            .with_file_name("state-v1.json.pre-sqlite-backup")
    }

    pub fn health(&self) -> Result<StoreHealth> {
        let mut conn = self.open_ready()?;
        let integrity = quick_check(&conn)?;
        let legacy_import = metadata_get(&conn, LEGACY_IMPORT_KEY)?;
        Ok(StoreHealth {
            schema_version: conn.query_row("PRAGMA user_version", [], |row| row.get(0))?,
            integrity,
            db_path: self.db_path.clone(),
            legacy_import,
        })
    }

    pub fn load_planning_snapshot(&self) -> Result<PlanningSnapshot> {
        let conn = self.open_ready()?;
        Ok(PlanningSnapshot {
            cards: load_cards(&conn)?,
            scratch: load_scratch(&conn)?,
            saved_views: load_saved_views(&conn)?,
        })
    }

    pub fn upsert_work_card(&self, card: &WorkCardRecord) -> Result<()> {
        let mut conn = self.open_ready()?;
        let tx = conn.transaction().context("begin WorkCard transaction")?;
        let tags_json = serde_json::to_string(&card.overlay.tags).context("serialize card tags")?;
        tx.execute(
            "INSERT INTO work_cards (
                local_id, anchor_kind, anchor_ref, title_override, note, pinned, tags_json,
                priority, manual_ready, done_at_unix_ms, snooze_until_unix_ms, updated_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(local_id) DO UPDATE SET
                anchor_kind=excluded.anchor_kind,
                anchor_ref=excluded.anchor_ref,
                title_override=excluded.title_override,
                note=excluded.note,
                pinned=excluded.pinned,
                tags_json=excluded.tags_json,
                priority=excluded.priority,
                manual_ready=excluded.manual_ready,
                done_at_unix_ms=excluded.done_at_unix_ms,
                snooze_until_unix_ms=excluded.snooze_until_unix_ms,
                updated_at_unix_ms=excluded.updated_at_unix_ms",
            params![
                card.local_id,
                enum_text(&card.anchor.kind)?,
                card.anchor.value,
                card.overlay.title_override,
                card.overlay.note,
                bool_i64(card.overlay.pinned),
                tags_json,
                card.overlay.priority,
                bool_i64(card.overlay.manual_ready),
                card.overlay.done_at_unix_ms.map(u64_to_i64).transpose()?,
                card.overlay.snooze_until_unix_ms.map(u64_to_i64).transpose()?,
                u64_to_i64(now_unix_ms())?,
            ],
        )
        .context("upsert WorkCard")?;

        tx.execute(
            "DELETE FROM work_card_links WHERE work_card_id = ?1",
            [&card.local_id],
        )?;
        for link in &card.links {
            tx.execute(
                "INSERT INTO work_card_links (
                    work_card_id, role, source_kind, source_ref
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    card.local_id,
                    enum_text(&link.role)?,
                    enum_text(&link.source.kind)?,
                    link.source.value,
                ],
            )
            .context("insert WorkCard link")?;
        }

        tx.commit().context("commit WorkCard transaction")
    }

    pub fn work_card_for_anchor(&self, anchor: &SourceRef) -> Result<Option<WorkCardRecord>> {
        let conn = self.open_ready()?;
        let local_id = conn
            .query_row(
                "SELECT local_id FROM work_cards WHERE anchor_kind=?1 AND anchor_ref=?2",
                params![enum_text(&anchor.kind)?, anchor.value],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .context("query WorkCard anchor")?;
        local_id
            .map(|id| load_card(&conn, &id))
            .transpose()
    }

    pub fn create_scratch(
        &self,
        title: &str,
        note: Option<&str>,
        workspace: Option<&str>,
        priority: Option<i32>,
    ) -> Result<ScratchWork> {
        let title = title.trim();
        anyhow::ensure!(!title.is_empty(), "ScratchWork title must not be empty");
        let mut conn = self.open_ready()?;
        let now = now_unix_ms();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO scratch_work (
                title, note, workspace, priority, state, created_at_unix_ms, updated_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                title,
                note,
                workspace,
                priority,
                enum_text(&ScratchState::Inbox)?,
                u64_to_i64(now)?,
            ],
        )?;
        let rowid = tx.last_insert_rowid();
        tx.commit()?;
        Ok(ScratchWork {
            id: scratch_id(rowid),
            title: title.to_string(),
            note: note.map(ToOwned::to_owned),
            workspace: workspace.map(ToOwned::to_owned),
            priority,
            state: ScratchState::Inbox,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        })
    }

    pub fn update_scratch(&self, scratch: &ScratchWork) -> Result<()> {
        let rowid = parse_prefixed_id(&scratch.id, "scratch:")?;
        let conn = self.open_ready()?;
        let changed = conn.execute(
            "UPDATE scratch_work SET
                title=?1, note=?2, workspace=?3, priority=?4, state=?5, updated_at_unix_ms=?6
             WHERE id=?7",
            params![
                scratch.title.trim(),
                scratch.note,
                scratch.workspace,
                scratch.priority,
                enum_text(&scratch.state)?,
                u64_to_i64(scratch.updated_at_unix_ms)?,
                rowid,
            ],
        )?;
        anyhow::ensure!(changed == 1, "ScratchWork does not exist: {}", scratch.id);
        Ok(())
    }

    pub fn delete_scratch(&self, scratch_id: &str) -> Result<()> {
        let rowid = parse_prefixed_id(scratch_id, "scratch:")?;
        let mut conn = self.open_ready()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM scratch_work WHERE id=?1", [rowid])?;
        tx.execute(
            "DELETE FROM work_cards WHERE anchor_kind=?1 AND anchor_ref=?2",
            params![
                enum_text(&SourceKind::ScratchWork)?,
                scratch_id,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn save_view(&self, view: &SavedView) -> Result<SavedView> {
        let conn = self.open_ready()?;
        let visible_fields =
            serde_json::to_string(&view.visible_fields).context("serialize visible fields")?;
        let existing_id = parse_optional_prefixed_id(&view.id, "view:")?;
        let rowid = if let Some(rowid) = existing_id {
            let changed = conn.execute(
                "UPDATE saved_views SET
                    name=?1, source_scope=?2, filter=?3, group_by=?4, order_by=?5,
                    layout=?6, visible_fields_json=?7
                 WHERE id=?8",
                params![
                    view.name,
                    view.source_scope,
                    view.filter,
                    view.group_by,
                    view.order_by,
                    enum_text(&view.layout)?,
                    visible_fields,
                    rowid,
                ],
            )?;
            anyhow::ensure!(changed == 1, "SavedView does not exist: {}", view.id);
            rowid
        } else {
            conn.execute(
                "INSERT INTO saved_views (
                    name, source_scope, filter, group_by, order_by, layout, visible_fields_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    view.name,
                    view.source_scope,
                    view.filter,
                    view.group_by,
                    view.order_by,
                    enum_text(&view.layout)?,
                    visible_fields,
                ],
            )?;
            conn.last_insert_rowid()
        };

        let mut saved = view.clone();
        saved.id = view_id(rowid);
        Ok(saved)
    }

    pub fn delete_view(&self, view_id: &str) -> Result<()> {
        let rowid = parse_prefixed_id(view_id, "view:")?;
        self.open_ready()?
            .execute("DELETE FROM saved_views WHERE id=?1", [rowid])?;
        Ok(())
    }

    fn open_ready(&self) -> Result<Connection> {
        ensure_private_parent(&self.db_path)?;
        let mut conn = Connection::open(&self.db_path)
            .with_context(|| format!("open SQLite store {}", self.db_path.display()))?;
        configure_connection(&conn)?;
        ensure_schema(&mut conn)?;
        self.migrate_legacy_state(&mut conn)?;
        let integrity = quick_check(&conn)?;
        anyhow::ensure!(
            integrity.eq_ignore_ascii_case("ok"),
            "SQLite integrity check failed: {integrity}"
        );
        ensure_private_file(&self.db_path)?;
        Ok(conn)
    }

    fn migrate_legacy_state(&self, conn: &mut Connection) -> Result<()> {
        if metadata_get(conn, LEGACY_IMPORT_KEY)?.is_some() {
            return Ok(());
        }

        let legacy_path = self.legacy_state_path();
        if !legacy_path.exists() {
            metadata_set(conn, LEGACY_IMPORT_KEY, "none")?;
            return Ok(());
        }

        let bytes = fs::read(&legacy_path)
            .with_context(|| format!("read legacy state {}", legacy_path.display()))?;
        let state: LocalStateV1 =
            serde_json::from_slice(&bytes).context("parse legacy state-v1.json")?;
        anyhow::ensure!(
            state.schema_version == 1,
            "unsupported legacy schemaVersion {}",
            state.schema_version
        );

        let backup = self.legacy_backup_path();
        if !backup.exists() {
            write_private_file(&backup, &bytes).context("backup legacy state-v1.json")?;
        }

        let tx = conn.transaction().context("begin legacy state migration")?;
        save_operator_state_tx(&tx, &state)?;
        metadata_set_tx(&tx, LEGACY_IMPORT_KEY, "imported")?;
        tx.commit().context("commit legacy state migration")?;

        let migrated = legacy_path.with_file_name("state-v1.json.migrated");
        if !migrated.exists() {
            fs::rename(&legacy_path, &migrated).with_context(|| {
                format!(
                    "archive migrated legacy state {} -> {}",
                    legacy_path.display(),
                    migrated.display()
                )
            })?;
        }
        Ok(())
    }
}

impl LocalStore for SqliteStore {
    fn load_config(&self) -> Result<AppConfig> {
        self.legacy.load_config()
    }

    fn load_state(&self) -> Result<LocalStateV1> {
        let conn = self.open_ready()?;
        let json = conn
            .query_row(
                "SELECT value_json FROM operator_state WHERE key=?1",
                [OPERATOR_STATE_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match json {
            Some(json) => serde_json::from_str(&json).context("decode SQLite operator state"),
            None => Ok(LocalStateV1::default()),
        }
    }

    fn save_state(&self, state: &LocalStateV1) -> Result<()> {
        let mut conn = self.open_ready()?;
        let tx = conn.transaction()?;
        save_operator_state_tx(&tx, state)?;
        tx.commit().context("commit operator state")
    }
}

fn configure_connection(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 2000;",
    )
    .context("configure SQLite connection")
}

fn ensure_schema(conn: &mut Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    anyhow::ensure!(
        version == 0 || version == DB_SCHEMA_VERSION,
        "unsupported SQLite schema version {version}"
    );
    if version == DB_SCHEMA_VERSION {
        return Ok(());
    }

    let tx = conn.transaction().context("begin SQLite schema migration")?;
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
    .context("create SQLite schema")?;
    tx.commit().context("commit SQLite schema")
}

fn save_operator_state_tx(tx: &Transaction<'_>, state: &LocalStateV1) -> Result<()> {
    let json = serde_json::to_string(state).context("serialize operator state")?;
    tx.execute(
        "INSERT INTO operator_state(key, value_json, updated_at_unix_ms)
         VALUES(?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET
            value_json=excluded.value_json,
            updated_at_unix_ms=excluded.updated_at_unix_ms",
        params![OPERATOR_STATE_KEY, json, u64_to_i64(now_unix_ms())?],
    )?;
    Ok(())
}

fn load_cards(conn: &Connection) -> Result<Vec<WorkCardRecord>> {
    let mut stmt = conn.prepare(
        "SELECT local_id, anchor_kind, anchor_ref, title_override, note, pinned,
                tags_json, priority, manual_ready, done_at_unix_ms, snooze_until_unix_ms
         FROM work_cards ORDER BY local_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<i32>>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, Option<i64>>(9)?,
            row.get::<_, Option<i64>>(10)?,
        ))
    })?;

    let mut cards = Vec::new();
    for row in rows {
        let (
            local_id,
            anchor_kind,
            anchor_ref,
            title_override,
            note,
            pinned,
            tags_json,
            priority,
            manual_ready,
            done_at,
            snooze_until,
        ) = row?;
        let mut card = WorkCardRecord {
            local_id: local_id.clone(),
            anchor: SourceRef {
                kind: enum_from_text(&anchor_kind)?,
                value: anchor_ref,
            },
            links: vec![],
            overlay: WorkCardOverlay {
                title_override,
                note,
                pinned: pinned != 0,
                tags: serde_json::from_str(&tags_json).context("decode WorkCard tags")?,
                priority,
                manual_ready: manual_ready != 0,
                done_at_unix_ms: done_at.map(i64_to_u64).transpose()?,
                snooze_until_unix_ms: snooze_until.map(i64_to_u64).transpose()?,
            },
        };
        card.links = load_links(conn, &local_id)?;
        cards.push(card);
    }
    Ok(cards)
}

fn load_card(conn: &Connection, local_id: &str) -> Result<WorkCardRecord> {
    load_cards(conn)?
        .into_iter()
        .find(|card| card.local_id == local_id)
        .with_context(|| format!("WorkCard disappeared during read: {local_id}"))
}

fn load_links(conn: &Connection, local_id: &str) -> Result<Vec<WorkCardLink>> {
    let mut stmt = conn.prepare(
        "SELECT role, source_kind, source_ref
         FROM work_card_links WHERE work_card_id=?1 ORDER BY role, source_kind, source_ref",
    )?;
    let rows = stmt.query_map([local_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (role, kind, value) = row?;
        Ok(WorkCardLink {
            role: enum_from_text(&role)?,
            source: SourceRef {
                kind: enum_from_text(&kind)?,
                value,
            },
        })
    })
    .collect()
}

fn load_scratch(conn: &Connection) -> Result<Vec<ScratchWork>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, note, workspace, priority, state,
                created_at_unix_ms, updated_at_unix_ms
         FROM scratch_work ORDER BY priority IS NULL, priority, id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<i32>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, i64>(7)?,
        ))
    })?;
    rows.map(|row| {
        let (id, title, note, workspace, priority, state, created, updated) = row?;
        Ok(ScratchWork {
            id: scratch_id(id),
            title,
            note,
            workspace,
            priority,
            state: enum_from_text(&state)?,
            created_at_unix_ms: i64_to_u64(created)?,
            updated_at_unix_ms: i64_to_u64(updated)?,
        })
    })
    .collect()
}

fn load_saved_views(conn: &Connection) -> Result<Vec<SavedView>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, source_scope, filter, group_by, order_by, layout, visible_fields_json
         FROM saved_views ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
        ))
    })?;
    rows.map(|row| {
        let (id, name, scope, filter, group, order, layout, fields) = row?;
        Ok(SavedView {
            id: view_id(id),
            name,
            source_scope: scope,
            filter,
            group_by: group,
            order_by: order,
            layout: enum_from_text(&layout)?,
            visible_fields: serde_json::from_str(&fields).context("decode SavedView fields")?,
        })
    })
    .collect()
}

fn metadata_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM metadata WHERE key=?1",
        [key],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn metadata_set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO metadata(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn metadata_set_tx(tx: &Transaction<'_>, key: &str, value: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO metadata(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn quick_check(conn: &Connection) -> Result<String> {
    conn.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .context("SQLite quick_check")
}

fn enum_text<T: Serialize>(value: &T) -> Result<String> {
    let json = serde_json::to_string(value)?;
    Ok(json.trim_matches('"').to_string())
}

fn enum_from_text<T: DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_str(&serde_json::to_string(value)?).context("decode enum text")
}

fn bool_i64(value: bool) -> i64 {
    i64::from(value)
}

fn u64_to_i64(value: u64) -> Result<i64> {
    i64::try_from(value).context("timestamp exceeds SQLite INTEGER range")
}

fn i64_to_u64(value: i64) -> Result<u64> {
    u64::try_from(value).context("negative SQLite timestamp")
}

fn parse_prefixed_id(value: &str, prefix: &str) -> Result<i64> {
    value
        .strip_prefix(prefix)
        .with_context(|| format!("invalid id prefix: {value}"))?
        .parse()
        .with_context(|| format!("invalid numeric id: {value}"))
}

fn parse_optional_prefixed_id(value: &str, prefix: &str) -> Result<Option<i64>> {
    if value.trim().is_empty() {
        Ok(None)
    } else {
        parse_prefixed_id(value, prefix).map(Some)
    }
}

fn scratch_id(rowid: i64) -> String {
    format!("scratch:{rowid}")
}

fn view_id(rowid: i64) -> String {
    format!("view:{rowid}")
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn ensure_private_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("SQLite path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)?;
    set_dir_private(parent)
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        set_dir_private(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create private file {}", path.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    ensure_private_file(path)
}

#[cfg(unix)]
fn set_dir_private(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("set private directory permissions {}", path.display()))
}

#[cfg(not(unix))]
fn set_dir_private(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn ensure_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("set private file permissions {}", path.display()))
}

#[cfg(not(unix))]
fn ensure_private_file(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ThreadUiState;
    use std::collections::{BTreeMap, BTreeSet};
    use tempfile::tempdir;

    #[test]
    fn imports_legacy_json_transactionally_and_preserves_backup() {
        let root = tempdir().expect("tempdir");
        let legacy = FileStore::at(root.path());
        let mut state = LocalStateV1::default();
        state.pins.insert("thread-1".into());
        state.aliases.insert("thread-1".into(), "primary".into());
        state.thread_ui.insert(
            "thread-1".into(),
            ThreadUiState {
                draft: "draft".into(),
                scroll: 4,
                follow: false,
            },
        );
        legacy.save_state(&state).expect("legacy state");

        let store = SqliteStore::at(root.path());
        let loaded = store.load_state().expect("migrated state");
        assert_eq!(loaded, state);
        assert!(store.db_path().exists());
        assert!(store.legacy_backup_path().exists());
        assert!(!store.legacy_state_path().exists());
        assert!(
            store
                .legacy_state_path()
                .with_file_name("state-v1.json.migrated")
                .exists()
        );

        let health = store.health().expect("health");
        assert_eq!(health.schema_version, DB_SCHEMA_VERSION);
        assert_eq!(health.integrity, "ok");
        assert_eq!(health.legacy_import.as_deref(), Some("imported"));
    }

    #[test]
    fn legacy_file_is_not_reimported_after_sqlite_changes() {
        let root = tempdir().expect("tempdir");
        let legacy = FileStore::at(root.path());
        let mut old = LocalStateV1::default();
        old.pins.insert("old".into());
        legacy.save_state(&old).expect("legacy");

        let store = SqliteStore::at(root.path());
        let mut current = store.load_state().expect("import");
        current.pins.clear();
        current.pins.insert("new".into());
        store.save_state(&current).expect("save SQLite");

        fs::copy(
            store.legacy_backup_path(),
            store.legacy_state_path(),
        )
        .expect("restore stale legacy file");

        let reloaded = store.load_state().expect("reload");
        assert_eq!(reloaded.pins, BTreeSet::from(["new".into()]));
    }

    #[test]
    fn planning_relations_round_trip_without_copying_external_state() {
        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());

        let mut card = WorkCardRecord::implicit_thread(&crate::domain::ThreadId::new("thread-1"));
        card.overlay.note = Some("local note".into());
        card.overlay.tags = BTreeSet::from(["kws".into(), "p0".into()]);
        card.links.push(WorkCardLink {
            role: LinkRole::Worktree,
            source: SourceRef {
                kind: SourceKind::Worktree,
                value: "/repo/wt".into(),
            },
        });
        store.upsert_work_card(&card).expect("card");

        let scratch = store
            .create_scratch("Investigate wake miss", Some("local only"), Some("kws"), Some(1))
            .expect("scratch");

        let view = store
            .save_view(&SavedView {
                id: String::new(),
                name: "Needs Review".into(),
                source_scope: "all".into(),
                filter: "stage:review".into(),
                group_by: Some("workspace".into()),
                order_by: Some("priority".into()),
                layout: SavedViewLayout::Board,
                visible_fields: vec!["stage".into(), "attention".into()],
            })
            .expect("view");

        let snapshot = store.load_planning_snapshot().expect("snapshot");
        assert_eq!(snapshot.cards, vec![card]);
        assert_eq!(snapshot.scratch, vec![scratch]);
        assert_eq!(snapshot.saved_views, vec![view]);
    }

    #[test]
    fn anchor_uniqueness_prevents_duplicate_active_work_cards() {
        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());
        let first = WorkCardRecord::implicit_thread(&crate::domain::ThreadId::new("thread-1"));
        store.upsert_work_card(&first).expect("first");

        let mut duplicate = first.clone();
        duplicate.local_id = "another-local-id".into();
        let error = store
            .upsert_work_card(&duplicate)
            .expect_err("duplicate anchor must fail");
        assert!(error.to_string().contains("WorkCard"));
    }

    #[test]
    fn config_remains_toml_while_runtime_state_moves_to_sqlite() {
        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());
        let config = store.load_config().expect("config");
        assert!(config.ui.mouse);
        assert!(store.legacy.config_path().exists());
        store
            .save_state(&LocalStateV1::default())
            .expect("save state");
        assert!(store.db_path().exists());
    }

    #[test]
    fn default_state_round_trips_through_sqlite() {
        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());
        let state = LocalStateV1 {
            schema_version: 1,
            thread_ui: BTreeMap::new(),
            pins: BTreeSet::new(),
            aliases: BTreeMap::new(),
            marked_unread: BTreeSet::new(),
            acknowledged_attention: BTreeSet::new(),
        };
        store.save_state(&state).expect("save");
        assert_eq!(store.load_state().expect("load"), state);
    }
}
