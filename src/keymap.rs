use crate::{app::ViewKind, command::Command};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpScope {
    AllViews,
    NonScratch,
    View(ViewKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelpBinding {
    pub surface: &'static str,
    pub scope: HelpScope,
    pub token: &'static str,
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub command: Command,
}

#[rustfmt::skip]
pub const HELP_BINDINGS: &[HelpBinding] = &[
    HelpBinding { surface: "Global", scope: HelpScope::AllViews, token: "?", code: KeyCode::Char('?'), modifiers: KeyModifiers::NONE, command: Command::Help },
    HelpBinding { surface: "Global", scope: HelpScope::AllViews, token: "Ctrl+K", code: KeyCode::Char('k'), modifiers: KeyModifiers::CONTROL, command: Command::CommandPalette },
    HelpBinding { surface: "Global", scope: HelpScope::AllViews, token: "/", code: KeyCode::Char('/'), modifiers: KeyModifiers::NONE, command: Command::Search },
    HelpBinding { surface: "Global", scope: HelpScope::AllViews, token: "Ctrl+F", code: KeyCode::Char('f'), modifiers: KeyModifiers::CONTROL, command: Command::TranscriptSearch },
    HelpBinding { surface: "Global", scope: HelpScope::AllViews, token: ".", code: KeyCode::Char('.'), modifiers: KeyModifiers::NONE, command: Command::ContextActions },
    HelpBinding { surface: "Global", scope: HelpScope::AllViews, token: "Esc", code: KeyCode::Esc, modifiers: KeyModifiers::NONE, command: Command::Back },
    HelpBinding { surface: "Terminal drawer", scope: HelpScope::NonScratch, token: "t", code: KeyCode::Char('t'), modifiers: KeyModifiers::NONE, command: Command::TerminalDrawer },
    HelpBinding { surface: "Terminal drawer", scope: HelpScope::NonScratch, token: "T", code: KeyCode::Char('T'), modifiers: KeyModifiers::NONE, command: Command::CloseTerminalDrawer },
    HelpBinding { surface: "Registry", scope: HelpScope::View(ViewKind::Registry), token: "j/k", code: KeyCode::Char('j'), modifiers: KeyModifiers::NONE, command: Command::Next },
    HelpBinding { surface: "Registry", scope: HelpScope::View(ViewKind::Registry), token: "j/k", code: KeyCode::Char('k'), modifiers: KeyModifiers::NONE, command: Command::Previous },
    HelpBinding { surface: "Registry", scope: HelpScope::View(ViewKind::Registry), token: "Enter", code: KeyCode::Enter, modifiers: KeyModifiers::NONE, command: Command::Open },
    HelpBinding { surface: "Registry", scope: HelpScope::View(ViewKind::Registry), token: "Space", code: KeyCode::Char(' '), modifiers: KeyModifiers::NONE, command: Command::NextAttention },
    HelpBinding { surface: "Registry", scope: HelpScope::View(ViewKind::Registry), token: "l", code: KeyCode::Char('l'), modifiers: KeyModifiers::NONE, command: Command::ToggleHostLocalFilter },
    HelpBinding { surface: "Registry", scope: HelpScope::View(ViewKind::Registry), token: "g", code: KeyCode::Char('g'), modifiers: KeyModifiers::NONE, command: Command::ToggleRepoBackedFilter },
    HelpBinding { surface: "Registry", scope: HelpScope::View(ViewKind::Registry), token: "h", code: KeyCode::Char('h'), modifiers: KeyModifiers::NONE, command: Command::ToggleAllHistory },
    HelpBinding { surface: "Registry", scope: HelpScope::View(ViewKind::Registry), token: "p", code: KeyCode::Char('p'), modifiers: KeyModifiers::NONE, command: Command::TogglePin },
    HelpBinding { surface: "Registry", scope: HelpScope::View(ViewKind::Registry), token: "e", code: KeyCode::Char('e'), modifiers: KeyModifiers::NONE, command: Command::EditAlias },
    HelpBinding { surface: "Registry", scope: HelpScope::View(ViewKind::Registry), token: "x", code: KeyCode::Char('x'), modifiers: KeyModifiers::NONE, command: Command::AcknowledgeAttention },
    HelpBinding { surface: "Thread", scope: HelpScope::View(ViewKind::Thread), token: "a", code: KeyCode::Char('a'), modifiers: KeyModifiers::NONE, command: Command::QuickPrompt },
    HelpBinding { surface: "Thread", scope: HelpScope::View(ViewKind::Thread), token: "y/n/c", code: KeyCode::Char('y'), modifiers: KeyModifiers::NONE, command: Command::ApprovePending },
    HelpBinding { surface: "Thread", scope: HelpScope::View(ViewKind::Thread), token: "y/n/c", code: KeyCode::Char('n'), modifiers: KeyModifiers::NONE, command: Command::DeclinePending },
    HelpBinding { surface: "Thread", scope: HelpScope::View(ViewKind::Thread), token: "y/n/c", code: KeyCode::Char('c'), modifiers: KeyModifiers::NONE, command: Command::CancelPending },
    HelpBinding { surface: "Thread", scope: HelpScope::View(ViewKind::Thread), token: "i", code: KeyCode::Char('i'), modifiers: KeyModifiers::NONE, command: Command::AnswerPending },
    HelpBinding { surface: "Thread", scope: HelpScope::View(ViewKind::Thread), token: "Ctrl+C", code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, command: Command::QuitOrInterrupt },
    HelpBinding { surface: "Thread", scope: HelpScope::View(ViewKind::Thread), token: "r", code: KeyCode::Char('r'), modifiers: KeyModifiers::NONE, command: Command::Review },
    HelpBinding { surface: "Review", scope: HelpScope::View(ViewKind::Review), token: "j/k", code: KeyCode::Char('j'), modifiers: KeyModifiers::NONE, command: Command::Next },
    HelpBinding { surface: "Review", scope: HelpScope::View(ViewKind::Review), token: "j/k", code: KeyCode::Char('k'), modifiers: KeyModifiers::NONE, command: Command::Previous },
    HelpBinding { surface: "Review", scope: HelpScope::View(ViewKind::Review), token: "w", code: KeyCode::Char('w'), modifiers: KeyModifiers::NONE, command: Command::ToggleWordDiff },
    HelpBinding { surface: "Review", scope: HelpScope::View(ViewKind::Review), token: "e", code: KeyCode::Char('e'), modifiers: KeyModifiers::NONE, command: Command::ExternalEditor },
    HelpBinding { surface: "Review", scope: HelpScope::View(ViewKind::Review), token: ".", code: KeyCode::Char('.'), modifiers: KeyModifiers::NONE, command: Command::ContextActions },
    HelpBinding { surface: "Review", scope: HelpScope::View(ViewKind::Review), token: "PageUp/PageDown", code: KeyCode::PageUp, modifiers: KeyModifiers::NONE, command: Command::PageUp },
    HelpBinding { surface: "Review", scope: HelpScope::View(ViewKind::Review), token: "PageUp/PageDown", code: KeyCode::PageDown, modifiers: KeyModifiers::NONE, command: Command::PageDown },
    HelpBinding { surface: "Review", scope: HelpScope::View(ViewKind::Review), token: "Esc", code: KeyCode::Esc, modifiers: KeyModifiers::NONE, command: Command::Back },
    HelpBinding { surface: "Workspace", scope: HelpScope::View(ViewKind::Workspace), token: ".", code: KeyCode::Char('.'), modifiers: KeyModifiers::NONE, command: Command::ContextActions },
    HelpBinding { surface: "Workspace", scope: HelpScope::View(ViewKind::Workspace), token: "r", code: KeyCode::Char('r'), modifiers: KeyModifiers::NONE, command: Command::Review },
    HelpBinding { surface: "Workspace", scope: HelpScope::View(ViewKind::Workspace), token: "m", code: KeyCode::Char('m'), modifiers: KeyModifiers::NONE, command: Command::ManagedWorktrees },
    HelpBinding { surface: "Workspace", scope: HelpScope::View(ViewKind::Workspace), token: "Esc", code: KeyCode::Esc, modifiers: KeyModifiers::NONE, command: Command::Back },
    HelpBinding { surface: "Managed Worktrees", scope: HelpScope::View(ViewKind::ManagedWorktrees), token: "n", code: KeyCode::Char('n'), modifiers: KeyModifiers::NONE, command: Command::CreateWorktree },
    HelpBinding { surface: "Managed Worktrees", scope: HelpScope::View(ViewKind::ManagedWorktrees), token: "a", code: KeyCode::Char('a'), modifiers: KeyModifiers::NONE, command: Command::AdoptWorktree },
    HelpBinding { surface: "Managed Worktrees", scope: HelpScope::View(ViewKind::ManagedWorktrees), token: "d", code: KeyCode::Char('d'), modifiers: KeyModifiers::NONE, command: Command::RemoveWorktree },
    HelpBinding { surface: "Managed Worktrees", scope: HelpScope::View(ViewKind::ManagedWorktrees), token: "x", code: KeyCode::Char('x'), modifiers: KeyModifiers::NONE, command: Command::DeleteBranch },
    HelpBinding { surface: "Managed Worktrees", scope: HelpScope::View(ViewKind::ManagedWorktrees), token: "y", code: KeyCode::Char('y'), modifiers: KeyModifiers::NONE, command: Command::ConfirmOperation },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "h/l", code: KeyCode::Char('h'), modifiers: KeyModifiers::NONE, command: Command::BoardLeft },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "h/l", code: KeyCode::Char('l'), modifiers: KeyModifiers::NONE, command: Command::BoardRight },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "j/k", code: KeyCode::Char('j'), modifiers: KeyModifiers::NONE, command: Command::Next },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "j/k", code: KeyCode::Char('k'), modifiers: KeyModifiers::NONE, command: Command::Previous },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "Space", code: KeyCode::Char(' '), modifiers: KeyModifiers::NONE, command: Command::NextAttention },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "s", code: KeyCode::Char('s'), modifiers: KeyModifiers::NONE, command: Command::Snooze },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "=", code: KeyCode::Char('='), modifiers: KeyModifiers::NONE, command: Command::BeginHotSlotBind },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "1–9", code: KeyCode::Char('1'), modifiers: KeyModifiers::NONE, command: Command::HotSlot(1) },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "1–9", code: KeyCode::Char('9'), modifiers: KeyModifiers::NONE, command: Command::HotSlot(9) },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "Tab", code: KeyCode::Tab, modifiers: KeyModifiers::NONE, command: Command::CycleSavedView },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: ".", code: KeyCode::Char('.'), modifiers: KeyModifiers::NONE, command: Command::ContextActions },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "Enter", code: KeyCode::Enter, modifiers: KeyModifiers::NONE, command: Command::Open },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "a", code: KeyCode::Char('a'), modifiers: KeyModifiers::NONE, command: Command::QuickPrompt },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "n", code: KeyCode::Char('n'), modifiers: KeyModifiers::NONE, command: Command::New },
    HelpBinding { surface: "Scratch", scope: HelpScope::View(ViewKind::Scratch), token: "Esc", code: KeyCode::Esc, modifiers: KeyModifiers::NONE, command: Command::Back },
];

