use super::{AppState, View};
use crate::command::Command;
use crate::domain::ThreadId;
use crate::planning::SourceKind;

fn fuzzy_subsequence(needle: &str, haystack: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut remaining = needle.chars();
    let mut current = remaining.next();
    for candidate in haystack.chars() {
        if current == Some(candidate) {
            current = remaining.next();
            if current.is_none() {
                return true;
            }
        }
    }
    false
}

fn command_matches_query(command: Command, query: &str) -> bool {
    let normalized = query.trim().to_lowercase();
    if normalized.is_empty() {
        return true;
    }
    [false, true].into_iter().any(|simplified_chinese| {
        command
            .palette_label(simplified_chinese)
            .is_some_and(|label| fuzzy_subsequence(&normalized, &label.to_lowercase()))
    })
}

impl AppState {
    fn command_palette_thread_id(&self) -> Option<ThreadId> {
        self.selected_local_target().and_then(|target| {
            (target.kind == SourceKind::CodexThread).then(|| ThreadId::new(target.value))
        })
    }

    pub(super) fn build_command_palette_choices(&self) -> Vec<Command> {
        let mut choices = vec![Command::Search];
        let thread_id = self.command_palette_thread_id();

        if matches!(self.view, View::Registry | View::Board) {
            choices.push(Command::NextAttention);
        }
        if thread_id.is_some() {
            choices.push(Command::QuickPrompt);
        }
        if !matches!(self.view, View::Board) {
            choices.push(Command::Board);
        }
        if thread_id.is_some() && !matches!(self.view, View::Review(_)) {
            choices.push(Command::Review);
        }
        if thread_id.is_some() && !matches!(self.view, View::Workspace(_)) {
            choices.push(Command::Workspace);
        }
        if thread_id.as_ref().is_some_and(|thread_id| {
            self.git_context(thread_id)
                .is_some_and(|context| context.repo.is_some())
        }) && !matches!(self.view, View::ManagedWorktrees(_))
        {
            choices.push(Command::ManagedWorktrees);
        }

        choices.push(Command::New);

        if matches!(self.view, View::Thread(_)) {
            choices.push(Command::Goal);
        }
        if matches!(self.view, View::Registry) && self.selected_thread().is_some() {
            choices.push(Command::TogglePin);
        }
        if self.selected_local_target().is_some() {
            choices.push(Command::Snooze);
        }
        if !self.context_choices().is_empty() {
            choices.push(Command::ContextActions);
        }
        if !matches!(self.view, View::Scratch(_)) {
            choices.push(if self.terminal_drawer_open {
                Command::CloseTerminalDrawer
            } else {
                Command::TerminalDrawer
            });
        }

        choices.push(Command::Help);
        choices
    }

    pub fn command_palette_choices(&self) -> Vec<Command> {
        let baseline = if self.command_palette_open {
            self.command_palette_items.clone()
        } else {
            self.build_command_palette_choices()
        };
        baseline
            .into_iter()
            .filter(|command| command_matches_query(*command, &self.command_palette_query))
            .collect()
    }

    pub fn command_palette_choice(&self) -> Option<Command> {
        self.command_palette_choices()
            .get(self.command_palette_selected)
            .copied()
    }

    pub(super) fn open_command_palette(&mut self) {
        let choices = self.build_command_palette_choices();
        if choices.is_empty() {
            return;
        }
        self.command_palette_items = choices;
        self.command_palette_query.clear();
        self.command_palette_open = true;
        self.command_palette_selected = 0;
        self.show_help = false;
    }

    pub(super) fn close_command_palette(&mut self) {
        self.command_palette_open = false;
        self.command_palette_selected = 0;
        self.command_palette_query.clear();
        self.command_palette_items.clear();
    }

    pub(super) fn move_command_palette(&mut self, delta: i32) {
        let len = self.command_palette_choices().len();
        if len == 0 {
            self.command_palette_selected = 0;
        } else {
            self.command_palette_selected =
                (self.command_palette_selected as i32 + delta).rem_euclid(len as i32) as usize;
        }
    }

    pub(super) fn input_command_palette_char(&mut self, character: char) {
        self.command_palette_query.push(character);
        self.command_palette_selected = 0;
    }

    pub(super) fn input_command_palette_text(&mut self, text: String) {
        self.command_palette_query
            .extend(text.chars().filter(|character| !character.is_control()));
        self.command_palette_selected = 0;
    }

    pub(super) fn backspace_command_palette(&mut self) {
        self.command_palette_query.pop();
        self.command_palette_selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    fn app() -> AppState {
        AppState::new(FakeBackend::seeded().snapshot().threads)
    }

    #[test]
    fn palette_query_fuzzy_matches_english_label() {
        let mut app = app();
        app.open_command_palette();
        app.command_palette_query = "srch".into();
        assert_eq!(app.command_palette_choices(), vec![Command::Search]);
    }

    #[test]
    fn palette_query_matches_both_language_labels() {
        let mut app = app();
        app.open_command_palette();

        app.command_palette_query = "board".into();
        assert!(app.command_palette_choices().contains(&Command::Board));

        app.command_palette_query = "看板".into();
        assert!(app.command_palette_choices().contains(&Command::Board));
    }

    #[test]
    fn no_match_has_no_hidden_selection() {
        let mut app = app();
        app.open_command_palette();
        app.command_palette_query = "definitely-no-command".into();
        assert!(app.command_palette_choices().is_empty());
        assert_eq!(app.command_palette_choice(), None);
    }

    #[test]
    fn frozen_palette_baseline_survives_context_change() {
        let mut app = app();
        app.open_command_palette();
        let baseline = app.command_palette_items.clone();
        app.view = View::Board;
        app.command_palette_query.clear();
        assert_eq!(app.command_palette_choices(), baseline);
    }
}
