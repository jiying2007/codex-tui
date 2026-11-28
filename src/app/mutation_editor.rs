//! Ephemeral intent guard for editing a native Codex Goal.
use super::{AppState, InputMode, local_text};
use crate::{
    domain::ThreadId,
    planning::{SourceKind, SourceRef},
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Goal {
        thread: ThreadId,
        original: Option<(i64, String)>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MutationEditor {
    mode: InputMode,
    target: Target,
}

fn family(mode: InputMode) -> Option<InputMode> {
    match mode {
        InputMode::GoalObjective => Some(mode),
        _ => None,
    }
}

impl AppState {
    pub(super) fn editor_snapshot(&self, mode: InputMode) -> Option<MutationEditor> {
        let mode = family(mode)?;
        let thread = self.current_thread_id()?.clone();
        self.thread_by_id(&thread)?;
        let target = match mode {
            InputMode::GoalObjective => Target::Goal {
                original: self
                    .goals
                    .get(&thread.0)
                    .map(|goal| (goal.created_at, goal.objective.clone())),
                thread,
            },
            _ => return None,
        };
        Some(MutationEditor { mode, target })
    }

    pub(super) fn begin_mutation_editor(&mut self, mode: InputMode) -> bool {
        self.mutation_editor = self.editor_snapshot(mode);
        if self.mutation_editor.is_none() {
            self.refuse_editor_target();
            return false;
        }
        self.mutation_notice = None;
        true
    }

    pub(super) fn guard_mutation_editor(&mut self, mode: InputMode) -> bool {
        if family(mode).is_none() {
            return true;
        }
        if self.mutation_editor.is_some()
            && self.editor_snapshot(mode).as_ref() == self.mutation_editor.as_ref()
        {
            return true;
        }
        self.refuse_editor_target();
        false
    }

    fn refuse_editor_target(&mut self) {
        self.mutation_notice = Some(local_text(
            self.language,
            "editing target changed or disappeared; draft retained, no operation planned; cancel and reopen to review",
            "编辑目标已变化或消失；已保留草稿，未创建操作计划，请取消并重新打开确认",
        ).into());
    }

    pub(super) fn local_target_live(&self, target: &SourceRef) -> bool {
        match target.kind {
            SourceKind::CodexThread => self
                .thread_by_id(&ThreadId::new(target.value.clone()))
                .is_some(),
            SourceKind::ScratchWork => self
                .planning_snapshot
                .scratch
                .iter()
                .any(|item| item.id == target.value),
            _ => self.work_cards.iter().any(|card| card.anchor == *target),
        }
    }
}
