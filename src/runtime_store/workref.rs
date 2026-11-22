use super::RuntimeStore;
use codex_tui::workref::{SessionRef, WorkRefLedger};

fn alias_base(repository: &str) -> String {
    let normalized = repository.replace('\\', "/");
    let leaf = normalized
        .trim_end_matches('/')
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("repo");
    let candidate = leaf
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else if matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let candidate = candidate.trim_matches(['-', '_']);
    let bounded = candidate.chars().take(48).collect::<String>();
    if bounded.is_empty() {
        "repo".into()
    } else {
        bounded
    }
}

fn next_alias(ledger: &WorkRefLedger, key: &str, session: &SessionRef) -> Result<String, String> {
    let item = ledger
        .items
        .get(key)
        .ok_or_else(|| format!("unknown work-item key {key}; add its authoritative Feishu/TAPD reference first"))?;
    if let Some((alias, _)) = item.sessions.iter().find(|(_, existing)| *existing == session) {
        return Ok(alias.clone());
    }
    let base = alias_base(&session.repository);
    for suffix in 1..=65 {
        let candidate = if suffix == 1 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        if !item.sessions.contains_key(&candidate) {
            return Ok(candidate);
        }
    }
    Err("no available session alias; work item session limit reached".into())
}

impl RuntimeStore {
    /// Add a manually requested link using the existing exclusive SQLite owner.
    /// The item must already exist; this never creates or updates an external ticket.
    pub(crate) fn link_workref_session(
        &mut self,
        key: &str,
        session: SessionRef,
    ) -> Result<(WorkRefLedger, String), String> {
        if !self.writable {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "SQLite store is read-only".into()));
        }
        let mut ledger = match self.sqlite.load_workref_ledger() {
            Ok(ledger) => ledger,
            Err(error) => {
                let message = format!("SQLite workref read failed: {error:#}");
                self.writable = false;
                self.error = Some(message.clone());
                return Err(message);
            }
        };
        let alias = next_alias(&ledger, key, &session)?;
        ledger
            .add_session(key, alias.clone(), session)
            .map_err(|error| format!("cannot link session: {error:#}"))?;
        if let Err(error) = self.sqlite.save_workref_ledger(&ledger) {
            let message = format!("SQLite workref write failed: {error:#}");
            self.writable = false;
            self.error = Some(message.clone());
            return Err(message);
        }
        Ok((ledger, alias))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_tui::{store::LocalStore, workref::WorkItem};

    fn session(target: &str, thread: &str) -> SessionRef {
        SessionRef {
            target_identity: target.into(),
            codex_home: "/home/test/.codex".into(),
            thread_id: thread.into(),
            repository: "/workspace/robot-app".into(),
            role: "development".into(),
        }
    }

    fn prepared_store(root: &std::path::Path) -> RuntimeStore {
        let sqlite = codex_tui::sqlite_store::SqliteStore::at(root);
        sqlite.load_config().unwrap();
        sqlite.health().unwrap();
        let mut ledger = WorkRefLedger::default();
        ledger
            .add_item(
                "BUG17".into(),
                WorkItem::new(
                    "local-draft".into(),
                    "robot".into(),
                    "technical".into(),
                    "BUG17".into(),
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        sqlite.save_workref_ledger(&ledger).unwrap();
        RuntimeStore::at_for_test(root)
    }

    #[test]
    fn link_existing_item_derives_stable_repo_alias_and_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let mut store = prepared_store(root.path());
        let session = session("target-A", "thread-1");
        let (first, alias) = store
            .link_workref_session("BUG17", session.clone())
            .unwrap();
        assert_eq!(alias, "robot-app");
        assert_eq!(first.items["BUG17"].sessions.len(), 1);
        let (second, repeated_alias) = store.link_workref_session("BUG17", session).unwrap();
        assert_eq!(repeated_alias, alias);
        assert_eq!(second.items["BUG17"].sessions.len(), 1);
        assert_eq!(store.sqlite.load_workref_ledger().unwrap(), second);
    }

    #[test]
    fn same_repo_on_another_target_gets_a_distinct_alias_and_missing_key_never_creates() {
        let root = tempfile::tempdir().unwrap();
        let mut store = prepared_store(root.path());
        let (first, alias_a) = store
            .link_workref_session("BUG17", session("target-A", "same-thread"))
            .unwrap();
        let (second, alias_b) = store
            .link_workref_session("BUG17", session("target-B", "same-thread"))
            .unwrap();
        assert_eq!(alias_a, "robot-app");
        assert_eq!(alias_b, "robot-app-2");
        assert_eq!(first.items["BUG17"].sessions.len(), 1);
        assert_eq!(second.items["BUG17"].sessions.len(), 2);
        assert!(store
            .link_workref_session("MISSING", session("target-C", "thread-C"))
            .unwrap_err()
            .contains("unknown work-item key"));
        assert_eq!(store.sqlite.load_workref_ledger().unwrap(), second);
    }
}
