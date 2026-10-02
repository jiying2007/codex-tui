use crate::batch_local::{LocalBatchAction, LocalBatchPlan};
use crate::forge_mutation::{ForgeMutationPlan, ForgeMutationReceipt};
use crate::operation::{ManagedWorktreeRecord, OperationPlan, OperationReceipt, OperationState};
use crate::planning::{
    Bookmark, HotSlot, LocalNote, PlanningSnapshot, SavedView, ScratchState, ScratchWork,
    SourceKind, SourceRef, WorkCardLink, WorkCardOverlay, WorkCardRecord,
};
use crate::sqlite_schema::{DB_SCHEMA_VERSION, configure_connection, ensure_schema};
use crate::store::{AppConfig, FileStore, LocalStateV1, LocalStore};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::NamedTempFile;

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

    pub fn config_path(&self) -> PathBuf {
        self.legacy.config_path()
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
        let conn = self.open_ready()?;
        let integrity = quick_check(&conn)?;
        let legacy_import = metadata_get(&conn, LEGACY_IMPORT_KEY)?;
        Ok(StoreHealth {
            schema_version: conn.query_row("PRAGMA user_version", [], |row| row.get(0))?,
            integrity,
            db_path: self.db_path.clone(),
            legacy_import,
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
        conn.execute_batch("PRAGMA wal_checkpoint(FULL);")
            .context("checkpoint SQLite before recovery backup")?;
        let schema_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        drop(conn);

        let temporary = NamedTempFile::new_in(parent)
            .with_context(|| format!("create temporary recovery backup in {}", parent.display()))?;
        fs::copy(&self.db_path, temporary.path()).with_context(|| {
            format!(
                "copy SQLite recovery backup {} -> {}",
                self.db_path.display(),
                temporary.path().display()
            )
        })?;
        temporary
            .as_file()
            .sync_all()
            .context("sync temporary SQLite recovery backup")?;
        validate_recovery_database(temporary.path())?;
        temporary.persist(destination).map_err(|error| {
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
        anyhow::ensure!(
            source != self.db_path,
            "recovery source must differ from the live database"
        );
        let source_schema = validate_recovery_database(source)?;
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
        fs::copy(source, temporary.path()).with_context(|| {
            format!(
                "stage SQLite recovery backup {} -> {}",
                source.display(),
                temporary.path().display()
            )
        })?;
        temporary
            .as_file()
            .sync_all()
            .context("sync staged SQLite recovery backup")?;
        validate_recovery_database(temporary.path())?;

        let previous_database = preserve_sqlite_image(&self.db_path, "pre-restore")?;

        if let Err(error) = temporary.persist(&self.db_path) {
            if let Some(previous) = previous_database.as_ref() {
                let _ = restore_preserved_sqlite_image(&self.db_path, previous);
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
                let failed = self
                    .db_path
                    .with_file_name(format!("state-v2.sqlite3.failed-restore.{}", now_unix_ms()));
                let _ = move_sqlite_image(&self.db_path, &failed);
                if let Some(previous) = previous_database.as_ref() {
                    let _ = restore_preserved_sqlite_image(&self.db_path, previous);
                }
                Err(error.context(format!(
                    "restored SQLite failed health check; failed image preserved at {}",
                    failed.display()
                )))
            }
        }
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

fn validate_recovery_database(path: &Path) -> Result<i64> {
    anyhow::ensure!(
        path.is_file(),
        "recovery database does not exist: {}",
        path.display()
    );
    let conn = Connection::open(path)
        .with_context(|| format!("open recovery SQLite {}", path.display()))?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    anyhow::ensure!(
        (1..=DB_SCHEMA_VERSION).contains(&version),
        "unsupported recovery SQLite schema version {version}"
    );
    let integrity = quick_check(&conn)?;
    anyhow::ensure!(
        integrity == "ok",
        "recovery SQLite quick_check failed: {integrity}"
    );
    Ok(version)
}

fn sqlite_sidecar_path(db_path: &Path, suffix: &str) -> Result<PathBuf> {
    let file_name = db_path
        .file_name()
        .context("SQLite database path has no file name")?;
    let parent = db_path
        .parent()
        .context("SQLite database path has no parent")?;
    let mut sidecar_name = file_name.to_os_string();
    sidecar_name.push(suffix);
    Ok(parent.join(sidecar_name))
}

fn move_sqlite_image(source: &Path, destination: &Path) -> Result<()> {
    if source.exists() {
        fs::rename(source, destination).with_context(|| {
            format!(
                "preserve SQLite database {} -> {}",
                source.display(),
                destination.display()
            )
        })?;
    }
    for suffix in ["-wal", "-shm"] {
        let source_sidecar = sqlite_sidecar_path(source, suffix)?;
        if !source_sidecar.exists() {
            continue;
        }
        let destination_sidecar = sqlite_sidecar_path(destination, suffix)?;
        fs::rename(&source_sidecar, &destination_sidecar).with_context(|| {
            format!(
                "preserve SQLite sidecar {} -> {}",
                source_sidecar.display(),
                destination_sidecar.display()
            )
        })?;
    }
    Ok(())
}

fn preserve_sqlite_image(db_path: &Path, label: &str) -> Result<Option<PathBuf>> {
    if !db_path.exists() {
        return Ok(None);
    }
    let previous = db_path.with_file_name(format!("state-v2.sqlite3.{label}.{}", now_unix_ms()));
    move_sqlite_image(db_path, &previous)?;
    Ok(Some(previous))
}

fn restore_preserved_sqlite_image(db_path: &Path, preserved: &Path) -> Result<()> {
    if db_path.exists() {
        let failed = db_path.with_file_name(format!(
            "state-v2.sqlite3.rollback-displaced.{}",
            now_unix_ms()
        ));
        move_sqlite_image(db_path, &failed)?;
    }
    move_sqlite_image(preserved, db_path)
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

fn metadata_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row("SELECT value FROM metadata WHERE key=?1", [key], |row| {
        row.get(0)
    })
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
    use crate::forge::{ForgeIdentity, ForgeProviderKind};
    use crate::forge_mutation::{ForgeMutationPlan, ForgeMutationReceipt};
    use crate::planning::{LinkRole, SavedViewLayout};
    use std::collections::{BTreeMap, BTreeSet};
    use tempfile::tempdir;

    #[test]
    fn local_batch_is_atomic_when_a_frozen_target_disappears() {
        use crate::batch_local::{LocalBatchAction, LocalBatchPlan, LocalBatchTarget};

        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());
        let scratch = store
            .create_scratch("one", None, None, None)
            .expect("scratch");
        let plan = LocalBatchPlan {
            action: LocalBatchAction::SetDone(true),
            targets: vec![
                LocalBatchTarget {
                    local_id: scratch.id.clone(),
                    anchor: SourceRef {
                        kind: SourceKind::ScratchWork,
                        value: scratch.id.clone(),
                    },
                    title: scratch.title.clone(),
                },
                LocalBatchTarget {
                    local_id: "scratch:999999".into(),
                    anchor: SourceRef {
                        kind: SourceKind::ScratchWork,
                        value: "scratch:999999".into(),
                    },
                    title: "gone".into(),
                },
            ],
            planned_at_unix_ms: 100,
        };

        assert!(store.apply_local_batch(&plan).is_err());
        let snapshot = store.load_planning_snapshot().expect("snapshot");
        assert_eq!(snapshot.scratch[0].state, ScratchState::Inbox);
    }

    #[test]
    fn local_batch_updates_scratch_authority_and_thread_overlay_in_one_api() {
        use crate::batch_local::{LocalBatchAction, LocalBatchPlan, LocalBatchTarget};

        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());
        let scratch = store
            .create_scratch("scratch", None, None, None)
            .expect("scratch");
        let scratch_plan = LocalBatchPlan {
            action: LocalBatchAction::SetPriority(7),
            targets: vec![LocalBatchTarget {
                local_id: scratch.id.clone(),
                anchor: SourceRef {
                    kind: SourceKind::ScratchWork,
                    value: scratch.id.clone(),
                },
                title: scratch.title.clone(),
            }],
            planned_at_unix_ms: 100,
        };
        store
            .apply_local_batch(&scratch_plan)
            .expect("scratch batch");

        let thread_plan = LocalBatchPlan {
            action: LocalBatchAction::AddTag("focus".into()),
            targets: vec![LocalBatchTarget {
                local_id: "thread:thread-1".into(),
                anchor: SourceRef::codex_thread(&crate::domain::ThreadId::new("thread-1")),
                title: "thread".into(),
            }],
            planned_at_unix_ms: 100,
        };
        store.apply_local_batch(&thread_plan).expect("thread batch");

        let snapshot = store.load_planning_snapshot().expect("snapshot");
        assert_eq!(snapshot.scratch[0].priority, Some(7));
        let thread = snapshot
            .cards
            .iter()
            .find(|card| card.anchor == thread_plan.targets[0].anchor)
            .expect("thread overlay");
        assert!(thread.overlay.tags.contains("focus"));
    }

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

        fs::copy(store.legacy_backup_path(), store.legacy_state_path())
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
            .create_scratch(
                "Investigate wake miss",
                Some("local only"),
                Some("kws"),
                Some(1),
            )
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
    fn local_notes_bookmarks_and_hot_slots_round_trip() {
        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());
        let thread = SourceRef {
            kind: SourceKind::CodexThread,
            value: "thread-1".into(),
        };

        store
            .upsert_note(&LocalNote {
                owner: thread.clone(),
                text: "remember this".into(),
                updated_at_unix_ms: 10,
            })
            .expect("note");
        let bookmark = store
            .create_bookmark(thread.clone(), Some("review"), Some("line 42"))
            .expect("bookmark");
        store.set_hot_slot(3, thread.clone()).expect("hot slot");

        let snapshot = store.load_planning_snapshot().expect("snapshot");
        assert_eq!(snapshot.notes.len(), 1);
        assert_eq!(snapshot.bookmarks, vec![bookmark.clone()]);
        assert_eq!(snapshot.hot_slots.len(), 1);
        assert_eq!(snapshot.hot_slots[0].slot, 3);
        assert_eq!(snapshot.hot_slots[0].target, thread);

        store
            .delete_bookmark(&bookmark.id)
            .expect("delete bookmark");
        store.clear_hot_slot(3).expect("clear hot slot");
        store
            .delete_note(&snapshot.notes[0].owner)
            .expect("delete note");
        let snapshot = store
            .load_planning_snapshot()
            .expect("snapshot after delete");
        assert!(snapshot.notes.is_empty());
        assert!(snapshot.bookmarks.is_empty());
        assert!(snapshot.hot_slots.is_empty());
    }

    #[test]
    fn schema_v1_upgrades_to_latest_without_losing_m4_state() {
        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());

        let mut state = LocalStateV1::default();
        state.pins.insert("thread-1".into());
        store.save_state(&state).expect("state");

        let mut card = WorkCardRecord::implicit_thread(&crate::domain::ThreadId::new("thread-1"));
        card.overlay.note = Some("keep me".into());
        store.upsert_work_card(&card).expect("card");

        {
            let conn = Connection::open(store.db_path()).expect("open raw database");
            conn.execute_batch(
                "DROP TABLE managed_worktrees;
                 DROP TABLE operation_receipts;
                 DROP TABLE forge_mutation_receipts;
                 PRAGMA user_version = 1;",
            )
            .expect("downgrade fixture to v1");
        }

        let health = store.health().expect("upgrade to latest");
        assert_eq!(health.schema_version, DB_SCHEMA_VERSION);
        assert_eq!(store.load_state().expect("state after upgrade"), state);
        assert_eq!(
            store.load_planning_snapshot().expect("planning").cards,
            vec![card]
        );
    }

    #[test]
    fn managed_worktree_and_unknown_receipt_survive_restart() {
        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());
        let repo = crate::domain::LocalRepoIdentity {
            git_common_dir: "/repo/.git".into(),
            primary_root: "/repo".into(),
        };
        let plan = OperationPlan::create_worktree(
            repo.clone(),
            "/repo".into(),
            "/tmp/wt-feature".into(),
            "feature".into(),
            "HEAD".into(),
            10,
        );
        let mut receipt = OperationReceipt::planned(plan.clone());
        receipt.start(11);
        receipt.outcome_unknown(12, "git process timed out".into());
        store
            .save_operation_receipt(&receipt)
            .expect("save unknown receipt");

        let managed = ManagedWorktreeRecord {
            repo: repo.clone(),
            canonical_path: "/tmp/wt-feature".into(),
            branch: Some("feature".into()),
            created_by_operation_id: plan.operation_id.clone(),
            adopted: false,
            created_at_unix_ms: 10,
            last_verified_at_unix_ms: 13,
        };
        store
            .upsert_managed_worktree(&managed)
            .expect("save managed worktree");

        let reopened = SqliteStore::at(root.path());
        assert_eq!(
            reopened
                .operation_receipt(&plan.operation_id)
                .expect("load receipt"),
            Some(receipt)
        );
        assert_eq!(
            reopened.load_managed_worktrees().expect("managed list"),
            vec![managed]
        );
    }

    #[test]
    fn recoverable_receipts_only_return_planned_executing_and_unknown() {
        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());
        let repo = crate::domain::LocalRepoIdentity {
            git_common_dir: "/repo/.git".into(),
            primary_root: "/repo".into(),
        };

        let mut planned = OperationReceipt::planned(OperationPlan::delete_branch(
            repo.clone(),
            "/repo".into(),
            "planned".into(),
            1,
        ));
        let mut executing = OperationReceipt::planned(OperationPlan::delete_branch(
            repo.clone(),
            "/repo".into(),
            "executing".into(),
            2,
        ));
        executing.start(3);
        let mut unknown = OperationReceipt::planned(OperationPlan::delete_branch(
            repo.clone(),
            "/repo".into(),
            "unknown".into(),
            4,
        ));
        unknown.start(5);
        unknown.outcome_unknown(6, "timeout".into());
        let mut succeeded = OperationReceipt::planned(OperationPlan::delete_branch(
            repo,
            "/repo".into(),
            "done".into(),
            7,
        ));
        succeeded.start(8);
        succeeded.succeed(9, "done".into(), "verified".into());

        for receipt in [&planned, &executing, &unknown, &succeeded] {
            store.save_operation_receipt(receipt).expect("save receipt");
        }

        let recoverable = store
            .load_recoverable_operation_receipts()
            .expect("recoverable");
        assert_eq!(recoverable.len(), 3);
        assert!(recoverable.iter().all(|receipt| matches!(
            receipt.state,
            OperationState::Planned | OperationState::Executing | OperationState::OutcomeUnknown
        )));

        planned.fail(10, "cancelled".into());
        store
            .save_operation_receipt(&planned)
            .expect("update planned");
        assert_eq!(
            store
                .load_recoverable_operation_receipts()
                .expect("recoverable after update")
                .len(),
            2
        );
    }

    fn forge_identity() -> ForgeIdentity {
        ForgeIdentity {
            provider: ForgeProviderKind::GitLab,
            host: "gitlab.example.com".into(),
            project_id: "42".into(),
            path_with_namespace: "team/repo".into(),
            web_url: "https://gitlab.example.com/team/repo".into(),
            default_branch: Some("main".into()),
        }
    }

    #[test]
    fn schema_v2_upgrades_to_v3_and_preserves_m5_receipts() {
        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());
        let repo = crate::domain::LocalRepoIdentity {
            git_common_dir: "/repo/.git".into(),
            primary_root: "/repo".into(),
        };
        let plan = OperationPlan::delete_branch(repo, "/repo".into(), "feature/old".into(), 1);
        let receipt = OperationReceipt::planned(plan.clone());
        store
            .save_operation_receipt(&receipt)
            .expect("save M5 receipt");

        {
            let conn = Connection::open(store.db_path()).expect("open raw database");
            conn.execute_batch(
                "DROP TABLE forge_mutation_receipts;
                 PRAGMA user_version = 2;",
            )
            .expect("downgrade fixture to v2");
        }

        let health = store.health().expect("upgrade to v3");
        assert_eq!(health.schema_version, 3);
        assert_eq!(
            store
                .operation_receipt(&plan.operation_id)
                .expect("load M5 receipt"),
            Some(receipt)
        );
        assert!(
            store
                .load_recent_forge_mutation_receipts(10)
                .expect("forge receipts")
                .is_empty()
        );
    }

    #[test]
    fn forge_mutation_receipts_round_trip_and_comment_body_is_never_persisted() {
        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());
        let secret_body = "review comment that must stay memory-only";
        let plan = ForgeMutationPlan::comment_merge_request(
            &forge_identity(),
            "/repo".into(),
            7,
            "feature/m6b".into(),
            "main".into(),
            secret_body.len(),
            10,
        )
        .expect("plan");
        let mut receipt = ForgeMutationReceipt::planned(plan.clone());
        receipt.start(11);
        receipt.outcome_unknown(12, "transport timeout".into());
        store
            .save_forge_mutation_receipt(&receipt)
            .expect("save forge receipt");

        assert_eq!(
            store
                .forge_mutation_receipt(&plan.operation_id)
                .expect("load forge receipt"),
            Some(receipt.clone())
        );

        let recoverable = store
            .load_recoverable_forge_mutation_receipts()
            .expect("recoverable forge receipts");
        assert_eq!(recoverable, vec![receipt]);

        let conn = Connection::open(store.db_path()).expect("open raw database");
        let plan_json: String = conn
            .query_row(
                "SELECT plan_json FROM forge_mutation_receipts WHERE operation_id=?1",
                [&plan.operation_id],
                |row| row.get(0),
            )
            .expect("stored plan json");
        assert!(!plan_json.contains(secret_body));
        assert!(plan_json.contains("\"payload_bytes\""));
    }

    #[test]
    fn forge_recoverable_query_excludes_terminal_receipts() {
        let root = tempdir().expect("tempdir");
        let store = SqliteStore::at(root.path());

        let planned_plan = ForgeMutationPlan::approve_merge_request(
            &forge_identity(),
            "/repo".into(),
            7,
            "feature".into(),
            "main".into(),
            1,
        )
        .expect("planned");
        let planned = ForgeMutationReceipt::planned(planned_plan);

        let succeeded_plan = ForgeMutationPlan::merge_merge_request(
            &forge_identity(),
            "/repo".into(),
            8,
            "feature-2".into(),
            "main".into(),
            2,
        )
        .expect("succeeded");
        let mut succeeded = ForgeMutationReceipt::planned(succeeded_plan);
        succeeded.start(3);
        succeeded.succeed(4, "mr:8".into(), "verified".into());

        store
            .save_forge_mutation_receipt(&planned)
            .expect("save planned");
        store
            .save_forge_mutation_receipt(&succeeded)
            .expect("save succeeded");

        let recoverable = store
            .load_recoverable_forge_mutation_receipts()
            .expect("recoverable");
        assert_eq!(recoverable.len(), 1);
        assert_eq!(recoverable[0].state, OperationState::Planned);
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
            host_local_only: true,
            repo_backed_only: true,
        };
        store.save_state(&state).expect("save");
        assert_eq!(store.load_state().expect("load"), state);
    }
}
