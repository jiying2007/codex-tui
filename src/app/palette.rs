use super::{AppState, View};
use crate::command::Command;
use crate::domain::ThreadId;
use crate::planning::SourceKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandPaletteMatch {
    pub(crate) command: Command,
    pub(crate) label: &'static str,
    pub(crate) matched_char_indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FuzzyMatch {
    score: i32,
    positions: Vec<usize>,
}

fn fold_char(character: char) -> char {
    character.to_lowercase().next().unwrap_or(character)
}

fn query_chars(query: &str) -> Vec<char> {
    query
        .trim()
        .chars()
        .filter(|character| !character.is_whitespace())
        .map(fold_char)
        .collect()
}

fn is_boundary(previous: Option<char>) -> bool {
    previous.is_none_or(|character| {
        character.is_whitespace()
            || matches!(character, '-' | '_' | '/' | ':' | '·' | '.')
    })
}

fn score_positions(label: &[char], query: &[char], positions: &[usize]) -> i32 {
    let mut score = i32::try_from(query.len()).unwrap_or(i32::MAX / 8) * 50;
    if positions.first() == Some(&0) {
        score += 160;
    }
    for (index, position) in positions.iter().copied().enumerate() {
        if is_boundary(position.checked_sub(1).and_then(|prev| label.get(prev)).copied()) {
            score += 35;
        }
        if index > 0 {
            let previous = positions[index - 1];
            let gap = position.saturating_sub(previous + 1);
            if gap == 0 {
                score += 45;
            } else {
                score -= i32::try_from(gap.min(40)).unwrap_or(40) * 3;
            }
        }
    }
    score -= i32::try_from(label.len().saturating_sub(query.len()).min(120)).unwrap_or(120);
    score
}

fn fuzzy_match(query: &str, label: &str) -> Option<FuzzyMatch> {
    let query = query_chars(query);
    if query.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            positions: vec![],
        });
    }

    let original = label.chars().collect::<Vec<_>>();
    let folded = original.iter().copied().map(fold_char).collect::<Vec<_>>();
    let mut best: Option<FuzzyMatch> = None;

    for start in 0..folded.len() {
        if folded[start] != query[0] {
            continue;
        }
        let mut positions = vec![start];
        let mut cursor = start + 1;
        let mut complete = true;
        for needle in query.iter().copied().skip(1) {
            let Some(relative) = folded[cursor..]
                .iter()
                .position(|candidate| *candidate == needle)
            else {
                complete = false;
                break;
            };
            let position = cursor + relative;
            positions.push(position);
            cursor = position + 1;
        }
        if !complete {
            continue;
        }
        let mut score = score_positions(&original, &query, &positions);
        let compact_label = folded
            .iter()
            .copied()
            .filter(|character| !character.is_whitespace())
            .collect::<Vec<_>>();
        if compact_label == query {
            score += 1_000;
        } else if compact_label.starts_with(&query) {
            score += 400;
        }
        let candidate = FuzzyMatch { score, positions };
        if best
            .as_ref()
            .is_none_or(|current| candidate.score > current.score)
        {
            best = Some(candidate);
        }
    }
    best
}

impl AppState {
    fn command_palette_thread_id(&self) -> Option<ThreadId> {
        self.selected_local_target().and_then(|target| {
            (target.kind == SourceKind::CodexThread).then(|| ThreadId::new(target.value))
        })
    }

    pub(super) fn build_command_palette_choices(&self) -> Vec<Command> {
        let mut choices = vec![Command::Search, Command::TranscriptSearch];
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
            choices.push(Command::ThreadQueue);
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

    pub(crate) fn command_palette_matches(&self) -> Vec<CommandPaletteMatch> {
        let baseline = if self.command_palette_open {
            &self.command_palette_items
        } else {
            return self
                .build_command_palette_choices()
                .into_iter()
                .filter_map(|command| {
                    command
                        .palette_label(self.language.is_simplified_chinese())
                        .map(|label| CommandPaletteMatch {
                            command,
                            label,
                            matched_char_indices: vec![],
                        })
                })
                .collect();
        };

        if self.command_palette_query.trim().is_empty() {
            return baseline
                .iter()
                .copied()
                .filter_map(|command| {
                    command
                        .palette_label(self.language.is_simplified_chinese())
                        .map(|label| CommandPaletteMatch {
                            command,
                            label,
                            matched_char_indices: vec![],
                        })
                })
                .collect();
        }

        let display_chinese = self.language.is_simplified_chinese();
        let mut ranked = baseline
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(baseline_index, command)| {
                let mut candidates = Vec::new();
                for simplified_chinese in [display_chinese, !display_chinese] {
                    let Some(label) = command.palette_label(simplified_chinese) else {
                        continue;
                    };
                    if let Some(matched) = fuzzy_match(&self.command_palette_query, label) {
                        candidates.push((simplified_chinese == display_chinese, label, matched));
                    }
                }
                let (display_language, label, matched) = candidates
                    .into_iter()
                    .max_by(|left, right| {
                        left.2
                            .score
                            .cmp(&right.2.score)
                            .then_with(|| left.0.cmp(&right.0))
                    })?;
                let _ = display_language;
                Some((
                    matched.score,
                    baseline_index,
                    CommandPaletteMatch {
                        command,
                        label,
                        matched_char_indices: matched.positions,
                    },
                ))
            })
            .collect::<Vec<_>>();

        ranked.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.cmp(&right.1))
        });
        ranked.into_iter().map(|(_, _, matched)| matched).collect()
    }

    pub fn command_palette_choices(&self) -> Vec<Command> {
        self.command_palette_matches()
            .into_iter()
            .map(|matched| matched.command)
            .collect()
    }

    pub fn command_palette_choice(&self) -> Option<Command> {
        self.command_palette_matches()
            .get(self.command_palette_selected)
            .map(|matched| matched.command)
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
        let len = self.command_palette_matches().len();
        if len == 0 {
            self.command_palette_selected = 0;
        } else {
            self.command_palette_selected =
                (self.command_palette_selected as i32 + delta).rem_euclid(len as i32) as usize;
        }
    }

    pub(super) fn input_command_palette_char(&mut self, character: char) {
        if !character.is_control() {
            self.command_palette_query.push(character);
            self.command_palette_selected = 0;
        }
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
    fn fuzzy_scoring_rewards_prefix_contiguous_and_word_boundary_matches() {
        let prefix = fuzzy_match("bo", "Board").expect("prefix");
        let boundary = fuzzy_match("ob", "Open Board").expect("boundary");
        let sparse = fuzzy_match("ob", "something_obscure").expect("sparse");
        assert!(prefix.score > sparse.score);
        assert!(boundary.score > sparse.score);
    }

    #[test]
    fn palette_query_ranks_and_matches_both_languages() {
        let mut app = app();
        app.open_command_palette();

        app.command_palette_query = "board".into();
        assert_eq!(app.command_palette_choice(), Some(Command::Board));

        app.command_palette_query = "看板".into();
        assert_eq!(app.command_palette_choice(), Some(Command::Board));
    }

    #[test]
    fn match_positions_are_retained_for_highlight() {
        let matched = fuzzy_match("ob", "Open Board").expect("match");
        assert_eq!(matched.positions, vec![0, 5]);
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
