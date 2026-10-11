use crate::batch_local::{LocalBatchAction, LocalBatchPlan};
use crate::conversation::{ConversationItemKind, ConversationPage};
use crate::forge_mutation::{ForgeMutationPlan, ForgeMutationReceipt};
use crate::operation::{ManagedWorktreeRecord, OperationPlan, OperationReceipt, OperationState};
use crate::planning::{
    Bookmark, HotSlot, LocalNote, PlanningSnapshot, SavedView, ScratchState, ScratchWork,
    SourceKind, SourceRef, WorkCardLink, WorkCardOverlay, WorkCardRecord,
};
use crate::sqlite_schema::{DB_SCHEMA_VERSION, configure_connection, ensure_schema};
use crate::store::{AppConfig, FileStore, LocalStateV1, LocalStore};
use crate::transcript_search::{
    TranscriptSearchHit, TranscriptSearchResults, TranscriptSearchSource, fts_match_query,
};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::NamedTempFile;

mod recovery;
use recovery::{preserve_sqlite_image, restore_preserved_sqlite_image, validate_recovery_database};

const OPERATOR_STATE_KEY: &str = "operator-state-v1";
const TRANSCRIPT_MAX_DOCUMENTS: i64 = 10_000;
const TRANSCRIPT_MAX_CHARS: usize = 4_096;
const TRANSCRIPT_RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1000;

// The SQLite connection maintains an OS-released cross-process exclusive
// lease; process crashes cannot strand an advisory lockfile.
pub struct LocalWriterGuard {
    _connection: Connection,
}

