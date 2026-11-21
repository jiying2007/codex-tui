//! The active connection owns only its own drafts, pins, aliases and unread state.
use super::*;
use crate::{
    app_server_target::ResolvedAppServerTarget,
    store::{ScopedOperatorState, operator_scope_key, target_instance_identity},
};

#[derive(Clone, Debug, Default)]
pub(crate) struct OperatorLocal {
    expected_source: Option<String>,
    target_identity: Option<String>,
    active_scope: Option<String>,
    persisted: Option<LocalStateV1>,
}

impl AppState {
    /// Configure before reading local state; this does not authorize any RPC.
    pub fn configure_operator_target(&mut self, target: &ResolvedAppServerTarget) {
        self.operator_local.expected_source = Some(target.status_source());
        self.operator_local.target_identity = Some(target_instance_identity(target));
        self.bind_operator_status();
    }

    pub fn apply_local_state(&mut self, local: &LocalStateV1) {
        self.operator_local.persisted = Some(local.clone());
        self.host_local_only = local.host_local_only;
        self.repo_backed_only = local.repo_backed_only;
        if self.host_local_only || self.repo_backed_only {
            self.reconcile_cwd_locality_cache(true);
        }
        if self.operator_local.target_identity.is_some() {
            self.operator_local.active_scope = None;
            self.bind_operator_status();
        } else {
            self.install_local_overlay(ScopedOperatorState::legacy(local));
        }
    }

    /// A different Target or home cannot reuse an old ThreadId's preferences.
    pub(super) fn bind_operator_status(&mut self) {
        let Some(target) = self.operator_local.target_identity.as_deref() else {
            return;
        };
        let expected = self.operator_local.expected_source.as_deref();
        if Some(self.backend_status.source.as_str()) != expected {
            if self.operator_local.active_scope.is_some() {
                self.operator_local.persisted = Some(self.to_local_state());
                self.operator_local.active_scope = None;
                self.install_local_overlay(ScopedOperatorState::default());
            }
            return;
        }
        if !self.backend_status.connected {
            return;
        }
        let key = operator_scope_key(target, self.backend_status.codex_home.as_deref());
        if self.operator_local.active_scope.as_deref() == Some(key.as_str()) {
            return;
        }
        self.operator_local.persisted = Some(self.to_local_state());
        self.operator_local.active_scope = Some(key.clone());
        let view = self
            .operator_local
            .persisted
            .as_ref()
            .and_then(|state| state.scopes.get(&key))
            .cloned()
            .unwrap_or_default();
        self.install_local_overlay(view);
    }

    /// Install a bounded, checked startup snapshot. CLI workref writes cannot
    /// race with a running TUI because both share the same exclusive writer.
    pub fn install_workrefs(&mut self, workrefs: Option<WorkRefLedger>) {
        self.workref_unavailable = workrefs.is_none();
        self.workref_ledger = Arc::new(workrefs.unwrap_or_default());
    }

    /// Exact connected Target/CODEX_HOME scope for local work references.
    /// This is a manually configured connection identity, not host attestation.
    pub(super) fn workref_scope_identity(&self) -> Option<(String, String)> {
        let target = self.operator_local.target_identity.as_deref()?;
        let home = self.backend_status.codex_home.as_deref()?;
        if self.workref_unavailable
            || !self.backend_status.connected
            || home.trim().is_empty()
            || self.operator_local.expected_source.as_deref()
                != Some(self.backend_status.source.as_str())
            || self.operator_local.active_scope.as_deref()
                != Some(operator_scope_key(target, Some(home)).as_str())
        {
            return None;
        }
        Some((target.to_string(), home.to_string()))
    }

