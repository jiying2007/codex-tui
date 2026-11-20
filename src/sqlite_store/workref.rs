//! One private, bounded G2/G3 JSON envelope within the existing SQLite store.
use super::*;
use crate::workref::{MAX_LEDGER_BYTES, WorkRefLedger};

const WORKREF_KEY: &str = "g23-workrefs-v1";

impl SqliteStore {
    /// No schema migration or second local database. Unknown envelopes fail closed.
    pub fn load_workref_ledger(&self) -> Result<WorkRefLedger> {
        let conn = self.open_ready()?;
        let json = conn
            .query_row(
                "SELECT value_json FROM operator_state WHERE key=?1",
                [WORKREF_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(json) = json else {
            return Ok(WorkRefLedger::default());
        };
        anyhow::ensure!(
            json.len() <= MAX_LEDGER_BYTES,
            "work reference ledger too large"
        );
        let ledger: WorkRefLedger =
            serde_json::from_str(&json).context("decode work reference ledger")?;
        ledger.validate()?;
        Ok(ledger)
    }

    /// Caller must own acquire_local_writer(); an active TUI prevents CLI races.
    pub fn save_workref_ledger(&self, ledger: &WorkRefLedger) -> Result<()> {
        ledger.validate()?;
        let json = serde_json::to_string(ledger).context("encode work reference ledger")?;
        anyhow::ensure!(
            json.len() <= MAX_LEDGER_BYTES,
            "work reference ledger too large"
        );
        let mut conn = self.open_ready()?;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO operator_state(key, value_json, updated_at_unix_ms)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET
                value_json=excluded.value_json,
                updated_at_unix_ms=excluded.updated_at_unix_ms",
            params![WORKREF_KEY, json, u64_to_i64(now_unix_ms())?],
        )?;
        tx.commit().context("commit work reference ledger")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{LocalStateV1, LocalStore};
    use crate::workref::{WorkItem, WorkRefLedger};
    use tempfile::tempdir;

    #[test]
    fn same_sqlite_preserves_operator_state_and_manual_refs_separately() {
        let root = tempdir().unwrap();
        let store = SqliteStore::at(root.path());
        let _guard = store.acquire_local_writer().unwrap();
        let mut operator = LocalStateV1::default();
        operator.aliases.insert("thread".into(), "retain".into());
        store.save_state(&operator).unwrap();
        let mut ledger = WorkRefLedger::default();
        ledger
            .add_item(
                "bug-11".into(),
                WorkItem::new(
                    "tapd".into(),
                    "project".into(),
                    "bug".into(),
                    "11".into(),
                    Some("https://www.tapd.cn/".into()),
                )
                .unwrap(),
            )
            .unwrap();
        store.save_workref_ledger(&ledger).unwrap();
        assert_eq!(store.load_state().unwrap(), operator);
        assert_eq!(store.load_workref_ledger().unwrap(), ledger);
        let reopen = SqliteStore::at(root.path());
        assert_eq!(reopen.load_workref_ledger().unwrap(), ledger);
    }

    #[test]
    fn malformed_and_future_envelopes_are_never_overwritten_as_defaults() {
        let root = tempdir().unwrap();
        let store = SqliteStore::at(root.path());
        let _guard = store.acquire_local_writer().unwrap();
        let mut ledger = WorkRefLedger::default();
        ledger.schema_version = 2;
        assert!(store.save_workref_ledger(&ledger).is_err());
        let conn = store.open_ready().unwrap();
        conn.execute(
            "INSERT INTO operator_state(key, value_json, updated_at_unix_ms)
             VALUES(?1, ?2, 0)",
            params![WORKREF_KEY, "{invalid"],
        )
        .unwrap();
        assert!(store.load_workref_ledger().is_err());
        assert!(
            store.load_state().is_ok(),
            "operator preferences remain intact"
        );
    }
}
