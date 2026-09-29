use crate::backend::BackendStatus;
use crate::domain::{AttentionReason, ThreadId, ThreadSummary, ThreadUiState};
use crate::store::LocalStateV1;
use std::collections::BTreeMap;

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
        }
    }

    pub const fn view_kind(&self) -> ViewKind {
        match self.view {
            View::Registry => ViewKind::Registry,
            View::Thread(_) => ViewKind::Thread,
        }
    }

    pub fn selected_thread(&self) -> Option<&ThreadSummary> {
        self.threads.get(self.selected)
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

    pub fn apply_local_state(&mut self, local: &LocalStateV1) {
        self.thread_ui = local.thread_ui.clone();
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
        }
    }
}

pub fn reduce(state: &mut AppState, action: Action) -> Vec<Effect> {
    match action {
        Action::ReplaceThreads(threads) => {
            let selected_id = state.selected_thread_id();
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
            if let Some(thread) = state.threads.get_mut(state.selected)
                && !thread.attention.contains(&AttentionReason::MarkedUnread)
            {
                thread.attention.push(AttentionReason::MarkedUnread);
                return vec![Effect::PersistOperatorState];
            }
        }
        Action::Quit => state.should_quit = true,
    }
    vec![]
}

fn move_selection(state: &mut AppState, delta: i32) {
    if state.threads.is_empty() {
        return;
    }
    let len = state.threads.len() as i32;
    let next = (state.selected as i32 + delta).rem_euclid(len);
    state.selected = next as usize;
}

fn select_next_attention(state: &mut AppState) {
    let len = state.threads.len();
    if len == 0 {
        return;
    }
    for offset in 1..=len {
        let index = (state.selected + offset) % len;
        if state.threads[index].needs_attention() {
            state.selected = index;
            return;
        }
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
}
