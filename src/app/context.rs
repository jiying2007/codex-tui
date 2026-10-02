use super::{AppState, ContextChoice, View};
use crate::planning::{SourceKind, SourceRef};

fn state_has_linkable_hot_slot(state: &AppState, anchor: &SourceRef) -> bool {
    state
        .planning_snapshot
        .hot_slots
        .iter()
        .any(|slot| slot.target != *anchor)
}

impl AppState {
    pub fn context_choices(&self) -> Vec<ContextChoice> {
        let mut choices = Vec::new();
        if let Some(target) = self.selected_local_target() {
            choices.extend([
                ContextChoice::Snooze,
                ContextChoice::EditNote,
                ContextChoice::Bookmark,
            ]);
            if matches!(self.view, View::Registry | View::Board)
                && state_has_linkable_hot_slot(self, &target)
            {
                choices.push(ContextChoice::LinkHotSlot);
            }
            if target.kind == SourceKind::ScratchWork {
                choices.extend([
                    ContextChoice::ScratchInbox,
                    ContextChoice::ScratchReady,
                    ContextChoice::ScratchDone,
                    ContextChoice::DeleteScratch,
                ]);
            }
        }
        if self.lifecycle_thread_id().is_some() {
            if self.lifecycle_cwd().is_some() && self.lifecycle_capability_available("thread/start")
            {
                choices.push(ContextChoice::NewCodexThread);
            }
            if self.lifecycle_capability_available("thread/fork") {
                choices.push(ContextChoice::ForkCodexThread);
            }
        }
        if matches!(self.view, View::Board) {
            if !self.visible_planning_cards().is_empty() {
                choices.extend([
                    ContextChoice::BatchAddTag,
                    ContextChoice::BatchRemoveTag,
                    ContextChoice::BatchSetPriority,
                    ContextChoice::BatchClearPriority,
                    ContextChoice::BatchMarkReady,
                    ContextChoice::BatchClearReady,
                    ContextChoice::BatchMarkDone,
                    ContextChoice::BatchReopen,
                    ContextChoice::BatchSnooze,
                    ContextChoice::BatchClearSnooze,
                ]);
            }
            choices.push(ContextChoice::SaveCurrentView);
            if self.active_saved_view().id.starts_with("view:") {
                choices.extend([
                    ContextChoice::EditViewName,
                    ContextChoice::EditViewSource,
                    ContextChoice::EditViewFilter,
                    ContextChoice::EditViewLayout,
                    ContextChoice::EditViewGroup,
                    ContextChoice::EditViewOrder,
                    ContextChoice::EditViewFields,
                    ContextChoice::DeleteCurrentView,
                ]);
            }
        }
        if matches!(self.view, View::Workspace(_) | View::Review(_))
            && self.current_thread_id().is_some_and(|thread_id| {
                self.git_context(thread_id)
                    .is_some_and(|context| context.repo.is_some())
            })
        {
            choices.push(ContextChoice::LaunchPreset);
        }
        if let Some(target) = self.current_forge_mutation_target() {
            if target.change_request.is_some() {
                choices.extend([
                    ContextChoice::ForgeComment,
                    ContextChoice::ForgeApprove,
                    ContextChoice::ForgeMerge,
                ]);
            } else if target
                .identity
                .default_branch
                .as_deref()
                .is_some_and(|default_branch| default_branch != target.branch)
            {
                choices.push(ContextChoice::ForgeCreateMergeRequest);
            }
        }
        choices
    }

    pub fn context_choice(&self) -> Option<ContextChoice> {
        self.context_choices().get(self.context_selected).copied()
    }
}
