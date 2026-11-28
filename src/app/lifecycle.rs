use super::{AppState, local_text};
use crate::app::{Effect, View};
use crate::domain::{CwdLocality, ThreadId, classify_cwd};
use crate::planning::SourceKind;

impl AppState {
    pub(super) fn lifecycle_thread_id(&self) -> Option<ThreadId> {
        match &self.view {
            View::Registry => self.selected_thread_id(),
            View::Thread(id) | View::Review(id) | View::Workspace(id) => Some(id.clone()),
            View::Board => self.selected_planning_card().and_then(|card| {
                (card.anchor.kind == SourceKind::CodexThread)
                    .then(|| ThreadId::new(card.anchor.value.clone()))
            }),
            View::Scratch(_) => None,
        }
    }

    pub(super) fn lifecycle_cwd(&self) -> Option<String> {
        let thread_id = self.lifecycle_thread_id()?;
        self.thread_by_id(&thread_id)
            .map(|thread| thread.metadata.cwd.clone())
            .filter(|cwd| !cwd.trim().is_empty())
    }

    pub(super) fn lifecycle_capability_available(&self, capability: &str) -> bool {
        self.backend_status.connected
            && self.backend_status.source != "fake"
            // No remote cwd validation exists; do not offer start/fork based
            // on a same-spelled local path supplied by a remote App Server.
            && !self.app_server_target_is_remote()
            && !self
                .backend_status
                .optional_capabilities_missing
                .iter()
                .any(|missing| missing == capability)
    }

    pub(super) fn plan_start_thread(&mut self) -> Vec<Effect> {
        if self.app_server_target_is_remote() {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "cannot start Codex thread: remote App Server cwd is not verified on this host",
                    "无法新建 Codex 会话：远端 App Server cwd 未在本机验证",
                )
                .into(),
            );
            return vec![];
        }
        let Some(cwd) = self.lifecycle_cwd() else {
            return vec![];
        };
        if classify_cwd(&cwd) != CwdLocality::LocalDirectory {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "cannot start Codex thread: target cwd is not a local directory",
                    "无法新建 Codex 会话：目标 cwd 不是本机目录",
                )
                .into(),
            );
            return vec![];
        }
        self.mutation_notice = Some(
            local_text(
                self.language,
                "starting Codex thread…",
                "正在新建 Codex 会话…",
            )
            .into(),
        );
        vec![Effect::StartThread { cwd }]
    }

    pub(super) fn plan_fork_thread(&mut self) -> Vec<Effect> {
        if self.app_server_target_is_remote() {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "cannot fork Codex thread: remote App Server source cwd is not verified",
                    "无法派生 Codex 会话：远端 App Server 源目录未验证",
                )
                .into(),
            );
            return vec![];
        }
        let Some(thread_id) = self.lifecycle_thread_id() else {
            return vec![];
        };
        let Some(cwd) = self
            .thread_by_id(&thread_id)
            .map(|thread| thread.metadata.cwd.clone())
        else {
            return vec![];
        };
        if classify_cwd(&cwd) != CwdLocality::LocalDirectory {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "cannot fork Codex thread: source cwd is not local",
                    "无法派生 Codex 会话：源 cwd 不属于本机",
                )
                .into(),
            );
            return vec![];
        }
        self.mutation_notice = Some(
            local_text(
                self.language,
                "forking Codex thread…",
                "正在派生 Codex 会话…",
            )
            .into(),
        );
        vec![Effect::ForkThread { thread_id }]
    }

    pub(super) fn apply_thread_created(
        &mut self,
        thread_id: ThreadId,
        operation: String,
    ) -> Vec<Effect> {
        if let Some(index) = self
            .threads
            .iter()
            .position(|thread| thread.id == thread_id)
        {
            self.previous_target = self.current_thread_id().cloned();
            self.selected = index;
            self.thread_ui.entry(thread_id.0.clone()).or_default();
            self.prepare_conversation(&thread_id);
            self.view = View::Thread(thread_id.clone());
            self.mutation_notice = Some(format!("{operation} succeeded"));
            return vec![Effect::LoadConversation(thread_id)];
        }
        self.mutation_notice = Some(format!(
            "{operation} succeeded; waiting for registry projection"
        ));
        vec![]
    }

    pub(super) fn apply_thread_lifecycle_failed(&mut self, operation: String, error: String) {
        self.mutation_notice = Some(format!("{operation} failed: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{BackendStatus, CodexBackend, FakeBackend};

    fn connected_status() -> BackendStatus {
        let mut status = BackendStatus::starting("codex-app-server");
        status.connected = true;
        status.registry_complete = true;
        status
    }

    fn app_with_local_cwd() -> (AppState, tempfile::TempDir) {
        let backend = FakeBackend::seeded();
        let mut app = AppState::new(backend.snapshot().threads);
        let dir = tempfile::tempdir().expect("tempdir");
        app.threads[0].metadata.cwd = dir.path().to_string_lossy().into_owned();
        app.backend_status = connected_status();
        (app, dir)
    }

    #[test]
    fn local_connected_thread_can_start_and_fork() {
        let (mut app, _dir) = app_with_local_cwd();
        let start = app.plan_start_thread();
        assert!(matches!(
            start.as_slice(),
            [Effect::StartThread { cwd }] if !cwd.is_empty()
        ));
        let fork = app.plan_fork_thread();
        assert!(matches!(
            fork.as_slice(),
            [Effect::ForkThread { thread_id }] if thread_id == &app.threads[0].id
        ));
    }

    #[test]
    fn remote_target_matching_local_directory_cannot_start_or_fork() {
        let (mut app, _dir) = app_with_local_cwd();
        let cwd = app.threads[0].metadata.cwd.clone();
        assert!(std::path::Path::new(&cwd).is_dir());
        app.backend_status.source =
            "codex-app-server[target=remote-fixture transport=websocket]".into();

        assert!(!app.lifecycle_capability_available("thread/start"));
        assert!(!app.lifecycle_capability_available("thread/fork"));
        assert!(
            !app.build_context_choices()
                .contains(&super::super::ContextChoice::NewCodexThread)
        );
        assert!(
            !app.build_context_choices()
                .contains(&super::super::ContextChoice::ForkCodexThread)
        );
        assert!(app.plan_start_thread().is_empty());
        assert!(
            app.mutation_notice
                .as_deref()
                .unwrap_or_default()
                .contains("remote App Server cwd")
        );
        assert!(app.plan_fork_thread().is_empty());
        assert!(
            app.mutation_notice
                .as_deref()
                .unwrap_or_default()
                .contains("remote App Server source cwd")
        );

        // Local stdio remains supported even when the target has a name.
        app.backend_status.source = "codex-app-server[target=local transport=stdio]".into();
        assert!(app.lifecycle_capability_available("thread/start"));
        assert!(app.lifecycle_capability_available("thread/fork"));
        assert!(matches!(
            app.plan_start_thread().as_slice(),
            [Effect::StartThread { cwd: target }] if target == &cwd
        ));
        assert!(matches!(
            app.plan_fork_thread().as_slice(),
            [Effect::ForkThread { .. }]
        ));
    }

    #[test]
    fn observed_missing_fork_capability_disables_only_fork() {
        let (mut app, _dir) = app_with_local_cwd();
        app.backend_status
            .optional_capabilities_missing
            .push("thread/fork".into());
        assert!(app.lifecycle_capability_available("thread/start"));
        assert!(!app.lifecycle_capability_available("thread/fork"));
    }
}
