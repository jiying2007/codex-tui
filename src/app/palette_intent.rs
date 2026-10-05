//! Scope captured at palette open; never resolve a stale command on a new item.
use super::{AppState, View, local_text};
use crate::{
    command::Command,
    domain::{LocalRepoIdentity, ThreadId},
    planning::{SourceKind, SourceRef},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PaletteIntent {
    view: View,
    target: Option<SourceRef>,
    cwd: Option<String>,
    workspace: Option<String>,
    saved_view: Option<String>,
    repository: Option<LocalRepoIdentity>,
}

impl AppState {
    pub(super) fn palette_intent(&self) -> PaletteIntent {
        let target = self.selected_local_target();
        let thread = target
            .as_ref()
            .filter(|target| target.kind == SourceKind::CodexThread)
            .and_then(|target| self.thread_by_id(&ThreadId::new(target.value.clone())));
        let workspace = match target.as_ref() {
            Some(target) if target.kind == SourceKind::CodexThread => {
                thread.map(|thread| thread.workspace.clone())
            }
            Some(target) if target.kind == SourceKind::ScratchWork => self
                .planning_snapshot
                .scratch
                .iter()
                .find(|item| item.id == target.value)
                .and_then(|item| item.workspace.clone()),
            _ => self
                .selected_planning_card()
                .and_then(|card| card.workspace.clone()),
        };
        PaletteIntent {
            view: self.view.clone(),
            target,
            cwd: thread.map(|thread| thread.metadata.cwd.clone()),
            workspace,
            saved_view: matches!(self.view, View::Board).then(|| self.active_saved_view().id),
            repository: thread
                .and_then(|thread| {
                    self.git_context(&thread.id).filter(|context| {
                        context.cwd == thread.metadata.cwd && context.error.is_none()
                    })
                })
                .and_then(|context| context.repo.clone()),
        }
    }

    /// Resolve and consume the displayed command while its open-time intent exists.
    /// An empty fuzzy result stays open; stale scoped choices close without effects.
    pub fn take_command_palette_choice(&mut self) -> Option<Command> {
        if !self.command_palette_open {
            return None;
        }
        let choice = self.command_palette_choice()?;
        let valid = self.build_command_palette_choices().contains(&choice)
            && self
                .command_palette_intent
                .as_ref()
                .is_some_and(|original| {
                    if matches!(
                        choice,
                        Command::Help
                            | Command::Search
                            | Command::TranscriptSearch
                            | Command::Board
                            | Command::CloseTerminalDrawer
                    ) {
                        return true;
                    }
                    let current = self.palette_intent();
                    original.view == current.view
                        && original.target == current.target
                        && original.cwd == current.cwd
                        && original.saved_view == current.saved_view
                        && original
                            .target
                            .as_ref()
                            .is_none_or(|target| self.local_target_live(target))
                        && (!matches!(choice, Command::New | Command::ContextActions)
                            || original.workspace == current.workspace)
                        && (!matches!(
                            choice,
                            Command::Review
                                | Command::Workspace
                                | Command::ManagedWorktrees
                                | Command::TerminalDrawer
                                | Command::ContextActions
                        ) || original.repository == current.repository)
                });
        self.close_command_palette();
        if valid {
            Some(choice)
        } else {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "command target or availability changed; nothing executed; reopen the palette",
                    "命令目标或可用性已变化；未执行任何操作，请重新打开命令面板",
                )
                .into(),
            );
            None
        }
    }
}
