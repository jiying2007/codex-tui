//! One in-flight local editor write; only its receipt can retire its draft.
use super::{Action, AppState, Effect, InputMode, local_text};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub(super) struct PendingLocalEdit {
    ticket: u64,
    revision: u64,
    mode: InputMode,
    effect: Effect,
}

impl AppState {
    pub(super) fn observe_local_edit_action(&mut self, action: &Action) {
        if matches!(action,
            Action::InputChar(_) | Action::InputText(_) | Action::Backspace
            | Action::CancelInput | Action::Back | Action::BeginScratch
            | Action::ExecuteContext | Action::CloseSavedViewEditor
            | Action::MoveSavedViewEditorField(_) | Action::CycleSavedViewEditorValue(_)
            | Action::BeginSavedViewFieldEdit
        ) {
            self.local_edit_revision = self.local_edit_revision.saturating_add(1);
        }
    }

    pub fn local_edit_ticket_for(&self, effect: &Effect) -> Option<u64> {
        self.pending_local_edit.as_ref()
            .filter(|pending| &pending.effect == effect)
            .map(|pending| pending.ticket)
    }

    pub fn pending_local_edit_ticket(&self) -> Option<u64> {
        self.pending_local_edit.as_ref().map(|pending| pending.ticket)
    }

    pub(super) fn begin_local_edit_write(&mut self, effect: Effect) -> Vec<Effect> {
        if self.pending_local_edit.is_some() {
            self.mutation_notice = Some(local_text(self.language,
                "local save is still pending; draft retained, no second write submitted",
                "本地保存仍在处理中；已保留草稿，未重复提交").into());
            return vec![];
        }
        if self.planning_store_error.is_some() {
            self.mutation_notice = Some(local_text(self.language,
                "local store is unavailable; draft retained, no write submitted",
                "本地存储不可用；已保留草稿，未提交写入").into());
            return vec![];
        }
        let Ok(ticket) = NEXT_TICKET.fetch_update(Ordering::Relaxed, Ordering::Relaxed,
            |next| next.checked_add(1)) else {
            self.mutation_notice = Some("local save identity exhausted; draft retained".into());
            return vec![];
        };
        self.pending_local_edit = Some(PendingLocalEdit {
            ticket, revision: self.local_edit_revision, mode: self.input_mode,
            effect: effect.clone(),
        });
        self.mutation_notice = Some(local_text(self.language,
            "saving local changes; draft retained until receipt; closing does not undo a submitted write",
            "正在保存本地更改；回执前保留草稿，关闭编辑不会撤销已提交写入").into());
        vec![effect]
    }

    /// Errors may occur after commit while refreshing the projection. Never
    /// advertise rollback or automatically retry an unconfirmed write.
    pub fn finish_local_edit_write(&mut self, ticket: u64, result: Result<(), String>) {
        if self.pending_local_edit_ticket() != Some(ticket) { return; }
        let pending = self.pending_local_edit.take().expect("matching local write");
        if let Err(error) = result {
            self.mutation_notice = Some(format!("{}: {error}", local_text(self.language,
                "local save not confirmed; draft retained unless explicitly closed; check state before retrying",
                "本地保存未确认；未主动关闭的草稿已保留，请核对状态后再操作")));
            if self.saved_view_editor.is_some() { self.saved_view_editor_error = Some(error); }
            return;
        }
        if pending.revision == self.local_edit_revision
            && self.local_edit_revision != u64::MAX && pending.mode == self.input_mode {
            match pending.effect {
                Effect::CreateScratch { .. } => self.new_scratch_workspace = None,
                Effect::SaveSourceNote { .. } | Effect::UpdateScratchNote { .. } => self.note_target = None,
                Effect::SaveSavedView { .. } => {
                    self.saved_view_editor = None;
                    self.saved_view_editor_error = None;
                }
                _ => return,
            }
            self.input_mode = InputMode::Normal;
            self.input_buffer.clear();
            self.mutation_notice = Some(local_text(self.language,
                "local changes saved", "本地更改已保存").into());
        } else {
            self.mutation_notice = Some(local_text(self.language,
                "submitted local changes saved; newer editor state was preserved",
                "已提交的本地更改已保存；后续编辑状态已保留").into());
        }
    }
}
