use crate::backend::BackendStatus;
use crate::domain::{AttentionReason, ThreadId, ThreadSummary, ThreadUiState};
use crate::store::LocalStateV1;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Registry,
    Thread(ThreadId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    Registry,
    Thread,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
    Alias,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    ReplaceThreads(Vec<ThreadSummary>),
    BackendStatus(BackendStatus),
    MoveSelection(i32),
    OpenSelected,
    Back,
    NextAttention,
    QuickPrompt,
    ToggleHelp,
    SetDraft(String),
    ScrollBy(i16),
    ToggleFollow,
    MarkUnread,
    TogglePin,
    AcknowledgeAttention,
    BeginSearch,
    BeginAlias,
    InputChar(char),
    InputBackspace,
    CommitInput,
    CancelInput,
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    PersistOperatorState,
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub threads: Vec<ThreadSummary>,
    pub selected: usize,
    pub view: View,
    pub previous_target: Option<ThreadId>,
    pub thread_ui: BTreeMap<String, ThreadUiState>,
    pub show_help: bool,
    pub should_quit: bool,
    pub backend_status: BackendStatus,
    pub acknowledged_attention: BTreeSet<String>,
    pub filter: String,
    pub input_mode: InputMode,
    pub input_buffer: String,
    input_original: String,
}

impl AppState {
    pub fn new(threads: Vec<ThreadSummary>) -> Self {
        Self {
            threads,
            selected: 0,
            view: View::Registry,
            previous_target: None,
            thread_ui: BTreeMap::new(),
            show_help: false,
            should_quit: false,
            backend_status: BackendStatus::starting("unknown"),
            acknowledged_attention: BTreeSet::new(),
            filter: String::new(),
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            input_original: String::new(),
        }
    }

    pub const fn view_kind(&self) -> ViewKind {
        match self.view {
            View::Registry => ViewKind::Registry,
            View::Thread(_) => ViewKind::Thread,
        }
    }

    pub fn selected_thread(&self) -> Option<&ThreadSummary> {
        let thread = self.threads.get(self.selected)?;
        matches_filter(thread, &self.filter).then_some(thread)
    }

    pub fn selected_thread_id(&self) -> Option<ThreadId> {
        self.selected_thread().map(|thread| thread.id.clone())
    }

    pub fn current_thread_id(&self) -> Option<&ThreadId> {
        match &self.view {
            View::Registry => None,
            View::Thread(id) => Some(id),
        }
    }

    pub fn current_thread_ui(&self) -> Option<&ThreadUiState> {
        let id = self.current_thread_id()?;
        self.thread_ui.get(&id.0)
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        self.threads
            .iter()
            .enumerate()
            .filter_map(|(index, thread)| matches_filter(thread, &self.filter).then_some(index))
            .collect()
    }

    pub fn thread_needs_attention(&self, index: usize) -> bool {
        let Some(thread) = self.threads.get(index) else {
            return false;
        };
        let actionable = thread.attention.iter().any(|reason| {
            matches!(
                reason,
                AttentionReason::ApprovalRequired
                    | AttentionReason::UserInputRequired
                    | AttentionReason::SystemError
            )
        });
        actionable
            || (!thread.attention.is_empty() && !self.acknowledged_attention.contains(&thread.id.0))
    }

    pub fn apply_local_state(&mut self, local: &LocalStateV1) {
        self.thread_ui = local.thread_ui.clone();
        self.acknowledged_attention = local.acknowledged_attention.clone();
        for thread in &mut self.threads {
            if local.pins.contains(&thread.id.0) {
                thread.pinned = true;
            }
            if let Some(alias) = local.aliases.get(&thread.id.0) {
                thread.alias = Some(alias.clone());
            }
            if local.marked_unread.contains(&thread.id.0)
                && !thread.attention.contains(&AttentionReason::MarkedUnread)
            {
                thread.attention.push(AttentionReason::MarkedUnread);
            }
        }
        ensure_selection_visible(self);
    }

