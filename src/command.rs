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
        let label = match self {
            Self::Search => {
                if simplified_chinese { "搜索" } else { "Search" }
            }
            Self::NextAttention => {
                if simplified_chinese { "下一个待处理" } else { "Next attention" }
            }
            Self::QuickPrompt => {
                if simplified_chinese { "快速消息" } else { "Quick Prompt" }
            }
            Self::Board => {
                if simplified_chinese { "打开看板" } else { "Open Board" }
            }
            Self::Review => {
                if simplified_chinese { "打开评审" } else { "Open Review" }
            }
            Self::Workspace => {
                if simplified_chinese { "打开工作区" } else { "Open Workspace" }
            }
            Self::ManagedWorktrees => {
                if simplified_chinese { "受管 Worktrees" } else { "Managed Worktrees" }
            }
            Self::New => {
                if simplified_chinese { "新建 Scratch" } else { "New Scratch" }
            }
            Self::Goal => {
                if simplified_chinese { "Goal 操作" } else { "Goal actions" }
            }
            Self::TogglePin => {
                if simplified_chinese { "切换固定" } else { "Toggle pin" }
            }
            Self::Snooze => {
                if simplified_chinese { "稍后提醒" } else { "Snooze" }
            }
            Self::ContextActions => {
                if simplified_chinese { "上下文操作" } else { "Context actions" }
            }
            Self::TerminalDrawer => {
                if simplified_chinese { "打开终端抽屉" } else { "Open Terminal Drawer" }
            }
            Self::CloseTerminalDrawer => {
                if simplified_chinese { "关闭终端抽屉" } else { "Close Terminal Drawer" }
            }
            Self::Help => {
                if simplified_chinese { "帮助" } else { "Help" }
            }
            _ => return None,
        };
        Some(label)
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