#[derive(Clone, Debug)]
pub struct SqliteStore {
    config_store: FileStore,
    db_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreHealth {
    pub schema_version: i64,
    pub integrity: String,
    pub db_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryBackupReceipt {
    pub path: PathBuf,
    pub schema_version: i64,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryRestoreReceipt {
    pub source: PathBuf,
    pub schema_version: i64,
    pub previous_database: Option<PathBuf>,
}

impl SqliteStore {
    pub fn discover() -> Result<Self> {
        Ok(Self::from_config_store(FileStore::discover()?))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self::from_config_store(FileStore::at(root))
    }

    fn from_config_store(config_store: FileStore) -> Self {
        Self {
            db_path: config_store.data_dir().join("state-v2.sqlite3"),
            config_store,
        }
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_store.config_path()
    }

    pub fn acquire_local_writer(&self) -> Result<LocalWriterGuard> {
        ensure_private_parent(&self.db_path)?;
        let lock_path = self.db_path.with_extension("sqlite3.owner-lock");
        refuse_unsafe_sqlite_path(&lock_path)?;
        let connection =
            Connection::open(&lock_path).context("open codex-tui local writer lock")?;
        connection.busy_timeout(std::time::Duration::ZERO)?;
        connection
            .execute_batch("PRAGMA journal_mode=DELETE; BEGIN EXCLUSIVE;")
            .context("codex-tui local state is already held by another writer")?;
        ensure_private_file(&lock_path)?;
        Ok(LocalWriterGuard {
            _connection: connection,
        })
    }

    pub fn health(&self) -> Result<StoreHealth> {
        let conn = self.open_ready()?;
        let integrity = quick_check(&conn)?;
        Ok(StoreHealth {
            schema_version: conn.query_row("PRAGMA user_version", [], |row| row.get(0))?,
            integrity,
            db_path: self.db_path.clone(),
        })
    }

    pub fn create_recovery_backup(
        &self,
        destination: impl AsRef<Path>,
    ) -> Result<RecoveryBackupReceipt> {
        let destination = destination.as_ref();
        anyhow::ensure!(
            !destination.exists(),
            "recovery backup destination already exists: {}",
            destination.display()
        );
        let parent = destination
            .parent()
            .context("recovery backup destination has no parent")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("create recovery backup directory {}", parent.display()))?;

        let conn = self.open_ready()?;
        let schema_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let temporary = NamedTempFile::new_in(parent)
            .with_context(|| format!("create temporary recovery backup in {}", parent.display()))?;
        crate::sqlite_backup::snapshot(&conn, temporary.path())?;
        drop(conn);
        temporary
            .as_file()
            .sync_all()
            .context("sync temporary SQLite recovery backup")?;
        validate_recovery_database(temporary.path())?;
        temporary.persist_noclobber(destination).map_err(|error| {
            anyhow::anyhow!(
                "persist recovery backup {}: {}",
                destination.display(),
                error.error
            )
        })?;

        Ok(RecoveryBackupReceipt {
            path: destination.to_path_buf(),
            schema_version,
            bytes: fs::metadata(destination)
                .with_context(|| format!("stat recovery backup {}", destination.display()))?
                .len(),
        })
    }

    pub fn restore_recovery_backup(
        &self,
        source: impl AsRef<Path>,
    ) -> Result<RecoveryRestoreReceipt> {
        let source = source.as_ref();
        recovery::ensure_distinct_source(source, &self.db_path)?;
        // Check alias safety without touching the store; then exclude live
        // writers before validating, preserving or installing any image.
        let _guard = self.acquire_local_writer()?;
        validate_recovery_database(source)?;
        let parent = self
            .db_path
            .parent()
            .context("SQLite database path has no parent")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("create SQLite state directory {}", parent.display()))?;

        // Disaster recovery must not open or mutate the live database before preservation.
        // A corrupt/forward-incompatible image can cause SQLite to consume or remove WAL/SHM
        // sidecars while merely probing it. Preserve the raw main+sidecar set first; the recovery
        // source is validated independently below.
        let temporary = NamedTempFile::new_in(parent)
            .with_context(|| format!("create restore staging file in {}", parent.display()))?;
        let source_connection =
            Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .context("open read-only recovery source")?;
        crate::sqlite_backup::snapshot(&source_connection, temporary.path())?;
        drop(source_connection);
        temporary
            .as_file()
            .sync_all()
            .context("sync staged SQLite recovery backup")?;
        let source_schema = validate_recovery_database(temporary.path())?;

        let previous_database = preserve_sqlite_image(&self.db_path, "pre-restore")?;

        if let Err(error) = temporary.persist_noclobber(&self.db_path) {
            if let Some(previous) = previous_database.as_ref() {
                restore_preserved_sqlite_image(&self.db_path, previous).with_context(|| {
                    format!(
                        "install failed: {}; rollback failed; preserved image: {}",
                        error.error,
                        previous.display()
                    )
                })?;
            }
            return Err(anyhow::anyhow!(
                "install recovery database {}: {}",
                self.db_path.display(),
                error.error
            ));
        }

        match self.health() {
            Ok(health) => Ok(RecoveryRestoreReceipt {
                source: source.to_path_buf(),
                schema_version: health.schema_version.max(source_schema),
                previous_database,
            }),
            Err(error) => {
                let failed = preserve_sqlite_image(&self.db_path, "failed-restore")?
                    .context("rejected restore disappeared; previous image remains retained")?;
                if let Some(previous) = previous_database.as_ref() {
                    restore_preserved_sqlite_image(&self.db_path, previous).with_context(|| {
                        format!(
                            "restore health failed: {error:#}; rollback failed; previous image: {}",
                            previous.display()
                        )
                    })?;
                }
                Err(error.context(format!(
                    "restored SQLite failed health check; failed image preserved at {}",
                    failed.display()
                )))
            }
        }
    }

    pub fn index_conversation_page(&self, page: &ConversationPage) -> Result<usize> {
        let mut conn = self.open_ready()?;
        let has_fts = transcript_fts_available(&conn)?;
        let observed_at = u64_to_i64(now_unix_ms())?;
        let bounded_title = page
            .title
            .as_deref()
            .map(|title| title.chars().take(TRANSCRIPT_MAX_CHARS).collect::<String>());
        let tx = conn
            .transaction()
            .context("begin transcript index transaction")?;
        let mut indexed = 0_usize;

        for item in &page.items {
            let kind = match item.kind {
                ConversationItemKind::User => "user",
                ConversationItemKind::Assistant => "assistant",
                _ => continue,
            };
            if item.text.trim().is_empty() {
                continue;
            }
            // Retention is from first local ingestion, not the latest reread.
            // Repeated hydration must not silently renew stored-message lifetime.
            tx.execute(
                "INSERT INTO transcript_documents (
                    thread_id, turn_id, item_id, kind, title, text, observed_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(thread_id, turn_id, item_id) DO UPDATE SET
                    kind=excluded.kind,
                    title=excluded.title,
                    text=excluded.text",
                params![
                    page.thread_id.0.as_str(),
                    item.turn_id.as_str(),
                    item.item_id.as_str(),
                    kind,
                    bounded_title.as_deref(),
                    item.text
                        .chars()
                        .take(TRANSCRIPT_MAX_CHARS)
                        .collect::<String>(),
                    observed_at,
                ],
            )
            .context("upsert transcript search document")?;

            if has_fts {
                tx.execute(
                    "DELETE FROM transcript_fts
                     WHERE thread_id=?1 AND turn_id=?2 AND item_id=?3",
                    params![
                        page.thread_id.0.as_str(),
                        item.turn_id.as_str(),
                        item.item_id.as_str()
                    ],
                )
                .context("remove prior transcript FTS row")?;
                tx.execute(
                    "INSERT INTO transcript_fts (
                        thread_id, turn_id, item_id, kind, title, text
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        page.thread_id.0.as_str(),
                        item.turn_id.as_str(),
                        item.item_id.as_str(),
                        kind,
                        bounded_title.as_deref(),
                        item.text
                            .chars()
                            .take(TRANSCRIPT_MAX_CHARS)
                            .collect::<String>(),
                    ],
                )
                .context("insert transcript FTS row")?;
            }
            indexed += 1;
        }

        prune_transcript_documents(&tx, has_fts, observed_at)?;
        tx.commit().context("commit transcript index transaction")?;
        Ok(indexed)
    }