    pub fn to_local_state(&self) -> LocalStateV1 {
        let pins = self
            .threads
            .iter()
            .filter(|thread| thread.pinned)
            .map(|thread| thread.id.0.clone())
            .collect();
        let aliases = self
            .threads
            .iter()
            .filter_map(|thread| {
                thread
                    .alias
                    .as_ref()
                    .map(|alias| (thread.id.0.clone(), alias.clone()))
            })
            .collect();
        let marked_unread = self
            .threads
            .iter()
            .filter(|thread| thread.attention.contains(&AttentionReason::MarkedUnread))
            .map(|thread| thread.id.0.clone())
            .collect();

        LocalStateV1 {
            schema_version: 1,
            thread_ui: self.thread_ui.clone(),
            pins,
            aliases,
            marked_unread,
            acknowledged_attention: self.acknowledged_attention.clone(),
        }
    }
}

pub fn reduce(state: &mut AppState, action: Action) -> Vec<Effect> {
    match action {
        Action::ReplaceThreads(mut threads) => {
            let selected_id = state.selected_thread_id();
            for fresh in &mut threads {
                if let Some(existing) = state.threads.iter().find(|old| old.id == fresh.id) {
                    fresh.pinned = existing.pinned;
                    fresh.alias.clone_from(&existing.alias);

                    let existing_remote = remote_attention(existing);
                    let fresh_remote = remote_attention(fresh);
                    if existing_remote != fresh_remote && !fresh_remote.is_empty() {
                        state.acknowledged_attention.remove(&fresh.id.0);
                    }

                    if existing.attention.contains(&AttentionReason::MarkedUnread)
                        && !fresh.attention.contains(&AttentionReason::MarkedUnread)
                    {
                        fresh.attention.push(AttentionReason::MarkedUnread);
                    }
                }
            }
            state.threads = threads;
            if state.threads.is_empty() {
                state.selected = 0;
            } else if let Some(id) = selected_id {
                state.selected = state
                    .threads
                    .iter()
                    .position(|thread| thread.id == id)
                    .unwrap_or_else(|| state.selected.min(state.threads.len() - 1));
            } else {
                state.selected = state.selected.min(state.threads.len() - 1);
            }
            ensure_selection_visible(state);
        }
        Action::BackendStatus(status) => state.backend_status = status,
        Action::MoveSelection(delta) => move_selection(state, delta),
        Action::OpenSelected | Action::QuickPrompt => {
            if let Some(id) = state.selected_thread_id() {
                state.previous_target = state.current_thread_id().cloned();
                state.thread_ui.entry(id.0.clone()).or_default();
                state.view = View::Thread(id);
            }
        }
        Action::Back => state.view = View::Registry,
        Action::NextAttention => select_next_attention(state),
        Action::ToggleHelp => state.show_help = !state.show_help,
        Action::SetDraft(draft) => {
            if let Some(id) = state.current_thread_id().cloned() {
                state.thread_ui.entry(id.0).or_default().draft = draft;
                return vec![Effect::PersistOperatorState];
            }
        }
        Action::ScrollBy(delta) => {
            if let Some(id) = state.current_thread_id().cloned() {
                let ui = state.thread_ui.entry(id.0).or_default();
                ui.follow = false;
                ui.scroll = if delta.is_negative() {
                    ui.scroll.saturating_sub(delta.unsigned_abs())
                } else {
                    ui.scroll.saturating_add(delta as u16)
                };
                return vec![Effect::PersistOperatorState];
            }
        }
        Action::ToggleFollow => {
            if let Some(id) = state.current_thread_id().cloned() {
                let ui = state.thread_ui.entry(id.0).or_default();
                ui.follow = !ui.follow;
                return vec![Effect::PersistOperatorState];
            }
        }
        Action::MarkUnread => {
            if state.selected_thread().is_some()
                && let Some(thread) = state.threads.get_mut(state.selected)
            {
                state.acknowledged_attention.remove(&thread.id.0);
                if !thread.attention.contains(&AttentionReason::MarkedUnread) {
                    thread.attention.push(AttentionReason::MarkedUnread);
                }
                return vec![Effect::PersistOperatorState];
            }
        }
        Action::TogglePin => {
            if state.selected_thread().is_some()
                && let Some(thread) = state.threads.get_mut(state.selected)
            {
                thread.pinned = !thread.pinned;
                return vec![Effect::PersistOperatorState];
            }
        }
        Action::AcknowledgeAttention => {
            if state.selected_thread().is_some()
                && let Some(thread) = state.threads.get_mut(state.selected)
            {
                state.acknowledged_attention.insert(thread.id.0.clone());
                thread
                    .attention
                    .retain(|reason| *reason != AttentionReason::MarkedUnread);
                return vec![Effect::PersistOperatorState];
            }
        }
        Action::BeginSearch => {
            state.input_original.clone_from(&state.filter);
            state.input_buffer.clone_from(&state.filter);
            state.input_mode = InputMode::Search;
        }
        Action::BeginAlias => {
            if let Some(alias) = state
                .selected_thread()
                .map(|thread| thread.alias.clone().unwrap_or_default())
            {
                state.input_original.clear();
                state.input_buffer = alias;
                state.input_mode = InputMode::Alias;
            }
        }
        Action::InputChar(character) => {
            if state.input_mode != InputMode::Normal {
                state.input_buffer.push(character);
                if state.input_mode == InputMode::Search {
                    state.filter.clone_from(&state.input_buffer);
                    ensure_selection_visible(state);
                }
            }
        }
        Action::InputBackspace => {
            if state.input_mode != InputMode::Normal {
                state.input_buffer.pop();
                if state.input_mode == InputMode::Search {
                    state.filter.clone_from(&state.input_buffer);
                    ensure_selection_visible(state);
                }
            }
        }
        Action::CommitInput => {
            let mode = state.input_mode;
            if mode == InputMode::Alias {
                let alias = state.input_buffer.trim().to_string();
                if let Some(thread) = state.threads.get_mut(state.selected) {
                    thread.alias = (!alias.is_empty()).then_some(alias);
                    state.input_mode = InputMode::Normal;
                    state.input_buffer.clear();
                    state.input_original.clear();
                    return vec![Effect::PersistOperatorState];
                }
            }
            state.input_mode = InputMode::Normal;
            state.input_buffer.clear();
            state.input_original.clear();
        }
        Action::CancelInput => {
            if state.input_mode == InputMode::Search {
                state.filter.clone_from(&state.input_original);
                ensure_selection_visible(state);
            }
            state.input_mode = InputMode::Normal;
            state.input_buffer.clear();
            state.input_original.clear();
        }
        Action::Quit => state.should_quit = true,
    }
    vec![]
}

