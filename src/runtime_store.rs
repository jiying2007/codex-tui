use anyhow::{Context, Result};
use codex_tui::{
    conversation::ConversationPage,
    planning::{
        LocalNote, PlanningSnapshot, SavedView, ScratchState, SourceKind, SourceRef, WorkCardRecord,
    },
    saved_view_editor::validate_saved_view,
    sqlite_store::{LocalWriterGuard, SqliteStore},
    store::{AppConfig, LocalStateV1, LocalStore},
    transcript_search::TranscriptSearchResults,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct RuntimeStore {
    sqlite: SqliteStore,
    _writer_guard: Option<LocalWriterGuard>,
    writable: bool,
    error: Option<String>,
}

pub(crate) struct StoreBootstrap {
    pub(crate) config: AppConfig,
    pub(crate) local: LocalStateV1,
    pub(crate) planning: PlanningSnapshot,
}

impl RuntimeStore {
    #[cfg(test)]
    pub(crate) fn at_for_test(root: &std::path::Path) -> Self {
        Self {
            sqlite: SqliteStore::at(root),
            _writer_guard: None,
            writable: true,
            error: None,
        }
    }

    pub(crate) fn discover() -> Result<(Self, StoreBootstrap)> {
        let sqlite = SqliteStore::discover()?;
        let config = sqlite.load_config()?;
        let guard = sqlite.acquire_local_writer()?;

        let mut error = None;
        let local = match sqlite.load_state() {
            Ok(state) => state,
            Err(store_error) => {
                error = Some(format!("SQLite LocalStore unavailable: {store_error:#}"));
                LocalStateV1::default()
            }
        };
        let planning = if error.is_none() {
            match sqlite.load_planning_snapshot() {
                Ok(snapshot) => snapshot,
                Err(store_error) => {
                    error = Some(format!(
                        "SQLite planning store unavailable: {store_error:#}"
                    ));
                    PlanningSnapshot::default()
                }
            }
        } else {
            PlanningSnapshot::default()
        };
        let writable = error.is_none();

        Ok((
            Self {
                sqlite,
                _writer_guard: Some(guard),
                writable,
                error,
            },
            StoreBootstrap {
                config,
                local,
                planning,
            },
        ))
    }

    pub(crate) fn persist_operator_state(&mut self, state: &LocalStateV1) -> Option<String> {
        if !self.writable {
            return None;
        }
        match self.sqlite.save_state(state) {
            Ok(()) => None,
            Err(error) => {
                let message = format!("SQLite operator-state write failed: {error:#}");
                self.writable = false;
                self.error = Some(message.clone());
                Some(message)
            }
        }
    }

    pub(crate) fn flush_operator_state_on_exit(&mut self, state: &LocalStateV1) -> Result<()> {
        anyhow::ensure!(
            self.writable,
            "final SQLite operator-state flush refused: {}",
            self.error.as_deref().unwrap_or("store is read-only")
        );
        self.sqlite
            .save_state(state)
            .context("final SQLite operator-state flush")
    }

    pub(crate) fn error(&self) -> Option<String> {
        self.error.clone()
    }

    pub(crate) fn sqlite_clone(&self) -> SqliteStore {
        self.sqlite.clone()
    }

    pub(crate) fn index_conversation_page(&self, page: &ConversationPage) -> Option<String> {
        self.sqlite
            .index_conversation_page(page)
            .err()
            .map(|error| format!("derived transcript index unavailable: {error:#}"))
    }

    pub(crate) fn search_transcript(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<TranscriptSearchResults, String> {
        self.sqlite
            .search_transcript(query, limit)
            .map_err(|error| format!("local transcript search unavailable: {error:#}"))
    }

    pub(crate) fn mutate_work_card<F>(
        &mut self,
        anchor: SourceRef,
        operation: F,
    ) -> Result<PlanningSnapshot, String>
    where
        F: FnOnce(&mut WorkCardRecord),
    {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }

        let result = (|| -> Result<PlanningSnapshot> {
            let mut card = match self.sqlite.work_card_for_anchor(&anchor)? {
                Some(card) => card,
                None => match anchor.kind {
                    SourceKind::CodexThread => WorkCardRecord::implicit_thread(
                        &codex_tui::domain::ThreadId::new(anchor.value.clone()),
                    ),
                    SourceKind::ScratchWork => WorkCardRecord {
                        local_id: anchor.value.clone(),
                        anchor: anchor.clone(),
                        links: vec![],
                        overlay: Default::default(),
                    },
                    _ => anyhow::bail!("local overlay unsupported for {:?}", anchor.kind),
                },
            };
            operation(&mut card);
            self.sqlite.upsert_work_card(&card)?;
            self.sqlite.load_planning_snapshot()
        })();

        match result {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                let message = format!("SQLite WorkCard write failed: {error:#}");
                self.writable = false;
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }

    pub(crate) fn snooze_work_card(
        &mut self,
        anchor: SourceRef,
        duration_ms: u64,
    ) -> Result<PlanningSnapshot, String> {
        let until = now_unix_ms().saturating_add(duration_ms);
        self.mutate_work_card(anchor, |card| {
            card.overlay.snooze_until_unix_ms = Some(until);
        })
    }

    pub(crate) fn apply_local_batch(
        &mut self,
        plan: &codex_tui::batch_local::LocalBatchPlan,
    ) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .apply_local_batch(plan)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        self.finish_planning_write(result, "local batch")
    }

    pub(crate) fn save_source_note(
        &mut self,
        owner: SourceRef,
        text: String,
    ) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = if text.trim().is_empty() {
            self.sqlite.delete_note(&owner)
        } else {
            self.sqlite.upsert_note(&LocalNote {
                owner,
                text,
                updated_at_unix_ms: now_unix_ms(),
            })
        }
        .and_then(|_| self.sqlite.load_planning_snapshot());
        self.finish_planning_write(result, "note")
    }

    pub(crate) fn update_scratch_note(
        &mut self,
        scratch_id: String,
        note: Option<String>,
    ) -> Result<PlanningSnapshot, String> {
        self.update_scratch(scratch_id, |scratch| {
            scratch.note = note;
        })
    }

    pub(crate) fn update_scratch_state(
        &mut self,
        scratch_id: String,
        state: ScratchState,
    ) -> Result<PlanningSnapshot, String> {
        self.update_scratch(scratch_id, |scratch| {
            scratch.state = state;
        })
    }

    pub(crate) fn update_scratch<F>(
        &mut self,
        scratch_id: String,
        operation: F,
    ) -> Result<PlanningSnapshot, String>
    where
        F: FnOnce(&mut codex_tui::planning::ScratchWork),
    {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = (|| -> Result<PlanningSnapshot> {
            let mut scratch = self
                .sqlite
                .load_planning_snapshot()?
                .scratch
                .into_iter()
                .find(|scratch| scratch.id == scratch_id)
                .ok_or_else(|| anyhow::anyhow!("ScratchWork does not exist: {scratch_id}"))?;
            operation(&mut scratch);
            scratch.updated_at_unix_ms = now_unix_ms();
            self.sqlite.update_scratch(&scratch)?;
            self.sqlite.load_planning_snapshot()
        })();
        self.finish_planning_write(result, "ScratchWork")
    }

    pub(crate) fn delete_scratch(
        &mut self,
        scratch_id: String,
    ) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .delete_scratch(&scratch_id)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        self.finish_planning_write(result, "ScratchWork delete")
    }

    pub(crate) fn create_bookmark(
        &mut self,
        source: SourceRef,
        label: Option<String>,
    ) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .create_bookmark(source, label.as_deref(), None)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        self.finish_planning_write(result, "bookmark")
    }

    pub(crate) fn finish_planning_write(
        &mut self,
        result: Result<PlanningSnapshot>,
        label: &str,
    ) -> Result<PlanningSnapshot, String> {
        match result {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                let message = format!("SQLite {label} write failed: {error:#}");
                self.writable = false;
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }

    pub(crate) fn save_view(&mut self, view: SavedView) -> Result<PlanningSnapshot, String> {
        validate_saved_view(&view)?;
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .save_view(&view)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        self.finish_planning_write(result, "SavedView")
    }

    pub(crate) fn delete_view(&mut self, view_id: String) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .delete_view(&view_id)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        self.finish_planning_write(result, "SavedView delete")
    }

    pub(crate) fn set_hot_slot(
        &mut self,
        slot: u8,
        target: SourceRef,
    ) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .set_hot_slot(slot, target)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        match result {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                let message = format!("SQLite hot-slot write failed: {error:#}");
                self.writable = false;
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }

    pub(crate) fn create_scratch(
        &mut self,
        title: String,
        workspace: Option<String>,
    ) -> Result<PlanningSnapshot, String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite planning store is read-only".into()));
        }
        let result = self
            .sqlite
            .create_scratch(&title, None, workspace.as_deref(), None)
            .and_then(|_| self.sqlite.load_planning_snapshot());
        match result {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                let message = format!("SQLite ScratchWork write failed: {error:#}");
                self.writable = false;
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_operator_state_flush_failure_is_returned_to_the_caller() {
        let root = tempfile::tempdir().expect("tempdir");
        let blocked = root.path().join("blocked-parent");
        std::fs::write(&blocked, "not a directory").expect("blocked parent fixture");
        let mut store = RuntimeStore {
            sqlite: SqliteStore::at(blocked),
            writable: true,
            error: None,
        };

        let error = store
            .flush_operator_state_on_exit(&LocalStateV1::default())
            .expect_err("final flush failure must not be silently ignored");
        assert!(
            error
                .to_string()
                .contains("final SQLite operator-state flush"),
            "unexpected final flush error: {error:#}"
        );
    }
}