    pub fn prune_transcript_index(&self) -> Result<()> {
        let mut conn = self.open_ready()?;
        let has_fts = transcript_fts_available(&conn)?;
        let tx = conn
            .transaction()
            .context("begin index retention cleanup")?;
        prune_transcript_documents(&tx, has_fts, u64_to_i64(now_unix_ms())?)?;
        tx.commit().context("commit index retention cleanup")
    }

    /// Clear derivative message text and FTS rows when local indexing is disabled.
    /// Logical erasure is not a guarantee of physical flash-block sanitization.
    pub fn clear_transcript_index(&self) -> Result<()> {
        let mut conn = self.open_ready()?;
        let has_fts = transcript_fts_available(&conn)?;
        let tx = conn.transaction().context("begin local index cleanup")?;
        if has_fts {
            tx.execute("DELETE FROM transcript_fts", [])
                .context("clear derived FTS rows")?;
        }
        tx.execute("DELETE FROM transcript_documents", [])
            .context("clear derived transcript rows")?;
        tx.commit().context("commit index cleanup")?;
        Ok(())
    }

    pub fn search_transcript(&self, query: &str, limit: usize) -> Result<TranscriptSearchResults> {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(TranscriptSearchResults::empty(
                query,
                TranscriptSearchSource::LocalFts,
            ));
        }
        let conn = self.open_ready()?;
        let limit = limit.min(crate::transcript_search::TRANSCRIPT_SEARCH_RESULT_LIMIT);
        let limit_i64 = i64::try_from(limit).context("transcript search limit overflow")?;
        let mut hits = Vec::new();

        if transcript_fts_available(&conn)? && query.chars().count() >= 3 {
            if let Some(match_query) = fts_match_query(query) {
                let mut stmt = conn.prepare(
                    "SELECT thread_id, turn_id, item_id,
                            snippet(transcript_fts, 5, '[', ']', ' … ', 24)
                     FROM transcript_fts
                     WHERE transcript_fts MATCH ?1
                     ORDER BY rank
                     LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![match_query, limit_i64], |row| {
                    Ok(TranscriptSearchHit {
                        thread_id: crate::domain::ThreadId::new(row.get::<_, String>(0)?),
                        turn_id: Some(row.get(1)?),
                        item_id: Some(row.get(2)?),
                        snippet: row.get(3)?,
                        turn_cursor: None,
                        match_start_utf16: None,
                        match_end_utf16: None,
                    })
                })?;
                hits = rows.collect::<rusqlite::Result<Vec<_>>>()?;
            }
        } else {
            // LIKE wildcard characters in a user query are data, not SQL syntax.
            // Escape backslash first to keep literal %, _ and \ searchable.
            let literal = query
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let pattern = format!("%{literal}%");
            let mut stmt = conn.prepare(
                "SELECT thread_id, turn_id, item_id,
                        CASE
                          WHEN length(text) <= 240 THEN text
                          ELSE substr(text, 1, 237) || '…'
                        END
                 FROM transcript_documents
                 WHERE lower(title) LIKE lower(?1) ESCAPE '\\'
                    OR lower(text) LIKE lower(?1) ESCAPE '\\'
                 ORDER BY observed_at_unix_ms DESC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![pattern, limit_i64], |row| {
                Ok(TranscriptSearchHit {
                    thread_id: crate::domain::ThreadId::new(row.get::<_, String>(0)?),
                    turn_id: Some(row.get(1)?),
                    item_id: Some(row.get(2)?),
                    snippet: row.get(3)?,
                    turn_cursor: None,
                    match_start_utf16: None,
                    match_end_utf16: None,
                })
            })?;
            hits = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        }

