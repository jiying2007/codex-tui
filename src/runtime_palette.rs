use crate::runtime_commands::handle_command;
use codex_tui::{
    app::{Action, AppState, Effect, reduce},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(crate) fn handle_command_palette_paste(
    app: &mut AppState,
    text: String,
) -> Vec<Effect> {
    reduce(app, Action::CommandPaletteInputText(text))
}

pub(crate) fn handle_command_palette_key(
    app: &mut AppState,
    key: KeyEvent,
) -> Vec<Effect> {
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('k') {
        return reduce(app, Action::CloseCommandPalette);
    }
    match key.code {
        KeyCode::Esc => reduce(app, Action::CloseCommandPalette),
        KeyCode::Down => reduce(app, Action::MoveCommandPalette(1)),
        KeyCode::Up => reduce(app, Action::MoveCommandPalette(-1)),
        KeyCode::Backspace => reduce(app, Action::CommandPaletteBackspace),
        KeyCode::Enter => {
            let Some(choice) = app.command_palette_choice() else {
                return vec![];
            };
            reduce(app, Action::CloseCommandPalette);
            handle_command(app, choice)
        }
        KeyCode::Char(character)
            if !key.modifiers.intersects(
                KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
            ) =>
        {
            reduce(app, Action::CommandPaletteInputChar(character))
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_tui::{
        app::{Action, AppState, reduce},
        backend::{CodexBackend, FakeBackend},
        command::Command,
    };

    fn app() -> AppState {
        AppState::new(FakeBackend::seeded().snapshot().threads)
    }

    #[test]
    fn palette_accepts_typed_and_pasted_query_text() {
        let mut app = app();
        reduce(&mut app, Action::OpenCommandPalette);
        assert!(app.command_palette_open);

        handle_command_palette_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        );
        handle_command_palette_paste(&mut app, "oard".into());
        assert_eq!(app.command_palette_query, "board");
        assert_eq!(app.command_palette_choice(), Some(Command::Board));
    }

    #[test]
    fn empty_result_enter_keeps_palette_open_and_executes_nothing() {
        let mut app = app();
        reduce(&mut app, Action::OpenCommandPalette);
        handle_command_palette_paste(&mut app, "definitely-no-command".into());
        assert!(app.command_palette_choices().is_empty());

        let effects = handle_command_palette_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert!(effects.is_empty());
        assert!(app.command_palette_open);
    }

    #[test]
    fn arrow_navigation_does_not_modify_query() {
        let mut app = app();
        reduce(&mut app, Action::OpenCommandPalette);
        handle_command_palette_paste(&mut app, "o".into());
        let query = app.command_palette_query.clone();
        handle_command_palette_key(
            &mut app,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        );
        assert_eq!(app.command_palette_query, query);
    }
}
