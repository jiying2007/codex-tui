//! Start a local manual work-item link from the exact currently observed Codex session.
use super::{AppState, Effect, InputMode, View, local_text};
use crate::{
    domain::ThreadId,
    planning::{SourceKind, SourceRef},
    workref::SessionRef,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WorkrefLinkIntent {
    view: View,
    target: SourceRef,
    cwd: String,
    session: SessionRef,
}

impl AppState {
    /// A picker is offered only for a live native Thread in a connected,
    /// already-bound Target/Home scope and a valid non-empty local ledger.
    pub(super) fn workref_link_snapshot(&self) -> Option<WorkrefLinkIntent> {
        if self.workref_ledger.items.is_empty() || self.planning_store_error.is_some() {
            return None;
        }
        let (target_identity, codex_home) = self.workref_scope_identity()?;
        let target = self.selected_local_target()?;
        if target.kind != SourceKind::CodexThread || !self.local_target_live(&target) {
            return None;
        }
        let thread_id = ThreadId::new(target.value.clone());
        let thread = self.thread_by_id(&thread_id)?;
        let cwd = thread.metadata.cwd.clone();
        let repository = self
            .git_context(&thread_id)
            .filter(|context| {
                context.cwd == cwd && context.is_repository && context.error.is_none()
            })
            .and_then(|context| context.repo.as_ref())
            .map(|repo| repo.primary_root.clone())
            .or_else(|| {
                (!thread.workspace.trim().is_empty()).then(|| thread.workspace.clone())
            })
            .or_else(|| (!cwd.trim().is_empty()).then(|| cwd.clone()))?;
        Some(WorkrefLinkIntent {
            view: self.view.clone(),
            target,
            cwd,
            session: SessionRef {
                target_identity,
                codex_home,
                thread_id: thread_id.0,
                repository,
                role: "development".into(),
            },
        })
    }

    pub(super) fn begin_workref_link(&mut self) {
        let Some(intent) = self.workref_link_snapshot() else {
            let message = if self.workref_unavailable {
                local_text(
                    self.language,
                    "local work references are unavailable; inspect the SQLite store",
                    "本地事项引用不可用；请检查 SQLite 存储",
                )
            } else if self.workref_ledger.items.is_empty() {
                local_text(
                    self.language,
                    "no work items recorded; add the real Feishu/TAPD reference with workref add first",
                    "尚无事项记录；请先用 workref add 添加真实飞书/TAPD 引用",
                )
            } else {
                local_text(
                    self.language,
                    "linking requires a connected, exact Target/CODEX_HOME and a live Codex thread",
                    "关联事项需要已连接的精确 Target/CODEX_HOME 和当前有效 Codex 会话",
                )
            };
            self.mutation_notice = Some(message.into());
            return;
        };
        self.workref_link_intent = Some(intent);
        self.input_mode = InputMode::WorkrefLink;
        self.input_buffer.clear();
        self.mutation_notice = None;
    }

    /// Return a write effect only while the exact view, thread, Target and Home
    /// still match the snapshot captured when the editor was opened.
    pub(super) fn commit_workref_link(&mut self) -> Option<Effect> {
        let key = self.input_buffer.trim().to_string();
        if key.is_empty() {
            self.mutation_notice = Some(
                local_text(self.language, "enter an existing work-item key", "请输入已有事项的 Key")
                    .into(),
            );
            return None;
        }
        if key.len() > 64
            || !key
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | ':'))
        {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "work-item key must use ASCII letters, digits, _, - or : (max 64 bytes)",
                    "事项 Key 仅允许 ASCII 字母、数字、_、-、:（最多 64 字节）",
                )
                .into(),
            );
            return None;
        }
        let Some(intent) = self.workref_link_intent.clone() else {
            self.cancel_workref_link();
            self.mutation_notice = Some(
                local_text(self.language, "link editor expired; reopen it", "关联编辑已失效，请重新打开")
                    .into(),
            );
            return None;
        };
        if self.workref_link_snapshot().as_ref() != Some(&intent) {
            self.cancel_workref_link();
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "session/Target/Home changed; no link was written; reopen the action",
                    "会话/Target/Home 已变化；未写入关联，请重新打开操作",
                )
                .into(),
            );
            return None;
        }
        if !self.workref_ledger.items.contains_key(&key) {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "unknown work-item key; add the authoritative Feishu/TAPD reference first",
                    "事项 Key 不存在；请先添加权威飞书/TAPD 引用",
                )
                .into(),
            );
            return None;
        }
        self.cancel_workref_link();
        Some(Effect::LinkWorkrefSession {
            key,
            session: intent.session,
        })
    }

    pub(super) fn cancel_workref_link(&mut self) {
        self.workref_link_intent = None;
        if self.input_mode == InputMode::WorkrefLink {
            self.input_mode = InputMode::Normal;
        }
        self.input_buffer.clear();
    }

    pub(crate) fn workref_link_items(&self) -> impl Iterator<Item = (&str, &crate::workref::WorkItem)> {
        self.workref_ledger
            .items
            .iter()
            .map(|(key, item)| (key.as_str(), item))
            .take(5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_server_target::ResolvedAppServerTarget,
        backend::{CodexBackend, FakeBackend},
        store::LocalStateV1,
        command::Command,
    };

    fn configured() -> AppState {
        let target = ResolvedAppServerTarget::implicit_local(None);
        let mut app = AppState::new(FakeBackend::scaled(3).snapshot().threads);
        app.configure_operator_target(&target);
        app.apply_local_state(&LocalStateV1::default());
        let mut status = crate::backend::BackendStatus::starting(target.status_source());
        status.connected = true;
        status.codex_home = Some("/home/test/.codex".into());
        app.backend_status = status;
        app.bind_operator_status();
        let thread = app.threads[0].id.clone();
        app.view = View::Thread(thread);
        let mut ledger = crate::workref::WorkRefLedger::default();
        ledger
            .add_item(
                "BUG17".into(),
                crate::workref::WorkItem::new(
                    "local-draft".into(),
                    "robot".into(),
                    "technical".into(),
                    "BUG17".into(),
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        app.install_workrefs(Some(ledger));
        app
    }

    #[test]
    fn workref_link_captures_exact_live_thread_and_generates_only_a_write_effect() {
        let mut app = configured();
        let expected = app.workref_link_snapshot().expect("exact binding");
        assert_eq!(expected.session.codex_home, "/home/test/.codex");
        assert_eq!(expected.session.role, "development");
        reduce(&mut app, Action::BeginWorkrefLink);
        assert_eq!(app.input_mode, InputMode::WorkrefLink);
        assert!(reduce(&mut app, Action::InputText("BUG17".into())).is_empty());
        let effect = reduce(&mut app, Action::CommitInput)
            .pop()
            .expect("link effect");
        assert!(matches!(
            effect,
            Effect::LinkWorkrefSession { key, session }
                if key == "BUG17" && session == expected.session
        ));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn workref_link_refuses_home_drift_and_unknown_keys_without_writing() {
        let mut app = configured();
        reduce(&mut app, Action::BeginWorkrefLink);
        app.input_buffer = "BUG17".into();
        app.backend_status.codex_home = Some("/home/other/.codex".into());
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.mutation_notice.as_deref().unwrap_or_default().contains("changed"));

        let mut app = configured();
        reduce(&mut app, Action::BeginWorkrefLink);
        app.input_buffer = "MISSING".into();
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        assert_eq!(app.input_mode, InputMode::WorkrefLink);
        assert!(app.mutation_notice.as_deref().unwrap_or_default().contains("unknown"));
    }

    #[test]
    fn palette_and_context_menu_offer_link_only_for_exact_existing_local_workrefs() {
        let mut app = configured();
        reduce(&mut app, Action::OpenCommandPalette);
        app.command_palette_query = "link".into();
        assert_eq!(app.command_palette_choice(), Some(Command::LinkWorkItem));
        app.backend_status.codex_home = Some("/home/other/.codex".into());
        assert!(app.take_command_palette_choice().is_none());
        assert!(!app.command_palette_open);

        let mut app = configured();
        assert!(app.context_choices().contains(&crate::app::ContextChoice::LinkWorkItem));
        app.install_workrefs(Some(crate::workref::WorkRefLedger::default()));
        assert!(!app.context_choices().contains(&crate::app::ContextChoice::LinkWorkItem));
    }

    #[test]
    fn workref_link_is_unavailable_without_connected_scope_or_existing_items() {
        let mut app = configured();
        app.backend_status.connected = false;
        assert!(app.workref_link_snapshot().is_none());
        app.backend_status.connected = true;
        app.install_workrefs(Some(crate::workref::WorkRefLedger::default()));
        assert!(app.workref_link_snapshot().is_none());
        app.install_workrefs(None);
        assert!(app.workref_link_snapshot().is_none());
    }
}