        Ok(TranscriptSearchResults {
            query: query.to_string(),
            source: TranscriptSearchSource::LocalFts,
            hits,
            complete: false,
        })
    }

    pub fn load_planning_snapshot(&self) -> Result<PlanningSnapshot> {
        let conn = self.open_ready()?;
        Ok(PlanningSnapshot {
            cards: load_cards(&conn)?,
            scratch: load_scratch(&conn)?,
            saved_views: load_saved_views(&conn)?,
            notes: load_notes(&conn)?,
            bookmarks: load_bookmarks(&conn)?,
            hot_slots: load_hot_slots(&conn)?,
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
                card.overlay
                    .snooze_until_unix_ms
                    .map(u64_to_i64)
                    .transpose()?,
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

    pub fn apply_local_batch(&self, plan: &LocalBatchPlan) -> Result<usize> {
        plan.validate().context("validate local batch plan")?;

        let mut conn = self.open_ready()?;
        let tx = conn
            .transaction()
            .context("begin local batch transaction")?;
        let existing = load_cards(&tx)?
            .into_iter()
            .map(|card| (card.anchor.clone(), card))
            .collect::<BTreeMap<_, _>>();
        let now = now_unix_ms();

        for target in &plan.targets {
            let mut card =
                existing
                    .get(&target.anchor)
                    .cloned()
                    .unwrap_or_else(|| WorkCardRecord {
                        local_id: target.local_id.clone(),
                        anchor: target.anchor.clone(),
                        links: vec![],
                        overlay: WorkCardOverlay::default(),
                    });
            let mut persist_card = true;

            match &plan.action {
                LocalBatchAction::AddTag(tag) => {
                    card.overlay.tags.insert(tag.trim().to_string());
                }
                LocalBatchAction::RemoveTag(tag) => {
                    card.overlay.tags.remove(tag.trim());
                }
                LocalBatchAction::SetPriority(priority)
                    if target.anchor.kind == SourceKind::ScratchWork =>
                {
                    update_scratch_priority_tx(&tx, &target.anchor.value, Some(*priority), now)?;
                    if existing.contains_key(&target.anchor) {
                        card.overlay.priority = None;
                    } else {
                        persist_card = false;
                    }
                }
                LocalBatchAction::ClearPriority
                    if target.anchor.kind == SourceKind::ScratchWork =>
                {
                    update_scratch_priority_tx(&tx, &target.anchor.value, None, now)?;
                    if existing.contains_key(&target.anchor) {
                        card.overlay.priority = None;
                    } else {
                        persist_card = false;
                    }
                }
                LocalBatchAction::SetPriority(priority) => {
                    card.overlay.priority = Some(*priority);
                }
                LocalBatchAction::ClearPriority => {
                    card.overlay.priority = None;
                }
                LocalBatchAction::SetReady(ready)
                    if target.anchor.kind == SourceKind::ScratchWork =>
                {
                    update_scratch_state_tx(
                        &tx,
                        &target.anchor.value,
                        if *ready {
                            ScratchState::Ready
                        } else {
                            ScratchState::Inbox
                        },
                        now,
                    )?;
                    if existing.contains_key(&target.anchor) {
                        card.overlay.manual_ready = false;
                    } else {
                        persist_card = false;
                    }
                }
                LocalBatchAction::SetDone(done)
                    if target.anchor.kind == SourceKind::ScratchWork =>
                {
                    update_scratch_state_tx(
                        &tx,
                        &target.anchor.value,
                        if *done {
                            ScratchState::Done
                        } else {
                            ScratchState::Ready
                        },
                        now,
                    )?;
                    if existing.contains_key(&target.anchor) {
                        card.overlay.done_at_unix_ms = None;
                    } else {
                        persist_card = false;
                    }
                }
                LocalBatchAction::SetReady(ready) => {
                    card.overlay.manual_ready = *ready;
                }
                LocalBatchAction::SetDone(done) => {
                    card.overlay.done_at_unix_ms = done.then_some(plan.planned_at_unix_ms);
                }
                LocalBatchAction::SnoozeUntil(until) => {
                    card.overlay.snooze_until_unix_ms = *until;
                }
            }

            if persist_card {
                upsert_work_card_tx(&tx, &card, now)?;
            }
        }

        tx.commit().context("commit local batch transaction")?;
        Ok(plan.targets.len())
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
        local_id.map(|id| load_card(&conn, &id)).transpose()
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
            params![enum_text(&SourceKind::ScratchWork)?, scratch_id,],
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

    pub fn upsert_note(&self, note: &LocalNote) -> Result<()> {
        let conn = self.open_ready()?;
        conn.execute(
            "INSERT INTO notes(owner_kind, owner_ref, text, updated_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(owner_kind, owner_ref) DO UPDATE SET
                text=excluded.text,
                updated_at_unix_ms=excluded.updated_at_unix_ms",
            params![
                enum_text(&note.owner.kind)?,
                note.owner.value,
                note.text,
                u64_to_i64(note.updated_at_unix_ms)?,
            ],
        )?;
        Ok(())
    }

    pub fn delete_note(&self, owner: &SourceRef) -> Result<()> {
        self.open_ready()?.execute(
            "DELETE FROM notes WHERE owner_kind=?1 AND owner_ref=?2",
            params![enum_text(&owner.kind)?, owner.value],
        )?;
        Ok(())
    }

    pub fn create_bookmark(
        &self,
        source: SourceRef,
        label: Option<&str>,
        note: Option<&str>,
    ) -> Result<Bookmark> {
        let conn = self.open_ready()?;
        let now = now_unix_ms();
        conn.execute(
            "INSERT INTO bookmarks(source_kind, source_ref, label, note, created_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                enum_text(&source.kind)?,
                source.value,
                label,
                note,
                u64_to_i64(now)?,
            ],
        )?;
        Ok(Bookmark {
            id: bookmark_id(conn.last_insert_rowid()),
            source,
            label: label.map(ToOwned::to_owned),
            note: note.map(ToOwned::to_owned),
            created_at_unix_ms: now,
        })
    }

    pub fn delete_bookmark(&self, bookmark_id: &str) -> Result<()> {
        let rowid = parse_prefixed_id(bookmark_id, "bookmark:")?;
        self.open_ready()?
            .execute("DELETE FROM bookmarks WHERE id=?1", [rowid])?;
        Ok(())
    }

    pub fn set_hot_slot(&self, slot: u8, target: SourceRef) -> Result<HotSlot> {
        anyhow::ensure!((1..=9).contains(&slot), "hot slot must be between 1 and 9");
        let conn = self.open_ready()?;
        let now = now_unix_ms();
        conn.execute(
            "INSERT INTO hot_slots(slot, target_kind, target_ref, updated_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(slot) DO UPDATE SET
                target_kind=excluded.target_kind,
                target_ref=excluded.target_ref,
                updated_at_unix_ms=excluded.updated_at_unix_ms",
            params![
                i64::from(slot),
                enum_text(&target.kind)?,
                target.value,
                u64_to_i64(now)?,
            ],
        )?;
        Ok(HotSlot {
            slot,
            target,
            updated_at_unix_ms: now,
        })
    }

    pub fn clear_hot_slot(&self, slot: u8) -> Result<()> {
        anyhow::ensure!((1..=9).contains(&slot), "hot slot must be between 1 and 9");
        self.open_ready()?
            .execute("DELETE FROM hot_slots WHERE slot=?1", [i64::from(slot)])?;
        Ok(())
    }

    pub fn upsert_managed_worktree(&self, record: &ManagedWorktreeRecord) -> Result<()> {
        let conn = self.open_ready()?;
        conn.execute(
            "INSERT INTO managed_worktrees (
                repo_common_dir, repo_primary_root, canonical_path, branch,
                created_by_operation_id, adopted, created_at_unix_ms, last_verified_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(repo_common_dir, canonical_path) DO UPDATE SET
                repo_primary_root=excluded.repo_primary_root,
                branch=excluded.branch,
                created_by_operation_id=excluded.created_by_operation_id,
                adopted=excluded.adopted,
                created_at_unix_ms=excluded.created_at_unix_ms,
                last_verified_at_unix_ms=excluded.last_verified_at_unix_ms",
            params![
                record.repo.git_common_dir,
                record.repo.primary_root,
                record.canonical_path,
                record.branch,
                record.created_by_operation_id,
                bool_i64(record.adopted),
                u64_to_i64(record.created_at_unix_ms)?,
                u64_to_i64(record.last_verified_at_unix_ms)?,
            ],
        )
        .context("upsert managed worktree")?;
        Ok(())
    }

    pub fn remove_managed_worktree(
        &self,
        repo_common_dir: &str,
        canonical_path: &str,
    ) -> Result<()> {
        self.open_ready()?.execute(
            "DELETE FROM managed_worktrees
             WHERE repo_common_dir=?1 AND canonical_path=?2",
            params![repo_common_dir, canonical_path],
        )?;
        Ok(())
    }

    pub fn load_managed_worktrees(&self) -> Result<Vec<ManagedWorktreeRecord>> {
        let conn = self.open_ready()?;
        let mut stmt = conn.prepare(
            "SELECT repo_common_dir, repo_primary_root, canonical_path, branch,
                    created_by_operation_id, adopted, created_at_unix_ms,
                    last_verified_at_unix_ms
             FROM managed_worktrees
             ORDER BY repo_common_dir, canonical_path",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (
                git_common_dir,
                primary_root,
                canonical_path,
                branch,
                created_by_operation_id,
                adopted,
                created_at,
                last_verified_at,
            ) = row?;
            Ok(ManagedWorktreeRecord {
                repo: crate::domain::LocalRepoIdentity {
                    git_common_dir,
                    primary_root,
                },
                canonical_path,
                branch,
                created_by_operation_id,
                adopted: adopted != 0,
                created_at_unix_ms: i64_to_u64(created_at)?,
                last_verified_at_unix_ms: i64_to_u64(last_verified_at)?,
            })
        })
        .collect()
    }

    pub fn managed_worktree(
        &self,
        repo_common_dir: &str,
        canonical_path: &str,
    ) -> Result<Option<ManagedWorktreeRecord>> {
        Ok(self.load_managed_worktrees()?.into_iter().find(|record| {
            record.repo.git_common_dir == repo_common_dir && record.canonical_path == canonical_path
        }))
    }

    pub fn save_operation_receipt(&self, receipt: &OperationReceipt) -> Result<()> {
        let conn = self.open_ready()?;
        let plan_json = serde_json::to_string(&receipt.plan).context("serialize operation plan")?;
        conn.execute(
            "INSERT INTO operation_receipts (
                operation_id, plan_json, state, started_at_unix_ms, completed_at_unix_ms,
                result_ref, verification, failure, updated_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(operation_id) DO UPDATE SET
                plan_json=excluded.plan_json,
                state=excluded.state,
                started_at_unix_ms=excluded.started_at_unix_ms,
                completed_at_unix_ms=excluded.completed_at_unix_ms,
                result_ref=excluded.result_ref,
                verification=excluded.verification,
                failure=excluded.failure,
                updated_at_unix_ms=excluded.updated_at_unix_ms",
            params![
                receipt.operation_id,
                plan_json,
                enum_text(&receipt.state)?,
                receipt.started_at_unix_ms.map(u64_to_i64).transpose()?,
                receipt.completed_at_unix_ms.map(u64_to_i64).transpose()?,
                receipt.result_ref,
                receipt.verification,
                receipt.failure,
                u64_to_i64(now_unix_ms())?,
            ],
        )
        .context("save operation receipt")?;
        Ok(())
    }

    pub fn operation_receipt(&self, operation_id: &str) -> Result<Option<OperationReceipt>> {
        let conn = self.open_ready()?;
        conn.query_row(
            "SELECT operation_id, plan_json, state, started_at_unix_ms,
                    completed_at_unix_ms, result_ref, verification, failure
             FROM operation_receipts WHERE operation_id=?1",
            [operation_id],
            decode_operation_receipt,
        )
        .optional()
        .context("load operation receipt")
    }

    pub fn load_recent_operation_receipts(&self, limit: usize) -> Result<Vec<OperationReceipt>> {
        let conn = self.open_ready()?;
        let limit = i64::try_from(limit).context("receipt limit exceeds SQLite range")?;
        let mut stmt = conn.prepare(
            "SELECT operation_id, plan_json, state, started_at_unix_ms,
                    completed_at_unix_ms, result_ref, verification, failure
             FROM operation_receipts
             ORDER BY updated_at_unix_ms DESC, operation_id DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], decode_operation_receipt)?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    pub fn load_recoverable_operation_receipts(&self) -> Result<Vec<OperationReceipt>> {
        let conn = self.open_ready()?;
        let mut stmt = conn.prepare(
            "SELECT operation_id, plan_json, state, started_at_unix_ms,
                    completed_at_unix_ms, result_ref, verification, failure
             FROM operation_receipts
             WHERE state IN ('planned', 'executing', 'outcome-unknown')
             ORDER BY updated_at_unix_ms, operation_id",
        )?;
        let rows = stmt.query_map([], decode_operation_receipt)?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    pub fn save_forge_mutation_receipt(&self, receipt: &ForgeMutationReceipt) -> Result<()> {
        let conn = self.open_ready()?;
        let plan_json =
            serde_json::to_string(&receipt.plan).context("serialize forge mutation plan")?;
        conn.execute(
            "INSERT INTO forge_mutation_receipts (
                operation_id, plan_json, state, started_at_unix_ms, completed_at_unix_ms,
                result_ref, verification, failure, updated_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(operation_id) DO UPDATE SET
                plan_json=excluded.plan_json,
                state=excluded.state,
                started_at_unix_ms=excluded.started_at_unix_ms,
                completed_at_unix_ms=excluded.completed_at_unix_ms,
                result_ref=excluded.result_ref,
                verification=excluded.verification,
                failure=excluded.failure,
                updated_at_unix_ms=excluded.updated_at_unix_ms",
            params![
                receipt.operation_id,
                plan_json,
                enum_text(&receipt.state)?,
                receipt.started_at_unix_ms.map(u64_to_i64).transpose()?,
                receipt.completed_at_unix_ms.map(u64_to_i64).transpose()?,
                receipt.result_ref,
                receipt.verification,
                receipt.failure,
                u64_to_i64(now_unix_ms())?,
            ],
        )
        .context("save forge mutation receipt")?;
        Ok(())
    }

    pub fn forge_mutation_receipt(
        &self,
        operation_id: &str,
    ) -> Result<Option<ForgeMutationReceipt>> {
        let conn = self.open_ready()?;
        conn.query_row(
            "SELECT operation_id, plan_json, state, started_at_unix_ms,
                    completed_at_unix_ms, result_ref, verification, failure
             FROM forge_mutation_receipts WHERE operation_id=?1",
            [operation_id],
            decode_forge_mutation_receipt,
        )
        .optional()
        .context("load forge mutation receipt")
    }

    pub fn load_recent_forge_mutation_receipts(
        &self,
        limit: usize,
    ) -> Result<Vec<ForgeMutationReceipt>> {
        let conn = self.open_ready()?;
        let limit = i64::try_from(limit).context("forge receipt limit exceeds SQLite range")?;
        let mut stmt = conn.prepare(
            "SELECT operation_id, plan_json, state, started_at_unix_ms,
                    completed_at_unix_ms, result_ref, verification, failure
             FROM forge_mutation_receipts
             ORDER BY updated_at_unix_ms DESC, operation_id DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], decode_forge_mutation_receipt)?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    pub fn load_recoverable_forge_mutation_receipts(&self) -> Result<Vec<ForgeMutationReceipt>> {
        let conn = self.open_ready()?;
        let mut stmt = conn.prepare(
            "SELECT operation_id, plan_json, state, started_at_unix_ms,
                    completed_at_unix_ms, result_ref, verification, failure
             FROM forge_mutation_receipts
             WHERE state IN ('planned', 'executing', 'outcome-unknown')
             ORDER BY updated_at_unix_ms, operation_id",
        )?;
        let rows = stmt.query_map([], decode_forge_mutation_receipt)?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    fn open_ready(&self) -> Result<Connection> {
        ensure_private_parent(&self.db_path)?;
        refuse_unsafe_sqlite_path(&self.db_path)?;
        let mut conn = Connection::open(&self.db_path)
            .with_context(|| format!("open SQLite store {}", self.db_path.display()))?;
        configure_connection(&conn)?;
        ensure_schema(&mut conn)?;
        recovery::validate_operator_envelope(&conn)?;
        // Full SQLite integrity scanning is a startup/Doctor qualification,
        // not a per-connection hot-path cost. SQLite errors still fail closed.
        ensure_private_file(&self.db_path)?;
        Ok(conn)
    }
}

impl LocalStore for SqliteStore {
    fn load_config(&self) -> Result<AppConfig> {
        self.config_store.load_config()
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

fn prune_transcript_documents(tx: &Transaction<'_>, has_fts: bool, observed_at: i64) -> Result<()> {
    let cutoff = observed_at
        .saturating_sub(i64::try_from(TRANSCRIPT_RETENTION_MS).context("retention bound")?);
    const STALE: &str = "observed_at_unix_ms < ?1 OR rowid NOT IN (
        SELECT rowid FROM transcript_documents
        ORDER BY observed_at_unix_ms DESC, rowid DESC LIMIT ?2
    )";
    if has_fts {
        tx.execute(
            &format!(
                "DELETE FROM transcript_fts WHERE (thread_id, turn_id, item_id) IN (
                    SELECT thread_id, turn_id, item_id FROM transcript_documents WHERE {STALE}
                )"
            ),
            params![cutoff, TRANSCRIPT_MAX_DOCUMENTS],
        )?;
    }
    tx.execute(
        &format!("DELETE FROM transcript_documents WHERE {STALE}"),
        params![cutoff, TRANSCRIPT_MAX_DOCUMENTS],
    )?;
    Ok(())
}

fn transcript_fts_available(conn: &Connection) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='transcript_fts'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn decode_operation_receipt(row: &rusqlite::Row<'_>) -> rusqlite::Result<OperationReceipt> {
    let operation_id: String = row.get(0)?;
    let plan_json: String = row.get(1)?;
    let state_text: String = row.get(2)?;
    let started: Option<i64> = row.get(3)?;
    let completed: Option<i64> = row.get(4)?;
    let result_ref: Option<String> = row.get(5)?;
    let verification: Option<String> = row.get(6)?;
    let failure: Option<String> = row.get(7)?;

    let plan: OperationPlan = serde_json::from_str(&plan_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let state: OperationState = enum_from_text(&state_text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, error.into())
    })?;

    Ok(OperationReceipt {
        operation_id,
        plan,
        state,
        started_at_unix_ms: started.and_then(|value| u64::try_from(value).ok()),
        completed_at_unix_ms: completed.and_then(|value| u64::try_from(value).ok()),
        result_ref,
        verification,
        failure,
    })
}

fn decode_forge_mutation_receipt(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ForgeMutationReceipt> {
    let operation_id: String = row.get(0)?;
    let plan_json: String = row.get(1)?;
    let state_text: String = row.get(2)?;
    let started: Option<i64> = row.get(3)?;
    let completed: Option<i64> = row.get(4)?;
    let result_ref: Option<String> = row.get(5)?;
    let verification: Option<String> = row.get(6)?;
    let failure: Option<String> = row.get(7)?;

    let plan: ForgeMutationPlan = serde_json::from_str(&plan_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let state: OperationState = enum_from_text(&state_text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, error.into())
    })?;

    Ok(ForgeMutationReceipt {
        operation_id,
        plan,
        state,
        started_at_unix_ms: started.and_then(|value| u64::try_from(value).ok()),
        completed_at_unix_ms: completed.and_then(|value| u64::try_from(value).ok()),
        result_ref,
        verification,
        failure,
    })
}

fn upsert_work_card_tx(tx: &Transaction<'_>, card: &WorkCardRecord, now: u64) -> Result<()> {
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
            card.overlay
                .snooze_until_unix_ms
                .map(u64_to_i64)
                .transpose()?,
            u64_to_i64(now)?,
        ],
    )
    .context("upsert batch WorkCard")?;

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
        .context("insert batch WorkCard link")?;
    }
    Ok(())
}

