use crate::app::ViewKind;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    QuitOrInterrupt,
    Back,
    Help,
    CommandPalette,
    Search,
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
    HotSlot(u8),
    BeginHotSlotBind,
}

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

    match (view, key.code) {
        (_, KeyCode::Char('?')) => Some(Command::Help),
        (_, KeyCode::Char('/')) => Some(Command::Search),
        (_, KeyCode::Char('.')) => Some(Command::ContextActions),
        (_, KeyCode::Esc) => Some(Command::Back),
        (ViewKind::Registry, KeyCode::Char('q')) => Some(Command::QuitOrInterrupt),
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
        (ViewKind::Board, KeyCode::Char('h') | KeyCode::Left) => Some(Command::BoardLeft),
        (ViewKind::Board, KeyCode::Char('l') | KeyCode::Right) => Some(Command::BoardRight),
        (ViewKind::Board, KeyCode::Char('j') | KeyCode::Down) => Some(Command::Next),
        (ViewKind::Board, KeyCode::Char('k') | KeyCode::Up) => Some(Command::Previous),
        (ViewKind::Board, KeyCode::Enter) => Some(Command::Open),
        (ViewKind::Board, KeyCode::Char(' ')) => Some(Command::NextAttention),
        (ViewKind::Board, KeyCode::Char('a')) => Some(Command::QuickPrompt),
        (ViewKind::Board, KeyCode::Char('n')) => Some(Command::New),
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
            command_for_key(
                KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
                ViewKind::Registry
            ),
            Some(Command::CommandPalette)
        );
    }
}