    /// Local, manual-only links for an actually observed thread. Never match
    /// on a bare ThreadId, target name, path spelling, or historical archive.
    pub fn manual_workrefs_for_thread(
        &self,
        thread_id: &str,
    ) -> Vec<(&str, &str, &crate::workref::WorkItem)> {
        let Some((target, home)) = self.workref_scope_identity() else {
            return Vec::new();
        };
        if !self.threads.iter().any(|thread| thread.id.0 == thread_id) {
            return Vec::new();
        }
        let mut matched = Vec::new();
        for (key, item) in &self.workref_ledger.items {
            for (alias, reference) in &item.sessions {
                if reference.target_identity == target
                    && reference.codex_home == home
                    && reference.thread_id == thread_id
                {
                    matched.push((key.as_str(), alias.as_str(), item));
                }
            }
        }
        matched
    }

    fn install_local_overlay(&mut self, local: ScopedOperatorState) {
        self.thread_ui = local.thread_ui;
        self.local_pins = local.pins;
        self.local_aliases = local.aliases;
        self.local_marked_unread = local.marked_unread;
        self.acknowledged_attention = local.acknowledged_attention;
        for thread in &mut self.threads {
            thread.pinned = self.local_pins.contains(&thread.id.0);
            thread.alias = self.local_aliases.get(&thread.id.0).cloned();
            thread
                .attention
                .retain(|reason| *reason != AttentionReason::MarkedUnread);
            if self.local_marked_unread.contains(&thread.id.0) {
                thread.attention.push(AttentionReason::MarkedUnread);
            }
        }
        ensure_selection_visible(self);
    }

    pub fn to_local_state(&self) -> LocalStateV1 {
        let mut state = self.operator_local.persisted.clone().unwrap_or_default();
        let view = ScopedOperatorState {
            thread_ui: self.thread_ui.clone(),
            pins: self.local_pins.clone(),
            aliases: self.local_aliases.clone(),
            marked_unread: self.local_marked_unread.clone(),
            acknowledged_attention: self.acknowledged_attention.clone(),
        };
        if self.operator_local.target_identity.is_none() {
            state.thread_ui = view.thread_ui;
            state.pins = view.pins;
            state.aliases = view.aliases;
            state.marked_unread = view.marked_unread;
            state.acknowledged_attention = view.acknowledged_attention;
        } else if let Some(key) = &self.operator_local.active_scope {
            state.scopes.insert(key.clone(), view);
        }
        state.host_local_only = self.host_local_only;
        state.repo_backed_only = self.repo_backed_only;
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_server_target::{ResolvedAppServerEndpoint, ResolvedAppServerTarget},
        backend::{CodexBackend, FakeBackend},
    };

    fn target(name: &str) -> ResolvedAppServerTarget {
        ResolvedAppServerTarget {
            name: name.into(),
            endpoint: ResolvedAppServerEndpoint::Stdio { codex_bin: None },
        }
    }

    fn status(target: &ResolvedAppServerTarget, home: Option<&str>) -> BackendStatus {
        let mut status = BackendStatus::starting(target.status_source());
        status.connected = true;
        status.codex_home = home.map(str::to_owned);
        status
    }

    #[test]
    fn same_thread_id_in_other_target_cannot_inherit_operator_state() {
        let thread_id = FakeBackend::seeded().snapshot().threads[0].id.0.clone();
        let a = target("A");
        let b = target("B");
        let mut old = LocalStateV1::default();
        old.pins.insert(thread_id.clone());
        old.aliases.insert(thread_id.clone(), "legacy".into());
        let mut first = AppState::new(FakeBackend::seeded().snapshot().threads);
        first.configure_operator_target(&a);
        first.apply_local_state(&old);
        first.backend_status = status(&a, Some("/same/.codex"));
        first.bind_operator_status();
        assert!(!first.threads[0].pinned);
        assert!(first.threads[0].alias.is_none());
        first.local_pins.insert(thread_id.clone());
        first
            .local_aliases
            .insert(thread_id.clone(), "only A".into());
        first.local_marked_unread.insert(thread_id.clone());
        first.thread_ui.entry(thread_id.clone()).or_default().draft = "draft A".into();
        first.acknowledged_attention.insert(thread_id.clone());
        let saved = first.to_local_state();
        assert!(saved.pins.contains(&thread_id));
        assert_eq!(saved.scopes.len(), 1);

        let mut second = AppState::new(FakeBackend::seeded().snapshot().threads);
        second.configure_operator_target(&b);
        second.apply_local_state(&saved);
        second.backend_status = status(&b, Some("/same/.codex"));
        second.bind_operator_status();
        assert!(second.thread_ui.is_empty());
        assert!(!second.local_pins.contains(&thread_id));
        assert!(!second.local_marked_unread.contains(&thread_id));
        assert!(second.acknowledged_attention.is_empty());
        assert!(second.threads[0].alias.is_none());
        assert!(!second.threads[0].pinned);
        let preserved = second.to_local_state();
        assert_eq!(preserved.scopes.len(), 2);
    }