fn remote_attention(thread: &ThreadSummary) -> Vec<AttentionReason> {
    thread
        .attention
        .iter()
        .filter(|reason| **reason != AttentionReason::MarkedUnread)
        .cloned()
        .collect()
}

fn move_selection(state: &mut AppState, delta: i32) {
    let visible = state.visible_indices();
    if visible.is_empty() {
        return;
    }
    let current = visible
        .iter()
        .position(|index| *index == state.selected)
        .unwrap_or(0);
    let len = visible.len() as i32;
    let next = (current as i32 + delta).rem_euclid(len) as usize;
    state.selected = visible[next];
}

fn select_next_attention(state: &mut AppState) {
    let visible = state.visible_indices();
    if visible.is_empty() {
        return;
    }
    let current = visible
        .iter()
        .position(|index| *index == state.selected)
        .unwrap_or(0);
    for offset in 1..=visible.len() {
        let index = visible[(current + offset) % visible.len()];
        if state.thread_needs_attention(index) {
            state.selected = index;
            return;
        }
    }
}

fn ensure_selection_visible(state: &mut AppState) {
    let visible = state.visible_indices();
    if visible.is_empty() {
        state.selected = 0;
    } else if !visible.contains(&state.selected) {
        state.selected = visible[0];
    }
}

