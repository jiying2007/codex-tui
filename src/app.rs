use crate::backend::BackendStatus;
use crate::conversation::{
    ConversationPage, ConversationState, InteractiveRequest, InteractiveRequestKind,
    InteractiveResolution, RpcRequestId, UserInputQuestion,
};
use crate::domain::{AttentionReason, RuntimeStatus, ThreadId, ThreadSummary, ThreadUiState};
use crate::forge::ForgeObservation;
use crate::git::{GitContext, GitReview};
use crate::goal::{GoalObservation, GoalStatus};
use crate::operation::{
    ManagedWorktreeRecord, MutationScope, OperationPlan, OperationReceipt, OperationState,
    mutation_scope_for_thread, now_unix_ms,
};
use crate::planning::{
    PlanningSnapshot, ReconcileInput, SavedView, SavedViewLayout, SourceKind, SourceRef,
    WorkCardProjection, WorkflowStage, apply_saved_view, builtin_saved_views,
    reconcile_scratch_card_with_local, reconcile_thread_card_with_goal_and_forge,
};
use crate::store::LocalStateV1;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Registry,
    Thread(ThreadId),
    Review(ThreadId),
    Workspace(ThreadId),
    ManagedWorktrees(ThreadId),
    Board,
    Scratch(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    Registry,
    Thread,
    Review,
    Workspace,
    ManagedWorktrees,
    Board,
    Scratch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
    Alias,
    Composer,
    UserInput,
    ScratchTitle,
    Snooze,
    Note,
    SavedViewName,
    GoalObjective,
    WorktreeCreateBranch,
    WorktreeCreatePath,
    WorktreeCreateStartPoint,
    WorktreeDeleteBranch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextChoice {
    Snooze,
    EditNote,
    Bookmark,
    ScratchInbox,
    ScratchReady,
    ScratchDone,
    DeleteScratch,
    SaveCurrentView,
    DeleteCurrentView,
}

impl ContextChoice {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Snooze => "Snooze attention…",
            Self::EditNote => "Edit local note…",
            Self::Bookmark => "Add local bookmark",
            Self::ScratchInbox => "Scratch → Inbox",
            Self::ScratchReady => "Scratch → Ready",
            Self::ScratchDone => "Scratch → Done",
            Self::DeleteScratch => "Delete local ScratchWork",
            Self::SaveCurrentView => "Save current view as…",
            Self::DeleteCurrentView => "Delete current SavedView",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    ReplaceThreads(Vec<ThreadSummary>),
    BackendStatus(BackendStatus),
    RefreshGitProjections,
    GitContextLoaded(GitContext),
    ForgeObservationLoaded(ForgeObservation),
    GitReviewLoaded(GitReview),
    ReviewError { thread_id: ThreadId, error: String },
    PlanningSnapshotLoaded(PlanningSnapshot),
    ReconcilePlanning { now_unix_ms: u64 },
    PlanningStoreDegraded(Option<String>),
    GoalObserved(GoalObservation),
    GoalCleared(ThreadId),
    OpenGoalActions,
    CloseGoalActions,
    OpenManagedWorktrees,
    ManagedWorktreesLoaded(Vec<ManagedWorktreeRecord>),
    MutationReceipt(Box<OperationReceipt>),
    MutationNotice(String),
    MoveManagedWorktree(i32),
    BeginCreateWorktree,
    BeginAdoptCurrentWorktree,
    BeginRemoveManagedWorktree,
    BeginDeleteBranch,
    ConfirmPendingOperation,
    CancelPendingOperation,
    BeginGoalObjective,
    SetGoalStatus(GoalStatus),
    ClearGoal,
    OpenReview,
    OpenWorkspace,
    OpenBoard,
    MoveBoardColumn(i32),
    MovePlanningSelection(i32),
    CycleSavedView(i32),
    OpenPlanningSelected,
    BeginScratch,
    BeginSnooze,
    OpenContext,
    CloseContext,
    MoveContext(i32),
    ExecuteContext,
    BeginHotSlotBind,
    UseHotSlot(u8),
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
    CreateScratch {
        title: String,
        workspace: Option<String>,
    },
    SnoozeWorkCard {
        anchor: SourceRef,
        duration_ms: u64,
    },
    SaveSourceNote {
        owner: SourceRef,
        text: String,
    },
    UpdateScratchNote {
        scratch_id: String,
        note: Option<String>,
    },
    CreateBookmark {
        source: SourceRef,
        label: Option<String>,
    },
    UpdateScratchState {
        scratch_id: String,
        state: crate::planning::ScratchState,
    },
    DeleteScratch {
        scratch_id: String,
    },
    SaveSavedView {
        view: SavedView,
    },
    DeleteSavedView {
        view_id: String,
    },
    SetHotSlot {
        slot: u8,
        target: SourceRef,
    },
    RefreshGoal(ThreadId),
    SetGoal {
        thread_id: ThreadId,
        objective: Option<String>,
        status: Option<GoalStatus>,
    },
    ClearGoal(ThreadId),
    RefreshManagedWorktrees,
    ExecuteOperation(Box<OperationPlan>),
    ProbeGit {
        thread_id: ThreadId,
        cwd: String,
    },
    ProbeForge {
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
    pub forge_observations: BTreeMap<String, ForgeObservation>,
    pub git_reviews: BTreeMap<String, GitReview>,
    pub planning_snapshot: PlanningSnapshot,
    pub work_cards: Vec<WorkCardProjection>,
    pub goals: BTreeMap<String, GoalObservation>,
    pub goal_checked: BTreeSet<String>,
    pub goal_actions_open: bool,
    pub managed_worktrees: Vec<ManagedWorktreeRecord>,
    pub managed_selected: usize,
    pub managed_return_view: Option<View>,
    pub pending_operation: Option<OperationPlan>,
    pub recent_operations: Vec<OperationReceipt>,
    pub mutation_notice: Option<String>,
    pub create_worktree_branch: Option<String>,
    pub create_worktree_path: Option<String>,
    pub planning_store_error: Option<String>,
    pub review_return_view: Option<View>,
    pub workspace_return_view: Option<View>,
    pub board_return_view: Option<View>,
    pub planning_view_index: usize,
    pub board_stage_index: usize,
    pub board_selected: usize,
    pub new_scratch_workspace: Option<String>,
    pub snooze_target: Option<SourceRef>,
    pub note_target: Option<SourceRef>,
    pub saved_view_template: Option<SavedView>,
    pub context_open: bool,
    pub context_selected: usize,
    pub hot_slot_bind_pending: bool,
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
            forge_observations: BTreeMap::new(),
            git_reviews: BTreeMap::new(),
            planning_snapshot: PlanningSnapshot::default(),
            work_cards: vec![],
            goals: BTreeMap::new(),
            goal_checked: BTreeSet::new(),
            goal_actions_open: false,
            managed_worktrees: vec![],
            managed_selected: 0,
            managed_return_view: None,
            pending_operation: None,
            recent_operations: vec![],
            mutation_notice: None,
            create_worktree_branch: None,
            create_worktree_path: None,
            planning_store_error: None,
            review_return_view: None,
            workspace_return_view: None,
            board_return_view: None,
            planning_view_index: 0,
            board_stage_index: 0,
            board_selected: 0,
            new_scratch_workspace: None,
            snooze_target: None,
            note_target: None,
            saved_view_template: None,
            context_open: false,
            context_selected: 0,
            hot_slot_bind_pending: false,
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
            View::Workspace(_) => ViewKind::Workspace,
            View::ManagedWorktrees(_) => ViewKind::ManagedWorktrees,
            View::Board => ViewKind::Board,
            View::Scratch(_) => ViewKind::Scratch,
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
            View::Thread(id)
            | View::Review(id)
            | View::Workspace(id)
            | View::ManagedWorktrees(id) => Some(id),
            View::Board | View::Scratch(_) => None,
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

    pub fn forge_observation(&self, thread_id: &ThreadId) -> Option<&ForgeObservation> {
        self.forge_observations.get(&thread_id.0)
    }

    pub fn current_goal(&self) -> Option<&GoalObservation> {
        let thread_id = self.current_thread_id()?;
        self.goals.get(&thread_id.0)
    }

    pub fn visible_managed_worktrees(&self) -> Vec<&ManagedWorktreeRecord> {
        let repo = self
            .current_thread_id()
            .and_then(|thread_id| self.git_context(thread_id))
            .and_then(|context| context.repo.as_ref());
        self.managed_worktrees
            .iter()
            .filter(|record| repo.is_none_or(|repo| &record.repo == repo))
            .collect()
    }

    pub fn selected_managed_worktree(&self) -> Option<&ManagedWorktreeRecord> {
        self.visible_managed_worktrees()
            .get(self.managed_selected)
            .copied()
    }

    pub fn active_mutation_scopes(&self) -> Vec<MutationScope> {
        self.threads
            .iter()
            .filter(|thread| {
                matches!(
                    thread.runtime,
                    RuntimeStatus::Working | RuntimeStatus::WaitingHuman
                ) || self.goals.get(&thread.id.0).is_some_and(|goal| {
                    matches!(
                        goal.status,
                        GoalStatus::Active
                            | GoalStatus::Blocked
                            | GoalStatus::UsageLimited
                            | GoalStatus::BudgetLimited
                    )
                })
            })
            .map(|thread| mutation_scope_for_thread(thread, self.git_context(&thread.id)))
            .collect()
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

    pub fn planning_views(&self) -> Vec<SavedView> {
        let mut views = builtin_saved_views();
        views.extend(self.planning_snapshot.saved_views.iter().cloned());
        views
    }

    pub fn active_saved_view(&self) -> SavedView {
        let views = self.planning_views();
        views
            .get(self.planning_view_index.min(views.len().saturating_sub(1)))
            .cloned()
            .unwrap_or_else(|| builtin_saved_views().remove(0))
    }

    pub fn visible_planning_cards(&self) -> Vec<&WorkCardProjection> {
        let view = self.active_saved_view();
        let mut cards = apply_saved_view(&self.work_cards, &view);
        if view.layout == SavedViewLayout::Board {
            let stage = WorkflowStage::ALL[self.board_stage_index % WorkflowStage::ALL.len()];
            cards.retain(|card| card.stage == stage);
        }
        cards
    }

    pub fn selected_planning_card(&self) -> Option<&WorkCardProjection> {
        self.visible_planning_cards()
            .get(self.board_selected)
            .copied()
    }

    pub fn selected_local_target(&self) -> Option<SourceRef> {
        match &self.view {
            View::Registry => self
                .selected_thread_id()
                .map(|thread_id| SourceRef::codex_thread(&thread_id)),
            View::Board => self
                .selected_planning_card()
                .map(|card| card.anchor.clone()),
            View::Thread(id)
            | View::Review(id)
            | View::Workspace(id)
            | View::ManagedWorktrees(id) => Some(SourceRef::codex_thread(id)),
            View::Scratch(id) => Some(SourceRef {
                kind: SourceKind::ScratchWork,
                value: id.clone(),
            }),
        }
    }

    pub fn context_choices(&self) -> Vec<ContextChoice> {
        let mut choices = Vec::new();
        if let Some(target) = self.selected_local_target() {
            choices.extend([
                ContextChoice::Snooze,
                ContextChoice::EditNote,
                ContextChoice::Bookmark,
            ]);
            if target.kind == SourceKind::ScratchWork {
                choices.extend([
                    ContextChoice::ScratchInbox,
                    ContextChoice::ScratchReady,
                    ContextChoice::ScratchDone,
                    ContextChoice::DeleteScratch,
                ]);
            }
        }
        if matches!(self.view, View::Board) {
            choices.push(ContextChoice::SaveCurrentView);
            if self.active_saved_view().id.starts_with("view:") {
                choices.push(ContextChoice::DeleteCurrentView);
            }
        }
        choices
    }

    pub fn context_choice(&self) -> Option<ContextChoice> {
        self.context_choices().get(self.context_selected).copied()
    }

    pub fn work_card_for_thread(&self, thread_id: &ThreadId) -> Option<&WorkCardProjection> {
        self.work_cards
            .iter()
            .find(|card| card.anchor == SourceRef::codex_thread(thread_id))
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
        if self
            .work_card_for_thread(&thread.id)
            .is_some_and(|card| card.snoozed)
        {
            return false;
        }
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
            state.forge_observations.retain(|thread_id, _| {
                state.threads.iter().any(|thread| thread.id.0 == *thread_id)
            });
            return effects;
        }
        Action::GitContextLoaded(context) => {
            let thread_id = context.thread_id.clone();
            let cwd = context.cwd.clone();
            let should_probe_forge = context.is_repository
                && context.error.is_none()
                && state
                    .forge_observations
                    .get(&thread_id.0)
                    .is_none_or(|observation| observation.cwd != cwd);
            state.git_contexts.insert(thread_id.0.clone(), context);
            if should_probe_forge {
                return vec![Effect::ProbeForge { thread_id, cwd }];
            }
        }
        Action::ForgeObservationLoaded(observation) => {
            state
                .forge_observations
                .insert(observation.thread_id.0.clone(), observation);
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
        Action::PlanningSnapshotLoaded(snapshot) => {
            state.planning_snapshot = snapshot;
            state.planning_view_index = state
                .planning_view_index
                .min(state.planning_views().len().saturating_sub(1));
            state.board_selected = 0;
        }
        Action::PlanningStoreDegraded(error) => {
            state.planning_store_error = error;
        }
        Action::OpenManagedWorktrees => {
            let Some(thread_id) = state.current_thread_id().cloned() else {
                return vec![];
            };
            if state
                .git_context(&thread_id)
                .and_then(|context| context.repo.as_ref())
                .is_none()
            {
                state.mutation_notice = Some("current thread is not in a Git repository".into());
                return vec![];
            }
            if !matches!(state.view, View::ManagedWorktrees(_)) {
                state.managed_return_view = Some(state.view.clone());
            }
            state.view = View::ManagedWorktrees(thread_id);
            state.managed_selected = 0;
            state.pending_operation = None;
            state.mutation_notice = None;
            return vec![Effect::RefreshManagedWorktrees];
        }
        Action::ManagedWorktreesLoaded(records) => {
            state.managed_worktrees = records;
            state.managed_selected = state
                .managed_selected
                .min(state.visible_managed_worktrees().len().saturating_sub(1));
        }
        Action::MutationReceipt(receipt) => {
            let receipt = *receipt;
            if state
                .pending_operation
                .as_ref()
                .is_some_and(|plan| plan.operation_id == receipt.operation_id)
            {
                state.pending_operation = None;
            }
            state.mutation_notice = Some(format!(
                "{} · {}",
                receipt.plan.kind.label(),
                match receipt.state {
                    OperationState::Planned => "planned",
                    OperationState::Executing => "executing",
                    OperationState::Succeeded => "succeeded",
                    OperationState::Failed => "failed",
                    OperationState::OutcomeUnknown => "outcome unknown",
                }
            ));
            let repo = receipt.plan.repo.clone();
            state
                .git_contexts
                .retain(|_, context| context.repo.as_ref() != Some(&repo));
            state.forge_observations.retain(|thread_id, _| {
                state
                    .git_contexts
                    .get(thread_id)
                    .is_some_and(|context| context.repo.as_ref() != Some(&repo))
            });
            state
                .recent_operations
                .retain(|item| item.operation_id != receipt.operation_id);
            state.recent_operations.insert(0, receipt);
            state.recent_operations.truncate(20);
        }
        Action::MutationNotice(notice) => {
            state.mutation_notice = Some(notice);
        }
        Action::MoveManagedWorktree(delta) => {
            let len = state.visible_managed_worktrees().len();
            if len == 0 {
                state.managed_selected = 0;
            } else {
                state.managed_selected =
                    (state.managed_selected as i32 + delta).rem_euclid(len as i32) as usize;
            }
        }
        Action::BeginCreateWorktree => {
            let Some(thread_id) = state.current_thread_id().cloned() else {
                return vec![];
            };
            let Some(context) = state.git_context(&thread_id) else {
                state.mutation_notice = Some("Git context is unavailable".into());
                return vec![];
            };
            if context.repo.is_none() {
                state.mutation_notice = Some("current cwd is not a Git repository".into());
                return vec![];
            }
            state.pending_operation = None;
            state.create_worktree_branch = None;
            state.create_worktree_path = None;
            state.input_buffer.clear();
            state.input_mode = InputMode::WorktreeCreateBranch;
        }
        Action::BeginAdoptCurrentWorktree => {
            let Some(thread_id) = state.current_thread_id().cloned() else {
                return vec![];
            };
            let Some(context) = state.git_context(&thread_id) else {
                return vec![];
            };
            let (Some(repo), Some(worktree)) = (&context.repo, &context.worktree) else {
                state.mutation_notice = Some("current cwd is not a Git worktree".into());
                return vec![];
            };
            if state.managed_worktrees.iter().any(|record| {
                record.repo == *repo && record.canonical_path == worktree.canonical_path
            }) {
                state.mutation_notice = Some("current worktree is already managed/adopted".into());
                return vec![];
            }
            state.pending_operation = Some(OperationPlan::adopt_worktree(
                repo.clone(),
                repo.primary_root.clone(),
                worktree.canonical_path.clone(),
                now_unix_ms(),
            ));
            state.mutation_notice = None;
        }
        Action::BeginRemoveManagedWorktree => {
            let Some(record) = state.selected_managed_worktree().cloned() else {
                return vec![];
            };
            state.pending_operation = Some(OperationPlan::remove_worktree(
                record.repo.clone(),
                record.repo.primary_root.clone(),
                record.canonical_path,
                now_unix_ms(),
            ));
            state.mutation_notice = None;
        }
        Action::BeginDeleteBranch => {
            let Some(thread_id) = state.current_thread_id().cloned() else {
                return vec![];
            };
            let Some(context) = state.git_context(&thread_id) else {
                return vec![];
            };
            if context.repo.is_none() {
                return vec![];
            }
            state.input_buffer = state
                .selected_managed_worktree()
                .and_then(|record| record.branch.clone())
                .or_else(|| context.branch.clone())
                .unwrap_or_default();
            state.input_mode = InputMode::WorktreeDeleteBranch;
        }
        Action::ConfirmPendingOperation => {
            if state.planning_store_error.is_some() {
                state.mutation_notice =
                    Some("local store is degraded; mutation receipts cannot be persisted".into());
                return vec![];
            }
            let Some(plan) = state.pending_operation.take() else {
                return vec![];
            };
            return vec![Effect::ExecuteOperation(Box::new(plan))];
        }
        Action::CancelPendingOperation => {
            state.pending_operation = None;
            state.mutation_notice = Some("operation cancelled before execution".into());
        }
        Action::GoalObserved(goal) => {
            state.goal_checked.insert(goal.thread_id.0.clone());
            state.goals.insert(goal.thread_id.0.clone(), goal);
        }
        Action::GoalCleared(thread_id) => {
            state.goal_checked.insert(thread_id.0.clone());
            state.goals.remove(&thread_id.0);
            state.goal_actions_open = false;
        }
        Action::OpenGoalActions => {
            if state.current_pending_request().is_some() {
                return vec![];
            }
            if let Some(thread_id) = state.current_thread_id().cloned() {
                state.goal_actions_open = true;
                if !state.goal_checked.contains(&thread_id.0) {
                    return vec![Effect::RefreshGoal(thread_id)];
                }
            }
        }
        Action::CloseGoalActions => {
            state.goal_actions_open = false;
        }
        Action::BeginGoalObjective => {
            let Some(thread_id) = state.current_thread_id().cloned() else {
                return vec![];
            };
            if state
                .backend_status
                .optional_capabilities_missing
                .iter()
                .any(|capability| capability == "thread/goal/get")
            {
                return vec![];
            }
            if !state.goal_checked.contains(&thread_id.0) {
                return vec![Effect::RefreshGoal(thread_id)];
            }
            state.input_buffer = state
                .goals
                .get(&thread_id.0)
                .map(|goal| goal.objective.clone())
                .unwrap_or_default();
            state.goal_actions_open = false;
            state.input_mode = InputMode::GoalObjective;
        }
        Action::SetGoalStatus(status) => {
            let Some(thread_id) = state.current_thread_id().cloned() else {
                return vec![];
            };
            if !state.goals.contains_key(&thread_id.0) {
                return vec![];
            }
            state.goal_actions_open = false;
            return vec![Effect::SetGoal {
                thread_id,
                objective: None,
                status: Some(status),
            }];
        }
        Action::ClearGoal => {
            let Some(thread_id) = state.current_thread_id().cloned() else {
                return vec![];
            };
            if !state.goals.contains_key(&thread_id.0) {
                return vec![];
            }
            state.goal_actions_open = false;
            return vec![Effect::ClearGoal(thread_id)];
        }
        Action::ReconcilePlanning { now_unix_ms } => {
            rebuild_planning(state, now_unix_ms);
        }
        Action::OpenBoard => {
            if !matches!(state.view, View::Board) {
                state.board_return_view = Some(state.view.clone());
            }
            state.view = View::Board;
            state.board_selected = 0;
        }
        Action::MoveBoardColumn(delta) => {
            let len = WorkflowStage::ALL.len() as i32;
            state.board_stage_index =
                (state.board_stage_index as i32 + delta).rem_euclid(len) as usize;
            state.board_selected = 0;
        }
        Action::MovePlanningSelection(delta) => {
            let len = state.visible_planning_cards().len();
            if len == 0 {
                state.board_selected = 0;
            } else {
                state.board_selected =
                    (state.board_selected as i32 + delta).rem_euclid(len as i32) as usize;
            }
        }
        Action::CycleSavedView(delta) => {
            let len = state.planning_views().len();
            if len == 0 {
                state.planning_view_index = 0;
            } else {
                state.planning_view_index =
                    (state.planning_view_index as i32 + delta).rem_euclid(len as i32) as usize;
            }
            state.board_selected = 0;
        }
        Action::OpenPlanningSelected => {
            let Some(card) = state.selected_planning_card().cloned() else {
                return vec![];
            };
            match card.anchor.kind {
                SourceKind::CodexThread => {
                    let id = ThreadId::new(card.anchor.value);
                    if state.threads.iter().any(|thread| thread.id == id) {
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
                SourceKind::ScratchWork => {
                    state.view = View::Scratch(card.anchor.value);
                }
                _ => {}
            }
        }
        Action::BeginScratch => {
            state.new_scratch_workspace = match &state.view {
                View::Registry => state
                    .selected_thread()
                    .map(|thread| thread.workspace.clone()),
                View::Board => state
                    .selected_planning_card()
                    .and_then(|card| card.workspace.clone()),
                View::Thread(id)
                | View::Review(id)
                | View::Workspace(id)
                | View::ManagedWorktrees(id) => state
                    .threads
                    .iter()
                    .find(|thread| thread.id == *id)
                    .map(|thread| thread.workspace.clone()),
                View::Scratch(id) => state
                    .planning_snapshot
                    .scratch
                    .iter()
                    .find(|scratch| scratch.id == *id)
                    .and_then(|scratch| scratch.workspace.clone()),
            };
            state.input_buffer.clear();
            state.input_mode = InputMode::ScratchTitle;
        }
        Action::OpenContext => {
            if !state.context_choices().is_empty() {
                state.context_open = true;
                state.context_selected = 0;
            }
        }
        Action::CloseContext => {
            state.context_open = false;
            state.context_selected = 0;
        }
        Action::MoveContext(delta) => {
            let len = state.context_choices().len();
            if len == 0 {
                state.context_selected = 0;
            } else {
                state.context_selected =
                    (state.context_selected as i32 + delta).rem_euclid(len as i32) as usize;
            }
        }
        Action::ExecuteContext => {
            let Some(choice) = state.context_choice() else {
                state.context_open = false;
                return vec![];
            };
            state.context_open = false;
            state.context_selected = 0;

            if choice == ContextChoice::SaveCurrentView {
                let mut template = state.active_saved_view();
                template.id.clear();
                state.input_buffer = format!("{} Copy", template.name);
                state.saved_view_template = Some(template);
                state.input_mode = InputMode::SavedViewName;
                return vec![];
            }
            if choice == ContextChoice::DeleteCurrentView {
                let view_id = state.active_saved_view().id;
                if view_id.starts_with("view:") {
                    return vec![Effect::DeleteSavedView { view_id }];
                }
                return vec![];
            }

            let Some(target) = state.selected_local_target() else {
                return vec![];
            };
            match choice {
                ContextChoice::Snooze => {
                    state.snooze_target = Some(target);
                    state.input_buffer = "1h".into();
                    state.input_mode = InputMode::Snooze;
                }
                ContextChoice::EditNote => {
                    let existing = if target.kind == SourceKind::ScratchWork {
                        state
                            .planning_snapshot
                            .scratch
                            .iter()
                            .find(|scratch| scratch.id == target.value)
                            .and_then(|scratch| scratch.note.clone())
                    } else {
                        state
                            .planning_snapshot
                            .notes
                            .iter()
                            .find(|note| note.owner == target)
                            .map(|note| note.text.clone())
                    };
                    state.note_target = Some(target);
                    state.input_buffer = existing.unwrap_or_default();
                    state.input_mode = InputMode::Note;
                }
                ContextChoice::Bookmark => {
                    let label = state
                        .work_cards
                        .iter()
                        .find(|card| card.anchor == target)
                        .map(|card| card.title.clone());
                    return vec![Effect::CreateBookmark {
                        source: target,
                        label,
                    }];
                }
                ContextChoice::ScratchInbox
                | ContextChoice::ScratchReady
                | ContextChoice::ScratchDone => {
                    let scratch_state = match choice {
                        ContextChoice::ScratchInbox => crate::planning::ScratchState::Inbox,
                        ContextChoice::ScratchReady => crate::planning::ScratchState::Ready,
                        ContextChoice::ScratchDone => crate::planning::ScratchState::Done,
                        _ => unreachable!(),
                    };
                    return vec![Effect::UpdateScratchState {
                        scratch_id: target.value,
                        state: scratch_state,
                    }];
                }
                ContextChoice::DeleteScratch => {
                    if matches!(state.view, View::Scratch(_)) {
                        state.view = View::Board;
                    }
                    return vec![Effect::DeleteScratch {
                        scratch_id: target.value,
                    }];
                }
                ContextChoice::SaveCurrentView | ContextChoice::DeleteCurrentView => unreachable!(),
            }
        }
        Action::BeginSnooze => {
            if let Some(target) = state.selected_local_target() {
                state.snooze_target = Some(target);
                state.input_buffer = "1h".into();
                state.input_mode = InputMode::Snooze;
            }
        }
        Action::BeginHotSlotBind => {
            state.hot_slot_bind_pending = state.selected_local_target().is_some();
        }
        Action::UseHotSlot(slot) => {
            if state.hot_slot_bind_pending {
                state.hot_slot_bind_pending = false;
                if let Some(target) = state.selected_local_target() {
                    return vec![Effect::SetHotSlot { slot, target }];
                }
                return vec![];
            }

            let target = state
                .planning_snapshot
                .hot_slots
                .iter()
                .find(|hot_slot| hot_slot.slot == slot)
                .map(|hot_slot| hot_slot.target.clone());
            let Some(target) = target else {
                return vec![];
            };
            match target.kind {
                SourceKind::CodexThread => {
                    let id = ThreadId::new(target.value);
                    if let Some(index) = state.threads.iter().position(|thread| thread.id == id) {
                        state.selected = index;
                        state.view = View::Registry;
                    }
                }
                SourceKind::ScratchWork
                    if state
                        .planning_snapshot
                        .scratch
                        .iter()
                        .any(|scratch| scratch.id == target.value) =>
                {
                    state.view = View::Scratch(target.value);
                }
                _ => {}
            }
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
                View::Thread(id)
                | View::Review(id)
                | View::Workspace(id)
                | View::ManagedWorktrees(id) => Some(id.clone()),
                View::Board => state.selected_planning_card().and_then(|card| {
                    (card.anchor.kind == SourceKind::CodexThread)
                        .then(|| ThreadId::new(card.anchor.value.clone()))
                }),
                View::Scratch(_) => None,
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
        Action::OpenWorkspace => {
            let thread_id = match &state.view {
                View::Registry => state.selected_thread_id(),
                View::Thread(id)
                | View::Review(id)
                | View::Workspace(id)
                | View::ManagedWorktrees(id) => Some(id.clone()),
                View::Board => state.selected_planning_card().and_then(|card| {
                    (card.anchor.kind == SourceKind::CodexThread)
                        .then(|| ThreadId::new(card.anchor.value.clone()))
                }),
                View::Scratch(_) => None,
            };
            let Some(thread_id) = thread_id else {
                return vec![];
            };
            if !matches!(state.view, View::Workspace(_)) {
                state.workspace_return_view = Some(state.view.clone());
            }
            state.view = View::Workspace(thread_id.clone());

            let Some(thread) = state.threads.iter().find(|thread| thread.id == thread_id) else {
                return vec![];
            };
            if thread.metadata.cwd.trim().is_empty() {
                return vec![];
            }
            let needs_probe = state
                .git_contexts
                .get(&thread_id.0)
                .is_none_or(|context| context.cwd != thread.metadata.cwd);
            if needs_probe {
                state.git_contexts.insert(
                    thread_id.0.clone(),
                    GitContext::pending(thread_id.clone(), thread.metadata.cwd.clone()),
                );
                return vec![Effect::ProbeGit {
                    thread_id,
                    cwd: thread.metadata.cwd.clone(),
                }];
            }
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
            if is_current_thread {
                state.goal_actions_open = false;
                if state.input_mode == InputMode::Composer {
                    state.input_mode = InputMode::Normal;
                }
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
                None if matches!(state.view, View::Board) => {
                    let Some(card) = state.selected_planning_card() else {
                        return vec![];
                    };
                    if card.anchor.kind != SourceKind::CodexThread {
                        return vec![];
                    }
                    ThreadId::new(card.anchor.value.clone())
                }
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
            if state.pending_operation.is_some() {
                state.pending_operation = None;
                state.mutation_notice = Some("operation cancelled before execution".into());
                return vec![];
            }
            if matches!(state.view, View::ManagedWorktrees(_)) {
                state.view = state.managed_return_view.take().unwrap_or(View::Registry);
                return vec![];
            }
            if state.context_open {
                state.context_open = false;
                state.context_selected = 0;
                return vec![];
            }
            if state.hot_slot_bind_pending {
                state.hot_slot_bind_pending = false;
                return vec![];
            }
            if matches!(state.view, View::Scratch(_)) {
                state.view = View::Board;
                return vec![];
            }
            if matches!(state.view, View::Board) {
                state.view = state.board_return_view.take().unwrap_or(View::Registry);
                return vec![];
            }
            if matches!(state.view, View::Review(_)) {
                state.view = state.review_return_view.take().unwrap_or(View::Registry);
                state.review_scroll = 0;
                return vec![];
            }
            if matches!(state.view, View::Workspace(_)) {
                state.view = state.workspace_return_view.take().unwrap_or(View::Registry);
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
        Action::NextAttention => {
            if matches!(state.view, View::Board) {
                select_next_planning_attention(state);
            } else {
                select_next_attention(state);
            }
        }
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
            InputMode::Search
            | InputMode::Alias
            | InputMode::UserInput
            | InputMode::ScratchTitle
            | InputMode::Snooze
            | InputMode::Note
            | InputMode::SavedViewName
            | InputMode::GoalObjective
            | InputMode::WorktreeCreateBranch
            | InputMode::WorktreeCreatePath
            | InputMode::WorktreeCreateStartPoint
            | InputMode::WorktreeDeleteBranch => {
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
            InputMode::Search
            | InputMode::Alias
            | InputMode::UserInput
            | InputMode::ScratchTitle
            | InputMode::Snooze
            | InputMode::Note
            | InputMode::SavedViewName
            | InputMode::GoalObjective
            | InputMode::WorktreeCreateBranch
            | InputMode::WorktreeCreatePath
            | InputMode::WorktreeCreateStartPoint
            | InputMode::WorktreeDeleteBranch => {
                state.input_buffer.pop();
                if state.input_mode == InputMode::Search {
                    state.filter.clone_from(&state.input_buffer);
                    ensure_selection_visible(state);
                }
            }
        },
        Action::CommitInput => {
            let mode = state.input_mode;
            if mode == InputMode::WorktreeCreateBranch {
                let branch = state.input_buffer.trim().to_string();
                if branch.is_empty() {
                    return vec![];
                }
                state.create_worktree_branch = Some(branch);
                state.input_buffer.clear();
                state.input_mode = InputMode::WorktreeCreatePath;
                return vec![];
            }
            if mode == InputMode::WorktreeCreatePath {
                let path = state.input_buffer.trim().to_string();
                if path.is_empty() {
                    return vec![];
                }
                if !std::path::Path::new(&path).is_absolute() {
                    state.mutation_notice =
                        Some("worktree path must be absolute before a plan can be created".into());
                    return vec![];
                }
                state.create_worktree_path = Some(path);
                state.input_buffer = "HEAD".into();
                state.input_mode = InputMode::WorktreeCreateStartPoint;
                return vec![];
            }
            if mode == InputMode::WorktreeCreateStartPoint {
                let start_point = state.input_buffer.trim().to_string();
                if start_point.is_empty() {
                    return vec![];
                }
                let Some(thread_id) = state.current_thread_id().cloned() else {
                    return vec![];
                };
                let Some(context) = state.git_context(&thread_id) else {
                    return vec![];
                };
                let Some(repo) = context.repo.clone() else {
                    return vec![];
                };
                let Some(branch) = state.create_worktree_branch.take() else {
                    return vec![];
                };
                let Some(path) = state.create_worktree_path.take() else {
                    return vec![];
                };
                state.pending_operation = Some(OperationPlan::create_worktree(
                    repo.clone(),
                    repo.primary_root,
                    path,
                    branch,
                    start_point,
                    now_unix_ms(),
                ));
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![];
            }
            if mode == InputMode::WorktreeDeleteBranch {
                let branch = state.input_buffer.trim().to_string();
                if branch.is_empty() {
                    return vec![];
                }
                let Some(thread_id) = state.current_thread_id().cloned() else {
                    return vec![];
                };
                let Some(repo) = state
                    .git_context(&thread_id)
                    .and_then(|context| context.repo.clone())
                else {
                    return vec![];
                };
                state.pending_operation = Some(OperationPlan::delete_branch(
                    repo.clone(),
                    repo.primary_root,
                    branch,
                    now_unix_ms(),
                ));
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![];
            }
            if mode == InputMode::GoalObjective {
                let Some(thread_id) = state.current_thread_id().cloned() else {
                    state.input_mode = InputMode::Normal;
                    state.input_buffer.clear();
                    return vec![];
                };
                let objective = state.input_buffer.trim().to_string();
                if objective.is_empty() {
                    return vec![];
                }
                let is_new = !state.goals.contains_key(&thread_id.0);
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![Effect::SetGoal {
                    thread_id,
                    objective: Some(objective),
                    status: is_new.then_some(GoalStatus::Active),
                }];
            }
            if mode == InputMode::ScratchTitle {
                let title = state.input_buffer.trim().to_string();
                if title.is_empty() {
                    return vec![];
                }
                let workspace = state.new_scratch_workspace.take();
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![Effect::CreateScratch { title, workspace }];
            }
            if mode == InputMode::SavedViewName {
                let name = state.input_buffer.trim().to_string();
                let Some(mut view) = state.saved_view_template.take() else {
                    state.input_mode = InputMode::Normal;
                    state.input_buffer.clear();
                    return vec![];
                };
                if name.is_empty() {
                    return vec![];
                }
                view.name = name;
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![Effect::SaveSavedView { view }];
            }
            if mode == InputMode::Note {
                let text = state.input_buffer.trim().to_string();
                let Some(target) = state.note_target.take() else {
                    state.input_mode = InputMode::Normal;
                    state.input_buffer.clear();
                    return vec![];
                };
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                if target.kind == SourceKind::ScratchWork {
                    return vec![Effect::UpdateScratchNote {
                        scratch_id: target.value,
                        note: (!text.is_empty()).then_some(text),
                    }];
                }
                return vec![Effect::SaveSourceNote {
                    owner: target,
                    text,
                }];
            }
            if mode == InputMode::Snooze {
                let Some(duration_ms) = parse_snooze_duration(&state.input_buffer) else {
                    return vec![];
                };
                let Some(anchor) = state.snooze_target.take() else {
                    state.input_mode = InputMode::Normal;
                    state.input_buffer.clear();
                    return vec![];
                };
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![Effect::SnoozeWorkCard {
                    anchor,
                    duration_ms,
                }];
            }
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
            if matches!(
                state.input_mode,
                InputMode::WorktreeCreateBranch
                    | InputMode::WorktreeCreatePath
                    | InputMode::WorktreeCreateStartPoint
                    | InputMode::WorktreeDeleteBranch
            ) {
                state.create_worktree_branch = None;
                state.create_worktree_path = None;
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![];
            }
            if state.input_mode == InputMode::GoalObjective {
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![];
            }
            if state.input_mode == InputMode::ScratchTitle {
                state.new_scratch_workspace = None;
            }
            if state.input_mode == InputMode::Snooze {
                state.snooze_target = None;
            }
            if state.input_mode == InputMode::Note {
                state.note_target = None;
            }
            if state.input_mode == InputMode::SavedViewName {
                state.saved_view_template = None;
            }
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

fn parse_snooze_duration(value: &str) -> Option<u64> {
    let value = value.trim().to_ascii_lowercase();
    if value.len() < 2 {
        return None;
    }
    let (number, suffix) = value.split_at(value.len() - 1);
    let amount = number.parse::<u64>().ok()?;
    if amount == 0 {
        return None;
    }
    let unit_ms = match suffix {
        "m" => 60_000,
        "h" => 60 * 60_000,
        "d" => 24 * 60 * 60_000,
        _ => return None,
    };
    amount.checked_mul(unit_ms)
}

fn select_next_planning_attention(state: &mut AppState) {
    let view = state.active_saved_view();
    let cards = apply_saved_view(&state.work_cards, &view);
    if cards.is_empty() {
        return;
    }

    let current_local_id = state
        .selected_planning_card()
        .map(|card| card.local_id.clone());
    let start = current_local_id
        .as_ref()
        .and_then(|local_id| cards.iter().position(|card| &card.local_id == local_id))
        .unwrap_or(0);

    let next = (1..=cards.len()).find_map(|offset| {
        let card = cards[(start + offset) % cards.len()];
        card.needs_you()
            .then(|| (card.local_id.clone(), card.stage))
    });
    drop(cards);

    if let Some((local_id, stage)) = next {
        state.planning_view_index = state
            .planning_views()
            .iter()
            .position(|candidate| candidate.id == view.id)
            .unwrap_or(state.planning_view_index);
        state.board_stage_index = WorkflowStage::ALL
            .iter()
            .position(|candidate| *candidate == stage)
            .unwrap_or(state.board_stage_index);
        let visible = state.visible_planning_cards();
        state.board_selected = visible
            .iter()
            .position(|candidate| candidate.local_id == local_id)
            .unwrap_or(0);
    }
}

fn rebuild_planning(state: &mut AppState, now_unix_ms: u64) {
    let local_by_anchor = state
        .planning_snapshot
        .cards
        .iter()
        .map(|card| (card.anchor.clone(), card))
        .collect::<BTreeMap<_, _>>();

    let mut projections =
        Vec::with_capacity(state.threads.len() + state.planning_snapshot.scratch.len());

    for thread in &state.threads {
        let anchor = SourceRef::codex_thread(&thread.id);
        let projection = reconcile_thread_card_with_goal_and_forge(
            ReconcileInput {
                thread,
                git: state.git_context(&thread.id),
                local: local_by_anchor.get(&anchor).copied(),
                collision_count: state.worktree_collision_count(&thread.id),
                backend_observed_at_unix_ms: state.backend_status.last_refresh_unix_ms,
                backend_error: state.backend_status.error.as_deref(),
                now_unix_ms,
            },
            state.goals.get(&thread.id.0),
            state.forge_observation(&thread.id),
        );
        projections.push(projection);
    }

    projections.extend(state.planning_snapshot.scratch.iter().map(|scratch| {
        let anchor = SourceRef {
            kind: SourceKind::ScratchWork,
            value: scratch.id.clone(),
        };
        reconcile_scratch_card_with_local(
            scratch,
            local_by_anchor.get(&anchor).copied(),
            now_unix_ms,
        )
    }));

    for projection in &mut projections {
        if let Some(note) = state
            .planning_snapshot
            .notes
            .iter()
            .find(|note| note.owner == projection.anchor)
        {
            projection.overlay.note = Some(note.text.clone());
        }
    }

    projections.sort_by(|left, right| {
        right
            .overlay
            .pinned
            .cmp(&left.overlay.pinned)
            .then_with(|| left.stage.cmp(&right.stage))
            .then_with(|| {
                left.overlay
                    .priority
                    .unwrap_or(i32::MAX)
                    .cmp(&right.overlay.priority.unwrap_or(i32::MAX))
            })
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.local_id.cmp(&right.local_id))
    });

    state.work_cards = projections;
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
    fn goal_actions_are_explicit_app_server_effects() {
        let mut app = app();
        reduce(&mut app, Action::OpenSelected);
        let refresh = reduce(&mut app, Action::OpenGoalActions);
        assert!(matches!(refresh.as_slice(), [Effect::RefreshGoal(_)]));

        let thread_id = app.current_thread_id().expect("thread").clone();
        reduce(
            &mut app,
            Action::GoalObserved(GoalObservation {
                thread_id: thread_id.clone(),
                objective: "Ship M4".into(),
                status: GoalStatus::Active,
                token_budget: Some(10_000),
                tokens_used: 100,
                time_used_seconds: 30,
                created_at: 1,
                updated_at: 2,
                observed_at_unix_ms: 10,
            }),
        );
        let pause = reduce(&mut app, Action::SetGoalStatus(GoalStatus::Paused));
        assert_eq!(
            pause,
            vec![Effect::SetGoal {
                thread_id: thread_id.clone(),
                objective: None,
                status: Some(GoalStatus::Paused),
            }]
        );

        let clear = reduce(&mut app, Action::ClearGoal);
        assert_eq!(clear, vec![Effect::ClearGoal(thread_id)]);
    }

    #[test]
    fn goal_observation_reconciles_without_entering_local_store_state() {
        let mut app = app();
        let thread_id = app.threads[0].id.clone();
        reduce(
            &mut app,
            Action::GoalObserved(GoalObservation {
                thread_id: thread_id.clone(),
                objective: "Ship M4".into(),
                status: GoalStatus::Blocked,
                token_budget: None,
                tokens_used: 0,
                time_used_seconds: 0,
                created_at: 1,
                updated_at: 1,
                observed_at_unix_ms: 100,
            }),
        );
        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 100 });
        let card = app.work_card_for_thread(&thread_id).expect("card");
        assert_eq!(card.stage, WorkflowStage::Working);
        assert!(
            card.attention
                .contains(&crate::planning::PlanningAttention::GoalBlocked)
        );

        let local = app.to_local_state();
        assert_eq!(local.schema_version, 1);
        assert!(
            !serde_json::to_string(&local)
                .expect("serialize")
                .contains("Ship M4")
        );
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
    fn managed_worktree_plan_requires_explicit_confirmation_before_execution() {
        let mut app = app();
        app.threads[0].metadata.cwd = "/repo".into();
        let repo = crate::domain::LocalRepoIdentity {
            git_common_dir: "/repo/.git".into(),
            primary_root: "/repo".into(),
        };
        app.git_contexts.insert(
            "thread-impl".into(),
            GitContext {
                thread_id: ThreadId::new("thread-impl"),
                cwd: "/repo".into(),
                is_repository: true,
                repo: Some(repo.clone()),
                worktree: Some(crate::domain::WorktreeIdentity {
                    repo,
                    canonical_path: "/repo".into(),
                    branch: Some("main".into()),
                    managed_by_codex_tui: false,
                }),
                head: Some("deadbeef".into()),
                branch: Some("main".into()),
                upstream: None,
                ahead: 0,
                behind: 0,
                dirty: false,
                changes: vec![],
                observed_at_unix_ms: 1,
                error: None,
            },
        );
        reduce(&mut app, Action::OpenSelected);
        let effects = reduce(&mut app, Action::OpenManagedWorktrees);
        assert_eq!(effects, vec![Effect::RefreshManagedWorktrees]);

        reduce(&mut app, Action::BeginCreateWorktree);
        for ch in "feature".chars() {
            reduce(&mut app, Action::InputChar(ch));
        }
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp
            .path()
            .join("feature-wt")
            .to_string_lossy()
            .into_owned();
        for ch in target.chars() {
            reduce(&mut app, Action::InputChar(ch));
        }
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        reduce(&mut app, Action::InputBackspace);
        reduce(&mut app, Action::InputBackspace);
        reduce(&mut app, Action::InputBackspace);
        reduce(&mut app, Action::InputBackspace);
        for ch in "HEAD".chars() {
            reduce(&mut app, Action::InputChar(ch));
        }
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        assert!(app.pending_operation.is_some());

        let effects = reduce(&mut app, Action::ConfirmPendingOperation);
        assert!(matches!(effects.as_slice(), [Effect::ExecuteOperation(_)]));
        assert!(app.pending_operation.is_none());
    }

    #[test]
    fn worktree_remove_and_branch_delete_are_distinct_plans() {
        let mut app = app();
        app.view = View::ManagedWorktrees(ThreadId::new("thread-impl"));
        let repo = crate::domain::LocalRepoIdentity {
            git_common_dir: "/repo/.git".into(),
            primary_root: "/repo".into(),
        };
        app.managed_worktrees.push(ManagedWorktreeRecord {
            repo: repo.clone(),
            canonical_path: "/repo-feature".into(),
            branch: Some("feature".into()),
            created_by_operation_id: "op-create".into(),
            adopted: false,
            created_at_unix_ms: 1,
            last_verified_at_unix_ms: 1,
        });
        reduce(&mut app, Action::BeginRemoveManagedWorktree);
        let remove = app.pending_operation.clone().expect("remove");
        assert_eq!(remove.kind, crate::operation::OperationKind::RemoveWorktree);
        assert!(remove.target_branch.is_none());

        app.git_contexts.insert(
            "thread-impl".into(),
            GitContext::pending(ThreadId::new("thread-impl"), "/repo"),
        );
        if let Some(context) = app.git_contexts.get_mut("thread-impl") {
            context.repo = Some(repo);
            context.branch = Some("feature".into());
        }
        reduce(&mut app, Action::BeginDeleteBranch);
        assert_eq!(app.input_mode, InputMode::WorktreeDeleteBranch);
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
    fn workspace_is_read_only_and_returns_to_originating_view() {
        let mut app = app();
        app.threads[0].metadata.cwd = "/repo".into();
        let effects = reduce(&mut app, Action::OpenWorkspace);
        assert!(matches!(app.view, View::Workspace(_)));
        assert_eq!(
            effects,
            vec![Effect::ProbeGit {
                thread_id: ThreadId::new("thread-impl"),
                cwd: "/repo".into(),
            }]
        );
        reduce(&mut app, Action::Back);
        assert_eq!(app.view, View::Registry);
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
    fn board_keeps_workflow_columns_separate_from_needs_you_filter() {
        let mut app = app();
        app.threads[0].runtime = RuntimeStatus::Working;
        app.threads[0].attention = vec![AttentionReason::ApprovalRequired];
        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 10 });
        reduce(&mut app, Action::OpenBoard);
        assert_eq!(app.active_saved_view().layout, SavedViewLayout::Board);
        app.board_stage_index = WorkflowStage::ALL
            .iter()
            .position(|stage| *stage == WorkflowStage::Working)
            .expect("working");
        assert!(
            app.visible_planning_cards()
                .iter()
                .any(|card| { card.stage == WorkflowStage::Working && card.needs_you() })
        );
        reduce(&mut app, Action::CycleSavedView(1));
        assert_eq!(app.active_saved_view().id, "builtin:attention");
        assert!(
            app.visible_planning_cards()
                .iter()
                .all(|card| card.needs_you())
        );
    }

    #[test]
    fn new_scratch_is_an_explicit_local_effect() {
        let mut app = app();
        reduce(&mut app, Action::BeginScratch);
        for ch in "Investigate wake miss".chars() {
            reduce(&mut app, Action::InputChar(ch));
        }
        let effects = reduce(&mut app, Action::CommitInput);
        assert!(matches!(
            effects.as_slice(),
            [Effect::CreateScratch { title, .. }] if title == "Investigate wake miss"
        ));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn board_context_can_save_but_not_delete_builtin_views() {
        let mut app = app();
        reduce(&mut app, Action::OpenBoard);
        let choices = app.context_choices();
        assert!(choices.contains(&ContextChoice::SaveCurrentView));
        assert!(!choices.contains(&ContextChoice::DeleteCurrentView));

        reduce(&mut app, Action::OpenContext);
        let save_index = app
            .context_choices()
            .iter()
            .position(|choice| *choice == ContextChoice::SaveCurrentView)
            .expect("save view action");
        app.context_selected = save_index;
        reduce(&mut app, Action::ExecuteContext);
        assert_eq!(app.input_mode, InputMode::SavedViewName);
        assert!(app.saved_view_template.is_some());
    }

    #[test]
    fn context_menu_exposes_workflow_changes_only_for_scratch() {
        let mut app = app();
        let thread_choices = app.context_choices();
        assert_eq!(thread_choices.len(), 3);
        assert!(!thread_choices.contains(&ContextChoice::ScratchDone));

        app.planning_snapshot
            .scratch
            .push(crate::planning::ScratchWork {
                id: "scratch:1".into(),
                title: "Local".into(),
                note: None,
                workspace: None,
                priority: None,
                state: crate::planning::ScratchState::Inbox,
                created_at_unix_ms: 1,
                updated_at_unix_ms: 1,
            });
        app.work_cards.push(reconcile_scratch_card_with_local(
            &app.planning_snapshot.scratch[0],
            None,
            1,
        ));
        app.view = View::Scratch("scratch:1".into());
        let scratch_choices = app.context_choices();
        assert!(scratch_choices.contains(&ContextChoice::ScratchReady));
        assert!(scratch_choices.contains(&ContextChoice::DeleteScratch));
    }

    #[test]
    fn context_note_prefills_existing_thread_note() {
        let mut app = app();
        let owner = SourceRef::codex_thread(&ThreadId::new("thread-impl"));
        app.planning_snapshot
            .notes
            .push(crate::planning::LocalNote {
                owner,
                text: "remember".into(),
                updated_at_unix_ms: 1,
            });
        reduce(&mut app, Action::OpenContext);
        reduce(&mut app, Action::MoveContext(1));
        reduce(&mut app, Action::ExecuteContext);
        assert_eq!(app.input_mode, InputMode::Note);
        assert_eq!(app.input_buffer, "remember");
    }

    #[test]
    fn snooze_duration_parser_is_bounded_and_unit_explicit() {
        assert_eq!(parse_snooze_duration("15m"), Some(900_000));
        assert_eq!(parse_snooze_duration("1h"), Some(3_600_000));
        assert_eq!(parse_snooze_duration("2d"), Some(172_800_000));
        assert_eq!(parse_snooze_duration("0h"), None);
        assert_eq!(parse_snooze_duration("later"), None);
    }

    #[test]
    fn back_cancels_hot_slot_binding_before_navigation() {
        let mut app = app();
        reduce(&mut app, Action::BeginHotSlotBind);
        assert!(app.hot_slot_bind_pending);
        let effects = reduce(&mut app, Action::Back);
        assert!(effects.is_empty());
        assert!(!app.hot_slot_bind_pending);
        assert_eq!(app.view, View::Registry);
    }

    #[test]
    fn hot_slot_binding_stores_only_source_reference() {
        let mut app = app();
        reduce(&mut app, Action::BeginHotSlotBind);
        let effects = reduce(&mut app, Action::UseHotSlot(3));
        assert_eq!(
            effects,
            vec![Effect::SetHotSlot {
                slot: 3,
                target: SourceRef::codex_thread(&ThreadId::new("thread-impl")),
            }]
        );
        assert!(!app.hot_slot_bind_pending);
    }

    #[test]
    fn board_attention_rotation_skips_snoozed_cards() {
        let mut app = app();
        app.threads[0].runtime = RuntimeStatus::Working;
        app.threads[0].attention = vec![AttentionReason::ApprovalRequired];
        app.threads[1].runtime = RuntimeStatus::Working;
        app.threads[1].attention = vec![AttentionReason::UserInputRequired];

        let mut local = crate::planning::WorkCardRecord::implicit_thread(&app.threads[0].id);
        local.overlay.snooze_until_unix_ms = Some(10_000);
        app.planning_snapshot.cards.push(local);
        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1 });
        reduce(&mut app, Action::OpenBoard);
        reduce(&mut app, Action::NextAttention);
        let selected = app
            .selected_planning_card()
            .expect("selected attention card");
        assert_ne!(selected.anchor, SourceRef::codex_thread(&app.threads[0].id));
        assert!(!selected.snoozed);
        assert!(selected.needs_you());
    }

    #[test]
    fn planning_reconciliation_keeps_workflow_and_attention_orthogonal() {
        let mut app = app();
        app.threads[0].runtime = RuntimeStatus::Working;
        app.threads[0].attention = vec![AttentionReason::ApprovalRequired];
        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 100 });
        let card = app
            .work_card_for_thread(&ThreadId::new("thread-impl"))
            .expect("card");
        assert_eq!(card.stage, crate::planning::WorkflowStage::Working);
        assert!(card.needs_you());
        assert!(
            card.attention
                .contains(&crate::planning::PlanningAttention::ApprovalRequired)
        );
    }

    #[test]
    fn scratch_items_join_the_same_planning_projection_without_becoming_threads() {
        let mut app = app();
        app.planning_snapshot
            .scratch
            .push(crate::planning::ScratchWork {
                id: "scratch:1".into(),
                title: "Investigate".into(),
                note: None,
                workspace: Some("kws".into()),
                priority: Some(1),
                state: crate::planning::ScratchState::Inbox,
                created_at_unix_ms: 1,
                updated_at_unix_ms: 1,
            });
        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 2 });
        assert!(app.work_cards.iter().any(|card| {
            card.anchor.kind == crate::planning::SourceKind::ScratchWork
                && card.title == "Investigate"
        }));
        assert_eq!(app.threads.len(), 4);
    }

    #[test]
    fn git_projection_probe_is_emitted_once_until_cwd_changes() {
        let mut app = app();
        for (index, thread) in app.threads.iter_mut().enumerate() {
            thread.metadata.cwd = format!("/repo-{index}");
        }
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
