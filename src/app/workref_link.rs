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
        if self.workref_link_pending
            || self.workref_ledger.items.is_empty()
            || self.planning_store_error.is_some()
        {
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
            .or_else(|| (!thread.workspace.trim().is_empty()).then(|| thread.workspace.clone()))
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
        self.workref_link_pending = false;
        self.input_mode = InputMode::WorkrefLink;
        self.input_buffer.clear();
        self.mutation_notice = None;
    }

    /// Return a write effect only while the exact view, thread, Target and Home
    /// still match the snapshot captured when the editor was opened.
    pub(super) fn commit_workref_link(&mut self) -> Option<Effect> {
        if self.workref_link_pending {
            return None;
        }
        let key = self.input_buffer.trim().to_string();
        if key.is_empty() {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "enter an existing work-item key",
                    "请输入已有事项的 Key",
                )
                .into(),
            );
            return None;
        }
        if key.len() > 64
            || !key.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | ':')
            })
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
                local_text(
                    self.language,
                    "link editor expired; reopen it",
                    "关联编辑已失效，请重新打开",
                )
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
        // Keep the editor and exact input alive until the ordered SQLite worker
        // returns a durable commit receipt. Admission rejection must be retryable.
        self.workref_link_pending = true;
        Some(Effect::LinkWorkrefSession {
            key,
            session: intent.session,
        })
    }

    pub fn workref_link_submission_rejected(&mut self, error: String) {
        self.workref_link_pending = false;
        self.mutation_notice = Some(error);
    }

    pub(crate) fn workref_link_cancel_refused(&mut self) {
        self.mutation_notice = Some(
            local_text(
                self.language,
                "the SQLite write was already accepted; wait for its commit receipt",
                "SQLite 写入已被接收；请等待提交回执",
            )
            .into(),
        );
    }

    pub fn finish_workref_link_success(
        &mut self,
        key: String,
        ledger: crate::workref::WorkRefLedger,
        alias: String,
    ) {
        self.workref_link_pending = false;
        self.install_workrefs(Some(ledger));
        self.cancel_workref_link();
        let notice = if self.language.is_simplified_chinese() {
            format!("已关联事项 {key} · 会话别名 {alias} · 手工/未核验；未修改飞书/TAPD")
        } else {
            format!(
                "Linked {key} · session alias {alias} · manual/unverified; Feishu/TAPD unchanged"
            )
        };
        self.mutation_notice = Some(notice);
    }

    pub fn finish_workref_link_failure(&mut self, error: String) {
        self.workref_link_pending = false;
        self.mutation_notice = Some(error.clone());
        if error.starts_with("SQLite workref read failed:")
            || error.starts_with("SQLite workref write failed:")
        {
            self.install_workrefs(None);
            self.planning_store_error = Some(error);
            self.cancel_workref_link();
        }
    }

    pub fn workref_link_outcome_unknown(&mut self, error: String) {
        if !self.workref_link_pending {
            return;
        }
        self.workref_link_pending = false;
        self.install_workrefs(None);
        self.planning_store_error = Some(error.clone());
        self.cancel_workref_link();
        self.mutation_notice = Some(local_text(
            self.language,
            "work-item link outcome is unknown because the local worker stopped; inspect SQLite before retrying",
            "本地写入工作线程已停止，事项关联结果未知；请先检查 SQLite，再决定是否重试",
        ).into());
    }

    pub(super) fn cancel_workref_link(&mut self) {
        self.workref_link_pending = false;
        self.workref_link_intent = None;
        if self.input_mode == InputMode::WorkrefLink {
            self.input_mode = InputMode::Normal;
        }
        self.input_buffer.clear();
    }

    pub fn workref_ledger(&self) -> &crate::workref::WorkRefLedger {
        &self.workref_ledger
    }

    pub(crate) fn workref_link_items(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::workref::WorkItem)> {
        self.workref_ledger
            .items
            .iter()
            .map(|(key, item)| (key.as_str(), item))
            .take(5)
    }
}

#[cfg(test)]
#[path = "workref_link/tests.rs"]
mod tests;