fn matches_filter(thread: &ThreadSummary, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }

    let haystack = [
        thread.id.0.as_str(),
        thread.display_title(),
        thread.title.as_str(),
        thread.workspace.as_str(),
        thread.metadata.cwd.as_str(),
        thread.metadata.source.as_str(),
        thread.metadata.workspace_key.as_str(),
        thread.metadata.model.as_deref().unwrap_or_default(),
        thread.metadata.project_id.as_deref().unwrap_or_default(),
    ]
    .join(" ")
    .to_lowercase();

    query
        .split_whitespace()
        .all(|token| fuzzy_subsequence(token, &haystack))
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    fn app() -> AppState {
        AppState::new(FakeBackend::seeded().snapshot().threads)
    }

    #[test]
    fn refresh_never_steals_manual_selection() {
        let mut app = app();
        reduce(&mut app, Action::MoveSelection(2));
        let selected = app.selected_thread_id().expect("selection");

        let mut reordered = app.threads.clone();
        reordered.reverse();
        reduce(&mut app, Action::ReplaceThreads(reordered));

        assert_eq!(app.selected_thread_id(), Some(selected));
    }

    #[test]
    fn opening_and_backing_out_preserves_registry_selection() {
        let mut app = app();
        reduce(&mut app, Action::MoveSelection(1));
        let selected = app.selected;
        reduce(&mut app, Action::OpenSelected);
        reduce(&mut app, Action::Back);
        assert_eq!(app.selected, selected);
        assert_eq!(app.view, View::Registry);
    }

    #[test]
    fn per_thread_draft_scroll_and_follow_are_independent() {
        let mut app = app();
        reduce(&mut app, Action::OpenSelected);
        reduce(&mut app, Action::SetDraft("first draft".into()));
        reduce(&mut app, Action::ScrollBy(5));
        reduce(&mut app, Action::Back);
        reduce(&mut app, Action::MoveSelection(1));
        reduce(&mut app, Action::OpenSelected);
        reduce(&mut app, Action::SetDraft("second draft".into()));

        let first = app.thread_ui.get("thread-impl").expect("first state");
        let second = app.thread_ui.get("thread-kws").expect("second state");
        assert_eq!(first.draft, "first draft");
        assert_eq!(first.scroll, 5);
        assert!(!first.follow);
        assert_eq!(second.draft, "second draft");
        assert_eq!(second.scroll, 0);
        assert!(second.follow);
    }

    #[test]
    fn fuzzy_metadata_filter_keeps_selection_inside_visible_rows() {
        let mut app = app();
        reduce(&mut app, Action::BeginSearch);
        for character in "audio".chars() {
            reduce(&mut app, Action::InputChar(character));
        }
        assert_eq!(app.visible_indices(), vec![2]);
        assert_eq!(
            app.selected_thread_id().expect("selected").0,
            "thread-audio"
        );
    }

    #[test]
    fn local_ack_does_not_hide_actionable_approval_or_input() {
        let mut app = app();
        app.selected = 1;
        reduce(&mut app, Action::AcknowledgeAttention);
        assert!(app.thread_needs_attention(1));
    }

    #[test]
    fn empty_filter_result_has_no_hidden_selection_or_mutation_target() {
        let mut app = app();
        reduce(&mut app, Action::BeginSearch);
        for character in "no-such-thread".chars() {
            reduce(&mut app, Action::InputChar(character));
        }
        assert!(app.visible_indices().is_empty());
        assert!(app.selected_thread().is_none());
        let effects = reduce(&mut app, Action::TogglePin);
        assert!(effects.is_empty());
        assert!(!app.threads[0].pinned);
    }

    #[test]
    fn pin_alias_and_ack_round_trip_through_local_state() {
        let mut app = app();
        reduce(&mut app, Action::TogglePin);
        reduce(&mut app, Action::BeginAlias);
        for character in "primary".chars() {
            reduce(&mut app, Action::InputChar(character));
        }
        reduce(&mut app, Action::CommitInput);
        reduce(&mut app, Action::AcknowledgeAttention);
        let local = app.to_local_state();

        let mut restored = AppState::new(FakeBackend::seeded().snapshot().threads);
        restored.apply_local_state(&local);
        assert!(restored.threads[0].pinned);
        assert_eq!(restored.threads[0].alias.as_deref(), Some("primary"));
        assert!(restored.acknowledged_attention.contains("thread-impl"));
    }
}
