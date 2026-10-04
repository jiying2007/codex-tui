use codex_tui::{
    app::{Action, AppState, Effect, ViewKind, reduce},
    command::Command,
    conversation::InteractiveResolution,
};

pub(crate) fn action_for_command(app: &AppState, command: Command) -> Option<Action> {
    let action = match command {
        Command::QuitOrInterrupt => match app.view_kind() {
            ViewKind::Registry => Action::Quit,
            ViewKind::Thread => Action::InterruptCurrent,
            ViewKind::Review
            | ViewKind::Workspace
            | ViewKind::ManagedWorktrees
            | ViewKind::Board
            | ViewKind::Scratch => Action::Back,
        },
        Command::Back => Action::Back,
        Command::Help => Action::ToggleHelp,
        Command::Search => Action::BeginSearch,
        Command::TranscriptSearch => Action::BeginTranscriptSearch,
        Command::ThreadQueue => Action::OpenThreadQueue,
        Command::ToggleHostLocalFilter => Action::ToggleHostLocalFilter,
        Command::ToggleRepoBackedFilter => Action::ToggleRepoBackedFilter,
        Command::ToggleAllHistory => Action::ToggleAllHistory,
        Command::Next => match app.view_kind() {
            ViewKind::Review => Action::MoveReview(1),
            ViewKind::ManagedWorktrees => Action::MoveManagedWorktree(1),
            ViewKind::Board => Action::MovePlanningSelection(1),
            _ => Action::MoveSelection(1),
        },
        Command::Previous => match app.view_kind() {
            ViewKind::Review => Action::MoveReview(-1),
            ViewKind::ManagedWorktrees => Action::MoveManagedWorktree(-1),
            ViewKind::Board => Action::MovePlanningSelection(-1),
            _ => Action::MoveSelection(-1),
        },
        Command::Open => {
            if app.view_kind() == ViewKind::Board {
                Action::OpenPlanningSelected
            } else {
                Action::OpenSelected
            }
        }
        Command::NextAttention => Action::NextAttention,
        Command::QuickPrompt => Action::QuickPrompt,
        Command::MarkUnread => Action::MarkUnread,
        Command::TogglePin => Action::TogglePin,
        Command::EditAlias => Action::BeginAlias,
        Command::AcknowledgeAttention => Action::AcknowledgeAttention,
        Command::ApprovePending => Action::ResolvePending(InteractiveResolution::Accept),
        Command::DeclinePending => Action::ResolvePending(InteractiveResolution::Decline),
        Command::CancelPending => Action::ResolvePending(InteractiveResolution::Cancel),
        Command::AnswerPending => Action::BeginUserInput,
        Command::Board => Action::OpenBoard,
        Command::BoardLeft => Action::MoveBoardColumn(-1),
        Command::BoardRight => Action::MoveBoardColumn(1),
        Command::CycleSavedView => Action::CycleSavedView(1),
        Command::Review => Action::OpenReview,
        Command::Workspace => Action::OpenWorkspace,
        Command::ManagedWorktrees => Action::OpenManagedWorktrees,
        Command::CreateWorktree => Action::BeginCreateWorktree,
        Command::AdoptWorktree => Action::BeginAdoptCurrentWorktree,
        Command::RemoveWorktree => Action::BeginRemoveManagedWorktree,
        Command::DeleteBranch => Action::BeginDeleteBranch,
        Command::ConfirmOperation => Action::ConfirmPendingOperation,
        Command::CancelOperation => Action::CancelPendingOperation,
        Command::New => Action::BeginScratch,
        Command::Snooze => Action::BeginSnooze,
        Command::BeginHotSlotBind => Action::BeginHotSlotBind,
        Command::PageUp => {
            if app.view_kind() == ViewKind::Review {
                Action::ScrollReviewBy(-10)
            } else {
                Action::ScrollBy(-5)
            }
        }
        Command::PageDown => {
            if app.view_kind() == ViewKind::Review {
                Action::ScrollReviewBy(10)
            } else {
                Action::ScrollBy(5)
            }
        }
        Command::ToggleWordDiff => Action::ToggleReviewWordDiff,
        Command::ExternalEditor => Action::OpenReviewExternalEditor,
        Command::TerminalDrawer => Action::ToggleTerminalDrawer,
        Command::CloseTerminalDrawer => Action::CloseTerminalDrawer,
        Command::HotSlot(slot) => Action::UseHotSlot(slot),
        Command::ContextActions => Action::OpenContext,
        Command::Goal => Action::OpenGoalActions,
        Command::CommandPalette => Action::OpenCommandPalette,
        Command::OpenExternal => Action::OpenReviewExternal,
    };
    Some(action)
}

pub(crate) fn handle_command(app: &mut AppState, command: Command) -> Vec<Effect> {
    let Some(action) = action_for_command(app, command) else {
        return vec![];
    };
    reduce(app, action)
}
