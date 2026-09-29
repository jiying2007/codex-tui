use crate::backend::BackendStatus;
use crate::conversation::{
    ConversationPage, ConversationState, InteractiveRequest, InteractiveRequestKind,
    InteractiveResolution, RpcRequestId, UserInputQuestion,
};
use crate::domain::{AttentionReason, RuntimeStatus, ThreadId, ThreadSummary, ThreadUiState};
use crate::git::{GitContext, GitReview};
use crate::store::LocalStateV1;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Registry,
    Thread(ThreadId),
    Review(ThreadId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    Registry,
    Thread,
    Review,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
    Alias,
    Composer,
    UserInput,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    ReplaceThreads(Vec<ThreadSummary>),
    BackendStatus(BackendStatus),
    RefreshGitProjections,
    GitContextLoaded(GitContext),
    GitReviewLoaded(GitReview),
    ReviewError { thread_id: ThreadId, error: String },
    OpenReview,
    MoveReview(i32),
    ScrollReviewBy(i16),
    ToggleReviewWordDiff,
    OpenReviewExternalEditor,
    ConversationLoaded(ConversationPage),
    OlderConversationLoaded(ConversationPage),
    ConversationFailed { thread_id: ThreadId, error: String },
    PromptSubmitted { thread_id: ThreadId },
    InteractiveRequested(InteractiveRequest),
    InteractiveResolved { request_id: RpcRequestId },
    ResolvePending(InteractiveResolution),
    BeginUserInput,
    MoveSelection(i32),
    OpenSelected,
    Back,
    NextAttention,
    QuickPrompt,
    InterruptCurrent,
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
    ProbeGit {
        thread_id: ThreadId,
        cwd: String,
    },
    LoadGitReview {
        thread_id: ThreadId,
        cwd: String,
    },
    OpenExternalEditor {
        thread_id: ThreadId,
        cwd: String,
        path: String,
    },
    LoadConversation(ThreadId),
    StopWatchingConversation(ThreadId),
    LoadOlderConversation {
        thread_id: ThreadId,
        turn_cursor: Option<String>,
        item_cursor: Option<String>,
    },
    SubmitPrompt {
        thread_id: ThreadId,
        text: String,
        active_turn_id: Option<String>,
    },
    InterruptTurn {
        thread_id: ThreadId,
        turn_id: String,
    },
    ResolveInteractive {
        request_id: RpcRequestId,
        resolution: InteractiveResolution,
    },
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub threads: Vec<ThreadSummary>,
    pub selected: usize,
    pub view: View,
    pub previous_target: Option<ThreadId>,
    pub thread_ui: BTreeMap<String, ThreadUiState>,
    pub conversations: BTreeMap<String, ConversationState>,
    pub git_contexts: BTreeMap<String, GitContext>,
    pub git_reviews: BTreeMap<String, GitReview>,
    pub review_return_view: Option<View>,
    pub review_selected: usize,
    pub review_scroll: u16,
    pub review_word_diff: bool,
    pub pending_requests: Vec<InteractiveRequest>,
    pub user_input_request_id: Option<RpcRequestId>,
    pub user_input_question_index: usize,
    pub user_input_answers: BTreeMap<String, Vec<String>>,
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
            conversations: BTreeMap::new(),
            git_contexts: BTreeMap::new(),
            git_reviews: BTreeMap::new(),
            review_return_view: None,
            review_selected: 0,
            review_scroll: 0,
            review_word_diff: false,
            pending_requests: vec![],
            user_input_request_id: None,
            user_input_question_index: 0,
            user_input_answers: BTreeMap::new(),
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
            View::Review(_) => ViewKind::Review,
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
            View::Thread(id) | View::Review(id) => Some(id),
        }
    }

    pub fn current_thread_ui(&self) -> Option<&ThreadUiState> {
        let id = self.current_thread_id()?;
        self.thread_ui.get(&id.0)
    }

    pub fn current_conversation(&self) -> Option<&ConversationState> {
        let id = self.current_thread_id()?;
        self.conversations.get(&id.0)
    }

    pub fn git_context(&self, thread_id: &ThreadId) -> Option<&GitContext> {
        self.git_contexts.get(&thread_id.0)
    }

    pub fn current_review(&self) -> Option<&GitReview> {
        let View::Review(thread_id) = &self.view else {
            return None;
        };
        self.git_reviews.get(&thread_id.0)
    }

    pub fn selected_review_change(&self) -> Option<&crate::git::GitFileChange> {
        self.current_review()?.changes.get(self.review_selected)
    }

    pub fn worktree_collision_count(&self, thread_id: &ThreadId) -> usize {
        let Some(context) = self.git_context(thread_id) else {
            return 0;
        };
        let Some(worktree) = context.worktree.as_ref() else {
            return 0;
        };
        self.threads
            .iter()
            .filter(|thread| {
                thread.id != *thread_id
                    && matches!(
                        thread.runtime,
                        RuntimeStatus::Working | RuntimeStatus::WaitingHuman
                    )
                    && self
                        .git_context(&thread.id)
                        .and_then(|other| other.worktree.as_ref())
                        .is_some_and(|other| {
                            other.canonical_path == worktree.canonical_path
                                && other.repo == worktree.repo
                        })
            })
            .count()
    }

    pub fn current_pending_request(&self) -> Option<&InteractiveRequest> {
        let thread_id = self.current_thread_id()?;
        self.pending_requests
            .iter()
            .find(|request| request.thread_id == *thread_id)
    }

    pub fn current_user_input_question(&self) -> Option<&UserInputQuestion> {
        let request_id = self.user_input_request_id.as_ref()?;
        let request = self
            .pending_requests
            .iter()
            .find(|request| &request.request_id == request_id)?;
        let InteractiveRequestKind::UserInput { questions } = &request.kind else {
            return None;
        };
        questions.get(self.user_input_question_index)
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
        let interactive = self
            .pending_requests
            .iter()
            .any(|request| request.thread_id == thread.id);
        actionable
            || interactive
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
        Action::RefreshGitProjections => {
            let mut effects = Vec::new();
            for thread in &state.threads {
                if thread.metadata.cwd.trim().is_empty() {
                    continue;
                }
                let needs_probe = state
                    .git_contexts
                    .get(&thread.id.0)
                    .is_none_or(|context| context.cwd != thread.metadata.cwd);
                if needs_probe {
                    state.git_contexts.insert(
                        thread.id.0.clone(),
                        GitContext::pending(thread.id.clone(), thread.metadata.cwd.clone()),
                    );
                    effects.push(Effect::ProbeGit {
                        thread_id: thread.id.clone(),
                        cwd: thread.metadata.cwd.clone(),
                    });
                }
            }
            state.git_contexts.retain(|thread_id, _| {
                state.threads.iter().any(|thread| thread.id.0 == *thread_id)
            });
            return effects;
        }
        Action::GitContextLoaded(context) => {
            state
                .git_contexts
                .insert(context.thread_id.0.clone(), context);
        }
        Action::GitReviewLoaded(review) => {
            let key = review.thread_id.0.clone();
            let len = review.changes.len();
            state.git_reviews.insert(key, review);
            state.review_selected = if len == 0 {
                0
            } else {
                state.review_selected.min(len - 1)
            };
        }
        Action::ReviewError { thread_id, error } => {
            let review = state
                .git_reviews
                .entry(thread_id.0.clone())
                .or_insert_with(|| GitReview::pending(thread_id, ""));
            review.error = Some(error);
        }
        Action::OpenReview => {
            let thread_id = match &state.view {
                View::Registry => state.selected_thread_id(),
                View::Thread(id) | View::Review(id) => Some(id.clone()),
            };
            let Some(thread_id) = thread_id else {
                return vec![];
            };
            let cwd = state
                .threads
                .iter()
                .find(|thread| thread.id == thread_id)
                .map(|thread| thread.metadata.cwd.clone())
                .unwrap_or_default();
            if cwd.trim().is_empty() {
                return vec![];
            }
            if !matches!(state.view, View::Review(_)) {
                state.review_return_view = Some(state.view.clone());
            }
            state.review_selected = 0;
            state.review_scroll = 0;
            state.git_reviews.insert(
                thread_id.0.clone(),
                GitReview::pending(thread_id.clone(), cwd.clone()),
            );
            state.view = View::Review(thread_id.clone());
            return vec![Effect::LoadGitReview { thread_id, cwd }];
        }
        Action::MoveReview(delta) => {
            let Some(review) = state.current_review() else {
                return vec![];
            };
            if review.changes.is_empty() {
                state.review_selected = 0;
                return vec![];
            }
            let len = review.changes.len() as i32;
            state.review_selected = (state.review_selected as i32 + delta).rem_euclid(len) as usize;
            state.review_scroll = 0;
        }
        Action::ScrollReviewBy(delta) => {
            state.review_scroll = if delta.is_negative() {
                state.review_scroll.saturating_sub(delta.unsigned_abs())
            } else {
                state.review_scroll.saturating_add(delta as u16)
            };
        }
        Action::ToggleReviewWordDiff => state.review_word_diff = !state.review_word_diff,
        Action::OpenReviewExternalEditor => {
            let View::Review(thread_id) = &state.view else {
                return vec![];
            };
            let Some(review) = state.git_reviews.get(&thread_id.0) else {
                return vec![];
            };
            let Some(change) = review.changes.get(state.review_selected) else {
                return vec![];
            };
            return vec![Effect::OpenExternalEditor {
                thread_id: thread_id.clone(),
                cwd: review.cwd.clone(),
                path: change.path.clone(),
            }];
        }
        Action::ConversationLoaded(page) => {
            let key = page.thread_id.0.clone();
            state
                .conversations
                .entry(key)
                .or_insert_with(|| ConversationState::loading(page.thread_id.clone()))
                .replace_page(page);
        }
        Action::OlderConversationLoaded(page) => {
            let key = page.thread_id.0.clone();
            state
                .conversations
                .entry(key)
                .or_insert_with(|| ConversationState::loading(page.thread_id.clone()))
                .prepend_page(page);
        }
        Action::ConversationFailed { thread_id, error } => {
            let conversation = state
                .conversations
                .entry(thread_id.0.clone())
                .or_insert_with(|| ConversationState::loading(thread_id));
            conversation.loading = false;
            conversation.loading_older = false;
            conversation.error = Some(error);
        }
        Action::PromptSubmitted { thread_id } => {
            if let Some(ui) = state.thread_ui.get_mut(&thread_id.0) {
                ui.draft.clear();
            }
            return vec![Effect::PersistOperatorState];
        }
        Action::InteractiveRequested(request) => {
            state.acknowledged_attention.remove(&request.thread_id.0);
            let is_current_thread = state.current_thread_id() == Some(&request.thread_id);
            state
                .pending_requests
                .retain(|pending| pending.request_id != request.request_id);
            state.pending_requests.push(request);
            if is_current_thread && state.input_mode == InputMode::Composer {
                state.input_mode = InputMode::Normal;
            }
        }
        Action::InteractiveResolved { request_id } => {
            state
                .pending_requests
                .retain(|request| request.request_id != request_id);
            if state.user_input_request_id.as_ref() == Some(&request_id) {
                clear_user_input_editor(state);
            }
        }
        Action::ResolvePending(resolution) => {
            if let Some(request) = state.current_pending_request().cloned() {
                let allowed = match request.kind {
                    InteractiveRequestKind::UserInput { .. } => matches!(
                        resolution,
                        InteractiveResolution::Decline | InteractiveResolution::Cancel
                    ),
                    _ => !matches!(resolution, InteractiveResolution::UserInput(_)),
                };
                if allowed {
                    return vec![Effect::ResolveInteractive {
                        request_id: request.request_id,
                        resolution,
                    }];
                }
            }
        }
        Action::BeginUserInput => {
            if let Some(request) = state.current_pending_request().cloned()
                && matches!(request.kind, InteractiveRequestKind::UserInput { .. })
            {
                state.user_input_request_id = Some(request.request_id);
                state.user_input_question_index = 0;
                state.user_input_answers.clear();
                state.input_buffer.clear();
                state.input_mode = InputMode::UserInput;
            }
        }
        Action::MoveSelection(delta) => move_selection(state, delta),
        Action::OpenSelected => {
            if let Some(id) = state.selected_thread_id() {
                state.previous_target = state.current_thread_id().cloned();
                state.thread_ui.entry(id.0.clone()).or_default();
                state
                    .conversations
                    .entry(id.0.clone())
                    .or_insert_with(|| ConversationState::loading(id.clone()))
                    .loading = true;
                state.view = View::Thread(id.clone());
                return vec![Effect::LoadConversation(id)];
            }
        }
        Action::QuickPrompt => {
            let id = match state.current_thread_id().cloned() {
                Some(id) => id,
                None => {
                    let Some(id) = state.selected_thread_id() else {
                        return vec![];
                    };
                    state.previous_target = None;
                    state
                        .conversations
                        .entry(id.0.clone())
                        .or_insert_with(|| ConversationState::loading(id.clone()))
                        .loading = true;
                    state.view = View::Thread(id.clone());
                    id
                }
            };
            state.thread_ui.entry(id.0.clone()).or_default();
            state.input_mode = InputMode::Composer;
            return vec![Effect::LoadConversation(id)];
        }
        Action::InterruptCurrent => {
            if let Some(thread_id) = state.current_thread_id().cloned() {
                let active_turn_id = state
                    .conversations
                    .get(&thread_id.0)
                    .and_then(ConversationState::active_turn_id)
                    .map(ToOwned::to_owned);
                if let Some(turn_id) = active_turn_id {
                    return vec![Effect::InterruptTurn { thread_id, turn_id }];
                }
                if state.input_mode == InputMode::UserInput {
                    clear_user_input_editor(state);
                } else {
                    state.input_mode = InputMode::Normal;
                }
                state.view = View::Registry;
                return vec![Effect::StopWatchingConversation(thread_id)];
            }
        }
        Action::Back => {
            if matches!(state.view, View::Review(_)) {
                state.view = state.review_return_view.take().unwrap_or(View::Registry);
                state.review_scroll = 0;
                return vec![];
            }
            let thread_id = state.current_thread_id().cloned();
            if state.input_mode == InputMode::UserInput {
                clear_user_input_editor(state);
            } else {
                state.input_mode = InputMode::Normal;
            }
            state.view = View::Registry;
            if let Some(thread_id) = thread_id {
                return vec![Effect::StopWatchingConversation(thread_id)];
            }
        }
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
                let current_scroll = state.thread_ui.entry(id.0.clone()).or_default().scroll;
                if delta.is_negative() && current_scroll == 0 {
                    let older_request = state.conversations.get(&id.0).and_then(|conversation| {
                        (!conversation.loading_older
                            && (conversation.next_turn_cursor.is_some()
                                || conversation.next_item_cursor.is_some()))
                        .then(|| {
                            (
                                conversation.next_turn_cursor.clone(),
                                conversation.next_item_cursor.clone(),
                            )
                        })
                    });
                    if let Some((turn_cursor, item_cursor)) = older_request {
                        if let Some(conversation) = state.conversations.get_mut(&id.0) {
                            conversation.loading_older = true;
                        }
                        return vec![Effect::LoadOlderConversation {
                            thread_id: id,
                            turn_cursor,
                            item_cursor,
                        }];
                    }
                    return vec![];
                }

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
        Action::InputChar(character) => match state.input_mode {
            InputMode::Normal => {}
            InputMode::Composer => {
                if let Some(id) = state.current_thread_id().cloned() {
                    state
                        .thread_ui
                        .entry(id.0)
                        .or_default()
                        .draft
                        .push(character);
                    return vec![Effect::PersistOperatorState];
                }
            }
            InputMode::Search | InputMode::Alias | InputMode::UserInput => {
                state.input_buffer.push(character);
                if state.input_mode == InputMode::Search {
                    state.filter.clone_from(&state.input_buffer);
                    ensure_selection_visible(state);
                }
            }
        },
        Action::InputBackspace => match state.input_mode {
            InputMode::Normal => {}
            InputMode::Composer => {
                if let Some(id) = state.current_thread_id().cloned() {
                    state.thread_ui.entry(id.0).or_default().draft.pop();
                    return vec![Effect::PersistOperatorState];
                }
            }
            InputMode::Search | InputMode::Alias | InputMode::UserInput => {
                state.input_buffer.pop();
                if state.input_mode == InputMode::Search {
                    state.filter.clone_from(&state.input_buffer);
                    ensure_selection_visible(state);
                }
            }
        },
        Action::CommitInput => {
            let mode = state.input_mode;
            if mode == InputMode::UserInput {
                let Some(request_id) = state.user_input_request_id.clone() else {
                    clear_user_input_editor(state);
                    return vec![];
                };
                let Some(question) = state.current_user_input_question().cloned() else {
                    clear_user_input_editor(state);
                    return vec![];
                };
                let input = state.input_buffer.trim();
                if input.is_empty() {
                    return vec![];
                }
                let answers = if question.options.is_empty() {
                    vec![input.to_string()]
                } else {
                    input
                        .split(',')
                        .map(str::trim)
                        .filter(|answer| !answer.is_empty())
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>()
                };
                if answers.is_empty() {
                    return vec![];
                }
                state.user_input_answers.insert(question.id, answers);
                state.input_buffer.clear();

                let question_count = state
                    .pending_requests
                    .iter()
                    .find(|request| request.request_id == request_id)
                    .and_then(|request| match &request.kind {
                        InteractiveRequestKind::UserInput { questions } => Some(questions.len()),
                        _ => None,
                    })
                    .unwrap_or(0);

                if state.user_input_question_index + 1 < question_count {
                    state.user_input_question_index += 1;
                    return vec![];
                }

                let answers = std::mem::take(&mut state.user_input_answers);
                clear_user_input_editor(state);
                return vec![Effect::ResolveInteractive {
                    request_id,
                    resolution: InteractiveResolution::UserInput(answers),
                }];
            }
            if mode == InputMode::Composer {
                if let Some(thread_id) = state.current_thread_id().cloned() {
                    let conversation_ready =
                        state
                            .conversations
                            .get(&thread_id.0)
                            .is_some_and(|conversation| {
                                !conversation.loading && conversation.error.is_none()
                            });
                    if !conversation_ready {
                        return vec![];
                    }

                    let text = state
                        .thread_ui
                        .get(&thread_id.0)
                        .map(|ui| ui.draft.trim().to_string())
                        .unwrap_or_default();
                    if !text.is_empty() {
                        let active_turn_id = state
                            .conversations
                            .get(&thread_id.0)
                            .and_then(ConversationState::active_turn_id)
                            .map(ToOwned::to_owned);
                        state.input_mode = InputMode::Normal;
                        return vec![Effect::SubmitPrompt {
                            thread_id,
                            text,
                            active_turn_id,
                        }];
                    }
                }
                return vec![];
            }
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
            if state.input_mode == InputMode::UserInput {
                clear_user_input_editor(state);
            } else {
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                state.input_original.clear();
            }
        }
        Action::Quit => state.should_quit = true,
    }
    vec![]
}