pub fn command_for_key(key: KeyEvent, view: ViewKind) -> Option<Command> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
        return Some(Command::QuitOrInterrupt);
    }
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('k') {
        return Some(Command::CommandPalette);
    }
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('f') {
        return Some(Command::TranscriptSearch);
    }

    match (view, key.code) {
        (_, KeyCode::Char('?')) => Some(Command::Help),
        (_, KeyCode::Char('/')) => Some(Command::Search),
        (_, KeyCode::Char('.')) => Some(Command::ContextActions),
        (
            ViewKind::Registry
            | ViewKind::Thread
            | ViewKind::Review
            | ViewKind::Workspace
            | ViewKind::ManagedWorktrees
            | ViewKind::Board,
            KeyCode::Char('t'),
        ) => Some(Command::TerminalDrawer),
        (
            ViewKind::Registry
            | ViewKind::Thread
            | ViewKind::Review
            | ViewKind::Workspace
            | ViewKind::ManagedWorktrees
            | ViewKind::Board,
            KeyCode::Char('T'),
        ) => Some(Command::CloseTerminalDrawer),
        (_, KeyCode::Esc) => Some(Command::Back),
        (ViewKind::Registry, KeyCode::Char('q')) => Some(Command::QuitOrInterrupt),
        (ViewKind::Registry, KeyCode::Char('l')) => Some(Command::ToggleHostLocalFilter),
        (ViewKind::Registry, KeyCode::Char('g')) => Some(Command::ToggleRepoBackedFilter),
        (ViewKind::Registry, KeyCode::Char('h')) => Some(Command::ToggleAllHistory),
        (ViewKind::Registry, KeyCode::Char('j') | KeyCode::Down) => Some(Command::Next),
        (ViewKind::Registry, KeyCode::Char('k') | KeyCode::Up) => Some(Command::Previous),
        (ViewKind::Registry, KeyCode::Enter) => Some(Command::Open),
        (ViewKind::Registry, KeyCode::Char(' ')) => Some(Command::NextAttention),
        (ViewKind::Registry, KeyCode::Char('a')) => Some(Command::QuickPrompt),
        (ViewKind::Registry, KeyCode::Char('b')) => Some(Command::Board),
        (ViewKind::Registry, KeyCode::Char('r')) => Some(Command::Review),
        (ViewKind::Registry, KeyCode::Char('w')) => Some(Command::Workspace),
        (ViewKind::Registry, KeyCode::Char('s')) => Some(Command::Snooze),
        (ViewKind::Registry, KeyCode::Char('u')) => Some(Command::MarkUnread),
        (ViewKind::Registry, KeyCode::Char('p')) => Some(Command::TogglePin),
        (ViewKind::Registry, KeyCode::Char('e')) => Some(Command::EditAlias),
        (ViewKind::Registry, KeyCode::Char('x')) => Some(Command::AcknowledgeAttention),
        (ViewKind::Registry, KeyCode::Char('n')) => Some(Command::New),
        (ViewKind::Registry, KeyCode::Char('=')) => Some(Command::BeginHotSlotBind),
        (ViewKind::Registry, KeyCode::Char(c @ '1'..='9')) => {
            Some(Command::HotSlot(c.to_digit(10)? as u8))
        }
        (ViewKind::Thread, KeyCode::Char('a')) => Some(Command::QuickPrompt),
        (ViewKind::Thread, KeyCode::Char('b')) => Some(Command::Board),
        (ViewKind::Thread, KeyCode::Char('y')) => Some(Command::ApprovePending),
        (ViewKind::Thread, KeyCode::Char('n')) => Some(Command::DeclinePending),
        (ViewKind::Thread, KeyCode::Char('c')) => Some(Command::CancelPending),
        (ViewKind::Thread, KeyCode::Char('i')) => Some(Command::AnswerPending),
        (ViewKind::Thread, KeyCode::Char('r')) => Some(Command::Review),
        (ViewKind::Thread, KeyCode::Char('w')) => Some(Command::Workspace),
        (ViewKind::Thread, KeyCode::Char('m')) => Some(Command::ManagedWorktrees),
        (ViewKind::Thread, KeyCode::Char('g')) => Some(Command::Goal),
        (ViewKind::Thread, KeyCode::PageUp) => Some(Command::PageUp),
        (ViewKind::Thread, KeyCode::PageDown) => Some(Command::PageDown),
        (ViewKind::Review, KeyCode::Char('j') | KeyCode::Down) => Some(Command::Next),
        (ViewKind::Review, KeyCode::Char('k') | KeyCode::Up) => Some(Command::Previous),
        (ViewKind::Review, KeyCode::PageUp) => Some(Command::PageUp),
        (ViewKind::Review, KeyCode::PageDown) => Some(Command::PageDown),
        (ViewKind::Review, KeyCode::Char('w')) => Some(Command::ToggleWordDiff),
        (ViewKind::Review, KeyCode::Char('e')) => Some(Command::ExternalEditor),
        (ViewKind::Workspace, KeyCode::Char('r')) => Some(Command::Review),
        (ViewKind::Workspace, KeyCode::Char('m')) => Some(Command::ManagedWorktrees),
        (ViewKind::ManagedWorktrees, KeyCode::Char('j') | KeyCode::Down) => Some(Command::Next),
        (ViewKind::ManagedWorktrees, KeyCode::Char('k') | KeyCode::Up) => Some(Command::Previous),
        (ViewKind::ManagedWorktrees, KeyCode::Char('n')) => Some(Command::CreateWorktree),
        (ViewKind::ManagedWorktrees, KeyCode::Char('a')) => Some(Command::AdoptWorktree),
        (ViewKind::ManagedWorktrees, KeyCode::Char('d')) => Some(Command::RemoveWorktree),
        (ViewKind::ManagedWorktrees, KeyCode::Char('x')) => Some(Command::DeleteBranch),
        (ViewKind::ManagedWorktrees, KeyCode::Char('y')) => Some(Command::ConfirmOperation),
        (ViewKind::ManagedWorktrees, KeyCode::Char('c')) => Some(Command::CancelOperation),
        (ViewKind::Board, KeyCode::Char('h') | KeyCode::Left) => Some(Command::BoardLeft),
        (ViewKind::Board, KeyCode::Char('l') | KeyCode::Right) => Some(Command::BoardRight),
        (ViewKind::Board, KeyCode::Char('j') | KeyCode::Down) => Some(Command::Next),
        (ViewKind::Board, KeyCode::Char('k') | KeyCode::Up) => Some(Command::Previous),
        (ViewKind::Board, KeyCode::Enter) => Some(Command::Open),
        (ViewKind::Board, KeyCode::Char(' ')) => Some(Command::NextAttention),
        (ViewKind::Board, KeyCode::Char('a')) => Some(Command::QuickPrompt),
        (ViewKind::Board, KeyCode::Char('n')) => Some(Command::New),
        (ViewKind::Board, KeyCode::Char('s')) => Some(Command::Snooze),
        (ViewKind::Board, KeyCode::Char('=')) => Some(Command::BeginHotSlotBind),
        (ViewKind::Board, KeyCode::Char(c @ '1'..='9')) => {
            Some(Command::HotSlot(c.to_digit(10)? as u8))
        }
        (ViewKind::Board, KeyCode::Char('r')) => Some(Command::Review),
        (ViewKind::Board, KeyCode::Char('w')) => Some(Command::Workspace),
        (ViewKind::Board, KeyCode::Tab) => Some(Command::CycleSavedView),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn views_for_scope(scope: HelpScope) -> Vec<ViewKind> {
        match scope {
            HelpScope::AllViews => vec![
                ViewKind::Registry,
                ViewKind::Thread,
                ViewKind::Review,
                ViewKind::Workspace,
                ViewKind::ManagedWorktrees,
                ViewKind::Board,
                ViewKind::Scratch,
            ],
            HelpScope::NonScratch => vec![
                ViewKind::Registry,
                ViewKind::Thread,
                ViewKind::Review,
                ViewKind::Workspace,
                ViewKind::ManagedWorktrees,
                ViewKind::Board,
            ],
            HelpScope::View(view) => vec![view],
        }
    }

    #[test]
    fn help_advertised_bindings_resolve_to_the_declared_command() {
        for binding in HELP_BINDINGS {
            for view in views_for_scope(binding.scope) {
                let key = KeyEvent::new(binding.code, binding.modifiers);
                assert_eq!(
                    command_for_key(key, view),
                    Some(binding.command),
                    "help token {} drifted in {view:?}",
                    binding.token
                );
            }
        }
    }

    #[test]
    fn global_keyboard_surfaces_are_reachable_without_pointer_input() {
        for view in [
            ViewKind::Registry,
            ViewKind::Thread,
            ViewKind::Review,
            ViewKind::Workspace,
            ViewKind::ManagedWorktrees,
            ViewKind::Board,
            ViewKind::Scratch,
        ] {
            assert_eq!(
                command_for_key(key(KeyCode::Char('?')), view),
                Some(Command::Help),
                "help must be keyboard-reachable in {view:?}"
            );
            assert_eq!(
                command_for_key(key(KeyCode::Char('/')), view),
                Some(Command::Search),
                "search must be keyboard-reachable in {view:?}"
            );
            assert_eq!(
                command_for_key(key(KeyCode::Char('.')), view),
                Some(Command::ContextActions),
                "context actions must be keyboard-reachable in {view:?}"
            );
            assert_eq!(
                command_for_key(key(KeyCode::Esc), view),
                Some(Command::Back),
                "back must be keyboard-reachable in {view:?}"
            );
        }
    }

    #[test]
    fn terminal_drawer_bindings_are_explicit_and_not_available_for_scratch() {
        assert_eq!(
            command_for_key(key(KeyCode::Char('t')), ViewKind::Workspace),
            Some(Command::TerminalDrawer)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('T')), ViewKind::Workspace),
            Some(Command::CloseTerminalDrawer)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('t')), ViewKind::Scratch),
            None
        );
    }

    #[test]
    fn managed_worktree_bindings_are_confirmation_oriented() {
        assert_eq!(
            command_for_key(key(KeyCode::Char('m')), ViewKind::Workspace),
            Some(Command::ManagedWorktrees)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('n')), ViewKind::ManagedWorktrees),
            Some(Command::CreateWorktree)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('y')), ViewKind::ManagedWorktrees),
            Some(Command::ConfirmOperation)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('d')), ViewKind::ManagedWorktrees),
            Some(Command::RemoveWorktree)
        );
    }

    #[test]
    fn board_bindings_keep_horizontal_stage_and_vertical_item_navigation_distinct() {
        assert_eq!(
            command_for_key(key(KeyCode::Char('h')), ViewKind::Board),
            Some(Command::BoardLeft)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('l')), ViewKind::Board),
            Some(Command::BoardRight)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('j')), ViewKind::Board),
            Some(Command::Next)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Tab), ViewKind::Board),
            Some(Command::CycleSavedView)
        );
    }

    #[test]
    fn locked_registry_bindings_are_semantic_commands() {
        assert_eq!(
            command_for_key(key(KeyCode::Char(' ')), ViewKind::Registry),
            Some(Command::NextAttention)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('a')), ViewKind::Registry),
            Some(Command::QuickPrompt)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('l')), ViewKind::Registry),
            Some(Command::ToggleHostLocalFilter)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('l')), ViewKind::Board),
            Some(Command::BoardRight)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('g')), ViewKind::Registry),
            Some(Command::ToggleRepoBackedFilter)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('g')), ViewKind::Thread),
            Some(Command::Goal)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('h')), ViewKind::Registry),
            Some(Command::ToggleAllHistory)
        );
        assert_eq!(
            command_for_key(key(KeyCode::Char('h')), ViewKind::Board),
            Some(Command::BoardLeft)
        );
        assert_eq!(
            command_for_key(
                KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
                ViewKind::Registry
            ),
            Some(Command::CommandPalette)
        );
    }
}