fn update_scratch_priority_tx(
    tx: &Transaction<'_>,
    scratch_id: &str,
    priority: Option<i32>,
    now: u64,
) -> Result<()> {
    let rowid = parse_prefixed_id(scratch_id, "scratch:")?;
    let changed = tx.execute(
        "UPDATE scratch_work SET priority=?1, updated_at_unix_ms=?2 WHERE id=?3",
        params![priority, u64_to_i64(now)?, rowid],
    )?;
    anyhow::ensure!(changed == 1, "ScratchWork does not exist: {scratch_id}");
    Ok(())
}

fn update_scratch_state_tx(
    tx: &Transaction<'_>,
    scratch_id: &str,
    state: ScratchState,
    now: u64,
) -> Result<()> {
    let rowid = parse_prefixed_id(scratch_id, "scratch:")?;
    let changed = tx.execute(
        "UPDATE scratch_work SET state=?1, updated_at_unix_ms=?2 WHERE id=?3",
        params![enum_text(&state)?, u64_to_i64(now)?, rowid],
    )?;
    anyhow::ensure!(changed == 1, "ScratchWork does not exist: {scratch_id}");
    Ok(())
}

fn save_operator_state_tx(tx: &Transaction<'_>, state: &LocalStateV1) -> Result<()> {
    anyhow::ensure!(
        state.schema_version == 1,
        "unsupported LocalStore schemaVersion {}",
        state.schema_version
    );
    recovery::validate_operator_envelope(tx)?;
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

fn load_notes(conn: &Connection) -> Result<Vec<LocalNote>> {
    let mut stmt = conn.prepare(
        "SELECT owner_kind, owner_ref, text, updated_at_unix_ms
         FROM notes ORDER BY owner_kind, owner_ref",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    rows.map(|row| {
        let (kind, value, text, updated_at) = row?;
        Ok(LocalNote {
            owner: SourceRef {
                kind: enum_from_text(&kind)?,
                value,
            },
            text,
            updated_at_unix_ms: i64_to_u64(updated_at)?,
        })
    })
    .collect()
}