fn clear_user_input_editor(state: &mut AppState) {
    state.user_input_request_id = None;
    state.user_input_question_index = 0;
    state.user_input_answers.clear();
    state.input_buffer.clear();
    state.input_mode = InputMode::Normal;
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

    let fields = [
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
    .map(str::to_lowercase);

    query
        .split_whitespace()
        .all(|token| fields.iter().any(|field| fuzzy_subsequence(token, field)))
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
        let effects = reduce(&mut app, Action::OpenSelected);
        assert_eq!(
            effects,
            vec![Effect::LoadConversation(
                app.current_thread_id().expect("thread").clone()
            )]
        );
        let back_effects = reduce(&mut app, Action::Back);
        assert_eq!(
            back_effects,
            vec![Effect::StopWatchingConversation(
                app.threads[selected].id.clone()
            )]
        );
        assert_eq!(app.selected, selected);
        assert_eq!(app.view, View::Registry);
    }

    #[test]
    fn idle_interrupt_exits_thread_and_releases_watch() {
        let mut app = app();
        reduce(&mut app, Action::OpenSelected);
        let thread_id = app.current_thread_id().expect("thread").clone();
        let effects = reduce(&mut app, Action::InterruptCurrent);
        assert_eq!(app.view, View::Registry);
        assert_eq!(effects, vec![Effect::StopWatchingConversation(thread_id)]);
    }

    #[test]
    fn quick_prompt_cannot_start_turn_before_history_resolves_active_turn_state() {
        let mut app = app();
        reduce(&mut app, Action::QuickPrompt);
        reduce(&mut app, Action::InputChar('h'));
        reduce(&mut app, Action::InputChar('i'));
        let effects = reduce(&mut app, Action::CommitInput);
        assert!(effects.is_empty());
        assert_eq!(app.input_mode, InputMode::Composer);
        let thread_id = app.current_thread_id().expect("thread").clone();
        assert_eq!(app.thread_ui.get(&thread_id.0).expect("ui").draft, "hi");
    }

    #[test]
    fn current_thread_interactive_request_pauses_composer_without_losing_draft() {
        let mut app = app();
        reduce(&mut app, Action::QuickPrompt);
        reduce(&mut app, Action::InputChar('x'));
        let thread_id = app.current_thread_id().expect("thread").clone();
        reduce(
            &mut app,
            Action::InteractiveRequested(InteractiveRequest {
                request_id: RpcRequestId::Integer(8),
                thread_id: thread_id.clone(),
                turn_id: "turn-1".into(),
                item_id: "item-1".into(),
                kind: InteractiveRequestKind::FileChangeApproval { reason: None },
            }),
        );
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.thread_ui[&thread_id.0].draft, "x");
    }

    #[test]
    fn approval_is_not_resolved_without_explicit_user_action() {
        let mut app = app();
        reduce(&mut app, Action::OpenSelected);
        let thread_id = app.current_thread_id().expect("thread").clone();
        reduce(
            &mut app,
            Action::InteractiveRequested(InteractiveRequest {
                request_id: RpcRequestId::Integer(9),
                thread_id,
                turn_id: "turn-1".into(),
                item_id: "item-1".into(),
                kind: InteractiveRequestKind::CommandApproval {
                    command: "cargo test".into(),
                    cwd: "/repo".into(),
                    reason: None,
                },
            }),
        );
        assert_eq!(app.pending_requests.len(), 1);
        let effects = reduce(
            &mut app,
            Action::ResolvePending(InteractiveResolution::Accept),
        );
        assert_eq!(
            effects,
            vec![Effect::ResolveInteractive {
                request_id: RpcRequestId::Integer(9),
                resolution: InteractiveResolution::Accept,
            }]
        );
    }

    #[test]
    fn multi_question_user_input_is_collected_sequentially() {
        let mut app = app();
        reduce(&mut app, Action::OpenSelected);
        let thread_id = app.current_thread_id().expect("thread").clone();
        reduce(
            &mut app,
            Action::InteractiveRequested(InteractiveRequest {
                request_id: RpcRequestId::String("req-1".into()),
                thread_id,
                turn_id: "turn-1".into(),
                item_id: "item-1".into(),
                kind: InteractiveRequestKind::UserInput {
                    questions: vec![
                        UserInputQuestion {
                            id: "q1".into(),
                            header: "One".into(),
                            question: "First?".into(),
                            is_secret: false,
                            options: vec![],
                        },
                        UserInputQuestion {
                            id: "q2".into(),
                            header: "Two".into(),
                            question: "Second?".into(),
                            is_secret: true,
                            options: vec![],
                        },
                    ],
                },
            }),
        );
        reduce(&mut app, Action::BeginUserInput);
        for ch in "alpha".chars() {
            reduce(&mut app, Action::InputChar(ch));
        }
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        for ch in "beta".chars() {
            reduce(&mut app, Action::InputChar(ch));
        }
        let effects = reduce(&mut app, Action::CommitInput);
        assert_eq!(effects.len(), 1);
        let Effect::ResolveInteractive { resolution, .. } = &effects[0] else {
            panic!("expected interactive resolution");
        };
        let InteractiveResolution::UserInput(answers) = resolution else {
            panic!("expected user input answers");
        };
        assert_eq!(answers["q1"], vec!["alpha"]);
        assert_eq!(answers["q2"], vec!["beta"]);
    }

    #[test]
    fn free_text_user_input_preserves_commas() {
        let mut app = app();
        reduce(&mut app, Action::OpenSelected);
        let thread_id = app.current_thread_id().expect("thread").clone();
        reduce(
            &mut app,
            Action::InteractiveRequested(InteractiveRequest {
                request_id: RpcRequestId::Integer(10),
                thread_id,
                turn_id: "turn-1".into(),
                item_id: "item-1".into(),
                kind: InteractiveRequestKind::UserInput {
                    questions: vec![UserInputQuestion {
                        id: "q1".into(),
                        header: "Token".into(),
                        question: "Enter value".into(),
                        is_secret: true,
                        options: vec![],
                    }],
                },
            }),
        );
        reduce(&mut app, Action::BeginUserInput);
        for ch in "alpha,beta".chars() {
            reduce(&mut app, Action::InputChar(ch));
        }
        let effects = reduce(&mut app, Action::CommitInput);
        let Effect::ResolveInteractive { resolution, .. } = &effects[0] else {
            panic!("expected interactive resolution");
        };
        let InteractiveResolution::UserInput(answers) = resolution else {
            panic!("expected user input answers");
        };
        assert_eq!(answers["q1"], vec!["alpha,beta"]);
    }

    #[test]
    fn page_up_at_history_top_requests_older_page_when_cursor_exists() {
        let mut app = app();
        let thread_id = app.selected_thread_id().expect("thread");
        reduce(&mut app, Action::OpenSelected);
        reduce(
            &mut app,
            Action::ConversationLoaded(ConversationPage {
                thread_id: thread_id.clone(),
                title: None,
                turns: vec![],
                items: vec![],
                next_turn_cursor: Some("turn-cursor".into()),
                next_item_cursor: Some("item-cursor".into()),
            }),
        );
        let effects = reduce(&mut app, Action::ScrollBy(-5));
        assert_eq!(
            effects,
            vec![Effect::LoadOlderConversation {
                thread_id,
                turn_cursor: Some("turn-cursor".into()),
                item_cursor: Some("item-cursor".into()),
            }]
        );
    }

    #[test]
    fn repeated_page_up_does_not_queue_duplicate_older_requests() {
        let mut app = app();
        let thread_id = app.selected_thread_id().expect("thread");
        reduce(&mut app, Action::OpenSelected);
        reduce(
            &mut app,
            Action::ConversationLoaded(ConversationPage {
                thread_id,
                title: None,
                turns: vec![],
                items: vec![],
                next_turn_cursor: Some("turn-cursor".into()),
                next_item_cursor: Some("item-cursor".into()),
            }),
        );
        assert_eq!(reduce(&mut app, Action::ScrollBy(-5)).len(), 1);
        assert!(reduce(&mut app, Action::ScrollBy(-5)).is_empty());
    }

    #[test]
    fn review_returns_to_originating_view_and_selects_changed_files() {
        let mut app = app();
        app.threads[0].metadata.cwd = "/repo".into();
        app.git_contexts.insert(
            "thread-impl".into(),
            GitContext::pending(ThreadId::new("thread-impl"), "/repo"),
        );
        let effects = reduce(&mut app, Action::OpenReview);
        assert!(matches!(app.view, View::Review(_)));
        assert_eq!(
            effects,
            vec![Effect::LoadGitReview {
                thread_id: ThreadId::new("thread-impl"),
                cwd: "/repo".into(),
            }]
        );
        reduce(
            &mut app,
            Action::GitReviewLoaded(GitReview {
                thread_id: ThreadId::new("thread-impl"),
                cwd: "/repo".into(),
                changes: vec![
                    crate::git::GitFileChange {
                        path: "a.rs".into(),
                        original_path: None,
                        index_status: Some('M'),
                        worktree_status: None,
                        untracked: false,
                        conflict: false,
                    },
                    crate::git::GitFileChange {
                        path: "b.rs".into(),
                        original_path: None,
                        index_status: None,
                        worktree_status: Some('M'),
                        untracked: false,
                        conflict: false,
                    },
                ],
                staged_diff: String::new(),
                unstaged_diff: String::new(),
                truncated: false,
                observed_at_unix_ms: 1,
                error: None,
            }),
        );
        reduce(&mut app, Action::MoveReview(1));
        assert_eq!(app.selected_review_change().expect("change").path, "b.rs");
        reduce(&mut app, Action::Back);
        assert_eq!(app.view, View::Registry);
    }

    #[test]
    fn git_projection_probe_is_emitted_once_until_cwd_changes() {
        let mut app = app();
        let effects = reduce(&mut app, Action::RefreshGitProjections);
        assert_eq!(effects.len(), 4);
        assert!(reduce(&mut app, Action::RefreshGitProjections).is_empty());

        app.threads[0].metadata.cwd = "/new/cwd".into();
        let effects = reduce(&mut app, Action::RefreshGitProjections);
        assert_eq!(
            effects,
            vec![Effect::ProbeGit {
                thread_id: ThreadId::new("thread-impl"),
                cwd: "/new/cwd".into(),
            }]
        );
    }

    #[test]
    fn collision_is_derived_from_active_threads_sharing_one_worktree() {
        let mut app = app();
        app.threads[0].runtime = RuntimeStatus::Working;
        app.threads[1].runtime = RuntimeStatus::WaitingHuman;

        let repo = crate::domain::LocalRepoIdentity {
            git_common_dir: "/repo/.git".into(),
            primary_root: "/repo".into(),
        };
        for thread_id in ["thread-impl", "thread-kws"] {
            let mut context = GitContext::pending(ThreadId::new(thread_id), "/repo");
            context.is_repository = true;
            context.repo = Some(repo.clone());
            context.worktree = Some(crate::domain::WorktreeIdentity {
                repo: repo.clone(),
                canonical_path: "/repo".into(),
                branch: Some("main".into()),
                managed_by_codex_tui: false,
            });
            app.git_contexts.insert(thread_id.into(), context);
        }

        assert_eq!(
            app.worktree_collision_count(&ThreadId::new("thread-impl")),
            1
        );
    }

    #[test]
    fn conversation_page_is_scoped_by_exact_thread_id() {
        let mut app = app();
        let thread_id = app.selected_thread_id().expect("thread");
        reduce(&mut app, Action::OpenSelected);
        reduce(
            &mut app,
            Action::ConversationLoaded(ConversationPage {
                thread_id: thread_id.clone(),
                title: Some("title".into()),
                turns: vec![],
                items: vec![],
                next_turn_cursor: None,
                next_item_cursor: None,
            }),
        );
        let conversation = app.conversations.get(&thread_id.0).expect("conversation");
        assert!(!conversation.loading);
        assert_eq!(conversation.title.as_deref(), Some("title"));
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
        let original_pin = app.threads[0].pinned;
        let effects = reduce(&mut app, Action::TogglePin);
        assert!(effects.is_empty());
        assert_eq!(app.threads[0].pinned, original_pin);
    }

    #[test]
    fn pin_alias_and_ack_round_trip_through_local_state() {
        let mut app = app();
        app.selected = 1;
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
        assert!(restored.threads[1].pinned);
        assert_eq!(restored.threads[1].alias.as_deref(), Some("primary"));
        assert!(restored.acknowledged_attention.contains("thread-kws"));
    }
}
