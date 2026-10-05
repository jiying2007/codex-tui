//! Keep an open menu's action and target stable while observations are refreshed.
use super::{AppState, ContextChoice, View, local_text, mutation_editor::MutationEditor};
use crate::planning::SourceRef;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Scope {
    view: View,
    target: Option<SourceRef>,
    saved_view: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct ContextMenu {
    pub(super) choices: Vec<ContextChoice>,
    scope: Scope,
    forge: Option<MutationEditor>,
    worktree: Option<MutationEditor>,
}

impl AppState {
    fn context_scope(&self) -> Scope {
        Scope {
            view: self.view.clone(),
            target: self.selected_local_target(),
            saved_view: matches!(self.view, View::Board).then(|| self.active_saved_view().id),
        }
    }

    pub(super) fn open_context_menu(&mut self) {
        let choices = self.build_context_choices();
        self.context_menu = (!choices.is_empty()).then(|| ContextMenu {
            choices,
            scope: self.context_scope(),
            forge: self.editor_snapshot(super::InputMode::ForgeComment),
            worktree: self.editor_snapshot(super::InputMode::WorktreeCreateBranch),
        });
        self.context_open = self.context_menu.is_some();
        self.context_selected = 0;
    }

    pub(super) fn close_context_menu(&mut self) {
        self.context_menu = None;
        self.context_open = false;
        self.context_selected = 0;
    }

    pub(super) fn take_context_choice(&mut self) -> Option<ContextChoice> {
        let choice = self.context_choice();
        // Without an open menu this is an immediate reducer action, not a
        // delayed menu selection. Existing programmatic callers use live state.
        let valid = self.context_menu.as_ref().is_none_or(|menu| {
            menu.scope == self.context_scope()
                && menu
                    .scope
                    .target
                    .as_ref()
                    .is_none_or(|target| self.local_target_live(target))
                && choice.is_some_and(|choice| {
                    self.build_context_choices().contains(&choice)
                        && match choice {
                            ContextChoice::ForgeCreateMergeRequest
                            | ContextChoice::ForgeComment
                            | ContextChoice::ForgeApprove
                            | ContextChoice::ForgeMerge => {
                                menu.forge.is_some()
                                    && menu.forge
                                        == self.editor_snapshot(super::InputMode::ForgeComment)
                            }
                            ContextChoice::LaunchPreset => {
                                menu.worktree.is_some()
                                    && menu.worktree
                                        == self
                                            .editor_snapshot(super::InputMode::WorktreeCreateBranch)
                            }
                            _ => true,
                        }
                })
        });
        self.close_context_menu();
        if !valid {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "context target or action changed; no operation started; reopen the menu",
                    "上下文目标或操作已变化；未启动操作，请重新打开菜单",
                )
                .into(),
            );
            return None;
        }
        choice
    }
}
