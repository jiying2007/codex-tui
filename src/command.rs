#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    QuitOrInterrupt,
    Back,
    Help,
    CommandPalette,
    Search,
    ToggleHostLocalFilter,
    ToggleRepoBackedFilter,
    ToggleAllHistory,
    ContextActions,
    Next,
    Previous,
    Open,
    NextAttention,
    QuickPrompt,
    Board,
    BoardLeft,
    BoardRight,
    CycleSavedView,
    Review,
    Workspace,
    ManagedWorktrees,
    CreateWorktree,
    AdoptWorktree,
    RemoveWorktree,
    DeleteBranch,
    ConfirmOperation,
    CancelOperation,
    Snooze,
    MarkUnread,
    TogglePin,
    EditAlias,
    AcknowledgeAttention,
    ApprovePending,
    DeclinePending,
    CancelPending,
    AnswerPending,
    New,
    Goal,
    PageUp,
    PageDown,
    ToggleWordDiff,
    ExternalEditor,
    OpenExternal,
    TerminalDrawer,
    CloseTerminalDrawer,
    HotSlot(u8),
    BeginHotSlotBind,
}

impl Command {
    pub const fn palette_label(self, simplified_chinese: bool) -> Option<&'static str> {
        Some(match (self, simplified_chinese) {
            (Self::Search, false) => "Search",
            (Self::Search, true) => "搜索",
            (Self::NextAttention, false) => "Next attention",
            (Self::NextAttention, true) => "下一个待处理",
            (Self::QuickPrompt, false) => "Quick Prompt",
            (Self::QuickPrompt, true) => "快速消息",
            (Self::Board, false) => "Open Board",
            (Self::Board, true) => "打开看板",
            (Self::Review, false) => "Open Review",
            (Self::Review, true) => "打开评审",
            (Self::Workspace, false) => "Open Workspace",
            (Self::Workspace, true) => "打开工作区",
            (Self::ManagedWorktrees, false) => "Managed Worktrees",
            (Self::ManagedWorktrees, true) => "受管 Worktrees",
            (Self::New, false) => "New Scratch",
            (Self::New, true) => "新建 Scratch",
            (Self::Goal, false) => "Goal actions",
            (Self::Goal, true) => "Goal 操作",
            (Self::TogglePin, false) => "Toggle pin",
            (Self::TogglePin, true) => "切换固定",
            (Self::Snooze, false) => "Snooze",
            (Self::Snooze, true) => "稍后提醒",
            (Self::ContextActions, false) => "Context actions",
            (Self::ContextActions, true) => "上下文操作",
            (Self::TerminalDrawer, false) => "Open Terminal Drawer",
            (Self::TerminalDrawer, true) => "打开终端抽屉",
            (Self::CloseTerminalDrawer, false) => "Close Terminal Drawer",
            (Self::CloseTerminalDrawer, true) => "关闭终端抽屉",
            (Self::Help, false) => "Help",
            (Self::Help, true) => "帮助",
            _ => return None,
        })
    }

    pub const fn palette_capable(self) -> bool {
        self.palette_label(false).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_capability_has_labels_in_both_languages() {
        for command in [
            Command::Search,
            Command::NextAttention,
            Command::QuickPrompt,
            Command::Board,
            Command::Review,
            Command::Workspace,
            Command::ManagedWorktrees,
            Command::New,
            Command::Goal,
            Command::TogglePin,
            Command::Snooze,
            Command::ContextActions,
            Command::TerminalDrawer,
            Command::CloseTerminalDrawer,
            Command::Help,
        ] {
            assert!(command.palette_capable());
            assert!(command.palette_label(false).is_some());
            assert!(command.palette_label(true).is_some());
        }
    }
}