    #[test]
    fn workref_projection_never_reuses_same_thread_across_target_or_home() {
        use crate::workref::{SessionRef, WorkItem};
        let a = target("host-A");
        let b = target("host-B");
        let thread_id = FakeBackend::seeded().snapshot().threads[0].id.0.clone();
        let mut ledger = WorkRefLedger::default();
        ledger
            .add_item(
                "BUG17".into(),
                WorkItem::new(
                    "tapd".into(),
                    "robot".into(),
                    "bug".into(),
                    "17".into(),
                    Some("https://www.tapd.cn/".into()),
                )
                .unwrap(),
            )
            .unwrap();
        for (alias, host) in [("soc", &a), ("mcu", &b)] {
            ledger
                .add_session(
                    "BUG17",
                    alias.into(),
                    SessionRef {
                        target_identity: target_instance_identity(host),
                        codex_home: "/same/.codex".into(),
                        thread_id: thread_id.clone(),
                        repository: format!("repo-{alias}"),
                        role: alias.into(),
                    },
                )
                .unwrap();
        }
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.configure_operator_target(&a);
        app.apply_local_state(&LocalStateV1::default());
        app.install_workrefs(Some(ledger));
        app.backend_status = status(&a, Some("/same/.codex"));
        app.bind_operator_status();
        let links = app.manual_workrefs_for_thread(&thread_id);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].1, "soc");
        // A same-spelled ThreadId on a different home is not a match.
        app.backend_status.codex_home = Some("/other/.codex".into());
        assert!(app.manual_workrefs_for_thread(&thread_id).is_empty());
        app.backend_status = status(&a, Some("/same/.codex"));
        app.bind_operator_status();
        assert_eq!(app.manual_workrefs_for_thread(&thread_id).len(), 1);
        // A disconnected or source-drifted target cannot claim the old links.
        app.backend_status.connected = false;
        assert!(app.manual_workrefs_for_thread(&thread_id).is_empty());
        app.backend_status = status(&b, Some("/same/.codex"));
        assert!(app.manual_workrefs_for_thread(&thread_id).is_empty());
        app.install_workrefs(None);
        assert!(app.workref_unavailable);
        assert!(app.manual_workrefs_for_thread(&thread_id).is_empty());
        assert!(app.manual_workrefs_for_thread("archive-only").is_empty());
    }

    #[test]
    fn home_change_flushes_previous_scope_and_resets_current_preferences() {
        let a = target("local");
        let thread_id = FakeBackend::seeded().snapshot().threads[0].id.0.clone();
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.configure_operator_target(&a);
        app.apply_local_state(&LocalStateV1::default());
        app.backend_status = status(&a, Some("/home/one"));
        app.bind_operator_status();
        app.local_aliases
            .insert(thread_id.clone(), "home one".into());
        app.backend_status.codex_home = Some("/home/two".into());
        app.bind_operator_status();
        assert!(app.local_aliases.is_empty());
        let saved = app.to_local_state();
        assert_eq!(saved.scopes.len(), 2);
        assert_eq!(
            saved
                .scopes
                .values()
                .filter(|s| s.aliases.contains_key(&thread_id))
                .count(),
            1
        );
    }
}
