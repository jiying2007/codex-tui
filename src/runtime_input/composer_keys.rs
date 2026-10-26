//! Composer navigation is isolated from global thread/history shortcuts.
use super::*;

pub(super) fn action(key: KeyEvent) -> Option<Action> {
    if key.modifiers.intersects(
        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
    ) {
        return None;
    }
    match key.code {
        KeyCode::Left => Some(Action::ComposerCursorMove(-1)),
        KeyCode::Right => Some(Action::ComposerCursorMove(1)),
        KeyCode::Home => Some(Action::ComposerCursorBoundary(false)),
        KeyCode::End => Some(Action::ComposerCursorBoundary(true)),
        KeyCode::Up => Some(Action::ComposerViewScroll(1)),
        KeyCode::Down => Some(Action::ComposerViewScroll(-1)),
        KeyCode::PageUp => Some(Action::ComposerViewScroll(3)),
        KeyCode::PageDown => Some(Action::ComposerViewScroll(-3)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_keys_do_not_leak_to_thread_history() {
        let plain = KeyModifiers::NONE;
        assert_eq!(action(KeyEvent::new(KeyCode::Home, plain)), Some(Action::ComposerCursorBoundary(false)));
        assert_eq!(action(KeyEvent::new(KeyCode::Left, plain)), Some(Action::ComposerCursorMove(-1)));
        assert_eq!(action(KeyEvent::new(KeyCode::PageUp, plain)), Some(Action::ComposerViewScroll(3)));
        assert_eq!(action(KeyEvent::new(KeyCode::Enter, plain)), None);
        assert_eq!(action(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), None);
    }
}