fn load_bookmarks(conn: &Connection) -> Result<Vec<Bookmark>> {
    let mut stmt = conn.prepare(
        "SELECT id, source_kind, source_ref, label, note, created_at_unix_ms
         FROM bookmarks ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, i64>(5)?,
        ))
    })?;
    rows.map(|row| {
        let (id, kind, value, label, note, created_at) = row?;
        Ok(Bookmark {
            id: bookmark_id(id),
            source: SourceRef {
                kind: enum_from_text(&kind)?,
                value,
            },
            label,
            note,
            created_at_unix_ms: i64_to_u64(created_at)?,
        })
    })
    .collect()
}

fn load_hot_slots(conn: &Connection) -> Result<Vec<HotSlot>> {
    let mut stmt = conn.prepare(
        "SELECT slot, target_kind, target_ref, updated_at_unix_ms
         FROM hot_slots ORDER BY slot",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    rows.map(|row| {
        let (slot, kind, value, updated_at) = row?;
        let slot = u8::try_from(slot).context("hot slot outside u8 range")?;
        Ok(HotSlot {
            slot,
            target: SourceRef {
                kind: enum_from_text(&kind)?,
                value,
            },
            updated_at_unix_ms: i64_to_u64(updated_at)?,
        })
    })
    .collect()
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

fn bookmark_id(rowid: i64) -> String {
    format!("bookmark:{rowid}")
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

// The data directory is private, but even a first deployment must not
// follow a pre-existing symlink or open a non-regular database/owner-lock.
// SQLite and the post-open chmod would otherwise operate on its target.
fn refuse_unsafe_sqlite_path(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.file_type().is_file(),
                "SQLite state path must be a regular file (no symlinks): {}",
                path.display()
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect SQLite state path {}", path.display()));
        }
    }
    Ok(())
}

fn ensure_private_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("SQLite path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)?;
    set_dir_private(parent)
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
mod tests;
