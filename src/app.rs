use crate::backend::BackendStatus;
use crate::batch_local::{LocalBatchAction, LocalBatchPlan, parse_priority};
use crate::command::Command;
#[cfg(test)]
use crate::conversation::ConversationPage;
use crate::conversation::{
    ConversationState, InteractiveRequest, InteractiveRequestKind, InteractiveResolution,
    RpcRequestId, UserInputQuestion,
};
use crate::domain::{
    AttentionReason, CwdLocality, LocalRepoIdentity, RuntimeStatus, ThreadId, ThreadSummary,
    ThreadUiState, classify_cwd, classify_cwd_without_io,
};
use crate::forge::{
    CapabilityState, ChangeRequestSummary, ForgeCapability, ForgeIdentity, ForgeObservation,
    ForgeReviewTarget,
};
use crate::forge_mutation::{ForgeMutationPlan, ForgeMutationReceipt, ForgeMutationRequest};
use crate::git::{GitContext, GitReview};
use crate::goal::{GoalObservation, GoalStatus};
use crate::i18n::{UiLanguage, pick};
use crate::launch::{LaunchPlan, LaunchPreset};
use crate::metadata_search::{
    MetadataSearchContext, filter_requires_locality, matches_metadata_query,
};
use crate::operation::{
    ManagedWorktreeRecord, MutationScope, OperationPlan, OperationReceipt, OperationState,
    mutation_scope_for_thread, now_unix_ms,
};
use crate::planning::{
    PlanningSnapshot, ReconcileInput, SavedView, SavedViewLayout, SourceKind, SourceRef,
    WorkCardProjection, WorkflowStage, apply_saved_view, builtin_saved_views,
    forge_issue_source_ref, reconcile_forge_issue_card, reconcile_scratch_card_with_local,
    reconcile_thread_card_with_goal_and_forge,
};
use crate::saved_view_editor::SavedViewEditor;
use crate::store::LocalStateV1;
use crate::terminal_drawer::TerminalSnapshot;
use crate::text::sanitize_inline;
use crate::thread_queue::{QueuedSubmission, ThreadQueueMutation, ThreadQueueSnapshot};
use crate::transcript_search::{
    TranscriptSearchHit, TranscriptSearchResults, TranscriptSearchSource,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::time::Instant;

mod lifecycle;
pub(crate) mod palette;
mod planning_selection;
pub mod planning_worker;
mod prompt;
mod review;
mod types;

pub use types::{Action, ContextChoice, Effect, InputMode, View, ViewKind};

const REGISTRY_RECENT_LIMIT: usize = 100;
const CONVERSATION_CACHE_LIMIT: usize = 16;
const GIT_REVIEW_CACHE_LIMIT: usize = 4;

pub const fn conversation_cache_limit() -> usize {
    CONVERSATION_CACHE_LIMIT
}

pub const fn git_review_cache_limit() -> usize {
    GIT_REVIEW_CACHE_LIMIT
}

fn local_text(
    language: UiLanguage,
    english: &'static str,
    simplified_chinese: &'static str,
) -> &'static str {
    pick(language, english, simplified_chinese)
}

fn operation_state_text(state: OperationState, language: UiLanguage) -> &'static str {
    match (state, language) {
        (OperationState::Planned, UiLanguage::SimplifiedChinese) => "已计划",
        (OperationState::Executing, UiLanguage::SimplifiedChinese) => "执行中",
        (OperationState::Succeeded, UiLanguage::SimplifiedChinese) => "已成功",
        (OperationState::Failed, UiLanguage::SimplifiedChinese) => "失败",
        (OperationState::OutcomeUnknown, UiLanguage::SimplifiedChinese) => "结果未知",
        (OperationState::Planned, UiLanguage::English) => "planned",
        (OperationState::Executing, UiLanguage::English) => "executing",
        (OperationState::Succeeded, UiLanguage::English) => "succeeded",
        (OperationState::Failed, UiLanguage::English) => "failed",
        (OperationState::OutcomeUnknown, UiLanguage::English) => "outcome unknown",
    }
}

fn build_thread_indexes(
    threads: &[ThreadSummary],
) -> (HashMap<String, usize>, BTreeMap<String, Vec<usize>>) {
    let mut by_id = HashMap::with_capacity(threads.len());
    let mut by_cwd = BTreeMap::<String, Vec<usize>>::new();
    for (index, thread) in threads.iter().enumerate() {
        by_id.insert(thread.id.0.clone(), index);
        by_cwd
            .entry(thread.metadata.cwd.clone())
            .or_default()
            .push(index);
    }
    (by_id, by_cwd)
}

#[derive(Clone, Debug)]
pub struct AppState {
    planning_generation: u64,
    planning_reconcile_count: usize,
    pub threads: Vec<ThreadSummary>,
    thread_index_by_id: HashMap<String, usize>,
    thread_indices_by_cwd: BTreeMap<String, Vec<usize>>,
    pub selected: usize,
    pub view: View,
    pub previous_target: Option<ThreadId>,
    pub thread_ui: BTreeMap<String, ThreadUiState>,
    prompt_submissions: prompt::PendingSubmissions,
    local_pins: BTreeSet<String>,
    local_aliases: BTreeMap<String, String>,
    local_marked_unread: BTreeSet<String>,
    pub conversations: BTreeMap<String, ConversationState>,
    conversation_cache_order: VecDeque<String>,
    pub git_contexts: BTreeMap<String, GitContext>,
    cwd_localities: BTreeMap<String, CwdLocality>,
    pub forge_observations: BTreeMap<String, ForgeObservation>,
    pub git_reviews: BTreeMap<String, GitReview>,
    git_review_cache_order: VecDeque<String>,
    pub planning_snapshot: PlanningSnapshot,
    pub work_cards: Vec<WorkCardProjection>,
    work_card_by_thread: BTreeMap<String, usize>,
    worktree_collision_counts: BTreeMap<String, usize>,
    pub goals: BTreeMap<String, GoalObservation>,
    pub goal_checked: BTreeSet<String>,
    pub goal_actions_open: bool,
    pub thread_queue_open: bool,
    pub thread_queue_snapshot: Option<ThreadQueueSnapshot>,
    pub thread_queue_selected: usize,
    pub thread_queue_loading: bool,
    pub thread_queue_error: Option<String>,
    pub pending_thread_queue_mutation: Option<ThreadQueueMutation>,
    pub managed_worktrees: Vec<ManagedWorktreeRecord>,
    pub managed_selected: usize,
    pub managed_return_view: Option<View>,
    pub pending_operation: Option<OperationPlan>,
    pub recent_operations: Vec<OperationReceipt>,
    pub pending_forge_operation: Option<ForgeMutationPlan>,
    pub pending_forge_payload: Option<String>,
    pub pending_local_batch: Option<LocalBatchPlan>,
    pub launch_menu_open: bool,
    pub launch_selected: usize,
    pub launch_repo_root: Option<String>,
    pub launch_thread_cwd: Option<String>,
    pub launch_presets: Vec<LaunchPreset>,
    pub pending_launch_plan: Option<LaunchPlan>,
    pub terminal_drawer_open: bool,
    pub terminal_focused: bool,
    pub terminal_snapshot: Option<TerminalSnapshot>,
    pub recent_forge_operations: Vec<ForgeMutationReceipt>,
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
    pub saved_view_editor: Option<SavedViewEditor>,
    pub saved_view_editor_error: Option<String>,
    pub command_palette_open: bool,
    pub command_palette_selected: usize,
    pub command_palette_query: String,
    command_palette_items: Vec<Command>,
    pub context_open: bool,
    pub context_selected: usize,
    pub hot_slot_bind_pending: bool,
    pub review_selected: usize,
    pub review_scroll: u16,
    pub review_word_diff: bool,
    pub pending_requests: Vec<InteractiveRequest>,
    pending_request_threads: BTreeSet<String>,
    pub user_input_request_id: Option<RpcRequestId>,
    pub user_input_question_index: usize,
    pub user_input_answers: BTreeMap<String, Vec<String>>,
    pub show_help: bool,
    pub should_quit: bool,
    pub backend_status: BackendStatus,
    pub language: UiLanguage,
    pub acknowledged_attention: BTreeSet<String>,
    pub filter: String,
    pub host_local_only: bool,
    pub repo_backed_only: bool,
    pub show_all_history: bool,
    pub transcript_search_open: bool,
    pub transcript_search_query: String,
    pub transcript_search_results: Option<TranscriptSearchResults>,
    pub transcript_search_selected: usize,
    pub transcript_search_loading: bool,
    pub transcript_search_error: Option<String>,
    pub transcript_search_active_hit: Option<TranscriptSearchHit>,
    pub input_mode: InputMode,
    pub input_buffer: String,
    input_original: String,
    search_return_view: Option<View>,
}

#[derive(Clone, Debug)]
struct ForgeMutationTarget {
    cwd: String,
    branch: String,
    identity: ForgeIdentity,
    change_request: Option<ChangeRequestSummary>,
}

impl AppState {
    pub fn new(threads: Vec<ThreadSummary>) -> Self {
        let (thread_index_by_id, thread_indices_by_cwd) = build_thread_indexes(&threads);
        let local_pins = threads
            .iter()
            .filter(|thread| thread.pinned)
            .map(|thread| thread.id.0.clone())
            .collect();
        let local_aliases = threads
            .iter()
            .filter_map(|thread| {
                thread
                    .alias
                    .as_ref()
                    .map(|alias| (thread.id.0.clone(), alias.clone()))
            })
            .collect();
        let local_marked_unread = threads
            .iter()
            .filter(|thread| thread.attention.contains(&AttentionReason::MarkedUnread))
            .map(|thread| thread.id.0.clone())
            .collect();
        Self {
            planning_generation: 0,
            planning_reconcile_count: 0,
            threads,
            thread_index_by_id,
            thread_indices_by_cwd,
            selected: 0,
            view: View::Registry,
            previous_target: None,
            thread_ui: BTreeMap::new(),
            prompt_submissions: Default::default(),
            local_pins,
            local_aliases,
            local_marked_unread,
            conversations: BTreeMap::new(),
            conversation_cache_order: VecDeque::new(),
            git_contexts: BTreeMap::new(),
            cwd_localities: BTreeMap::new(),
            forge_observations: BTreeMap::new(),
            git_reviews: BTreeMap::new(),
            git_review_cache_order: VecDeque::new(),
            planning_snapshot: PlanningSnapshot::default(),
            work_cards: vec![],
            work_card_by_thread: BTreeMap::new(),
            worktree_collision_counts: BTreeMap::new(),
            goals: BTreeMap::new(),
            goal_checked: BTreeSet::new(),
            goal_actions_open: false,
            thread_queue_open: false,
            thread_queue_snapshot: None,
            thread_queue_selected: 0,
            thread_queue_loading: false,
            thread_queue_error: None,
            pending_thread_queue_mutation: None,
            managed_worktrees: vec![],
            managed_selected: 0,
            managed_return_view: None,
            pending_operation: None,
            recent_operations: vec![],
            pending_forge_operation: None,
            pending_forge_payload: None,
            pending_local_batch: None,
            launch_menu_open: false,
            launch_selected: 0,
            launch_repo_root: None,
            launch_thread_cwd: None,
            launch_presets: vec![],
            pending_launch_plan: None,
            terminal_drawer_open: false,
            terminal_focused: false,
            terminal_snapshot: None,
            recent_forge_operations: vec![],
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
            saved_view_editor: None,
            saved_view_editor_error: None,
            command_palette_open: false,
            command_palette_selected: 0,
            command_palette_query: String::new(),
            command_palette_items: vec![],
            context_open: false,
            context_selected: 0,
            hot_slot_bind_pending: false,
            review_selected: 0,
            review_scroll: 0,
            review_word_diff: false,
            pending_requests: vec![],
            pending_request_threads: BTreeSet::new(),
            user_input_request_id: None,
            user_input_question_index: 0,
            user_input_answers: BTreeMap::new(),
            show_help: false,
            should_quit: false,
            backend_status: BackendStatus::starting("unknown"),
            language: UiLanguage::English,
            acknowledged_attention: BTreeSet::new(),
            filter: String::new(),
            host_local_only: false,
            repo_backed_only: false,
            show_all_history: false,
            transcript_search_open: false,
            transcript_search_query: String::new(),
            transcript_search_results: None,
            transcript_search_selected: 0,
            transcript_search_loading: false,
            transcript_search_error: None,
            transcript_search_active_hit: None,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            input_original: String::new(),
            search_return_view: None,
        }
    }

    pub fn transcript_search_hit(&self) -> Option<&TranscriptSearchHit> {
        self.transcript_search_results
            .as_ref()
            .and_then(|results| results.hits.get(self.transcript_search_selected))
    }

    pub fn transcript_search_source_label(&self) -> &'static str {
        self.transcript_search_results
            .as_ref()
            .map(|results| results.source.label())
            .unwrap_or("pending")
    }

    pub fn selected_thread_queue_submission(&self) -> Option<&QueuedSubmission> {
        self.thread_queue_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.submissions.get(self.thread_queue_selected))
    }

    fn prepare_conversation(&mut self, thread_id: &ThreadId) {
        self.conversations
            .entry(thread_id.0.clone())
            .or_insert_with(|| ConversationState::loading(thread_id.clone()))
            .loading = true;
        self.touch_conversation_cache(thread_id);
    }

    fn touch_conversation_cache(&mut self, thread_id: &ThreadId) {
        self.conversation_cache_order
            .retain(|candidate| candidate != &thread_id.0);
        self.conversation_cache_order.push_back(thread_id.0.clone());

        while self.conversation_cache_order.len() > CONVERSATION_CACHE_LIMIT {
            let Some(evicted) = self.conversation_cache_order.pop_front() else {
                break;
            };
            if self
                .current_thread_id()
                .is_some_and(|current| current.0 == evicted)
            {
                self.conversation_cache_order.push_back(evicted);
                continue;
            }
            self.conversations.remove(&evicted);
        }
    }

    fn prepare_git_review(&mut self, thread_id: &ThreadId, cwd: String) {
        self.git_reviews.insert(
            thread_id.0.clone(),
            GitReview::pending(thread_id.clone(), cwd),
        );
        self.touch_git_review_cache(thread_id);
    }

    fn touch_git_review_cache(&mut self, thread_id: &ThreadId) {
        self.git_review_cache_order
            .retain(|candidate| candidate != &thread_id.0);
        self.git_review_cache_order.push_back(thread_id.0.clone());

        while self.git_review_cache_order.len() > GIT_REVIEW_CACHE_LIMIT {
            let Some(evicted) = self.git_review_cache_order.pop_front() else {
                break;
            };
            if matches!(&self.view, View::Review(current) if current.0 == evicted) {
                self.git_review_cache_order.push_back(evicted);
                continue;
            }
            self.git_reviews.remove(&evicted);
        }
    }

    fn rebuild_thread_indexes(&mut self) {
        let (thread_index_by_id, thread_indices_by_cwd) = build_thread_indexes(&self.threads);
        self.thread_index_by_id = thread_index_by_id;
        self.thread_indices_by_cwd = thread_indices_by_cwd;
    }

    fn thread_by_id(&self, thread_id: &ThreadId) -> Option<&ThreadSummary> {
        self.thread_index_by_id
            .get(&thread_id.0)
            .and_then(|index| self.threads.get(*index))
            .filter(|thread| thread.id == *thread_id)
            .or_else(|| self.threads.iter().find(|thread| thread.id == *thread_id))
    }

    fn search_return_view_is_valid(&self, view: &View) -> bool {
        match view {
            View::Thread(thread_id)
            | View::Review(thread_id)
            | View::Workspace(thread_id)
            | View::ManagedWorktrees(thread_id) => self.thread_by_id(thread_id).is_some(),
            View::Registry | View::Board | View::Scratch(_) => true,
        }
    }

    fn thread_id_for_view(view: &View) -> Option<ThreadId> {
        match view {
            View::Thread(thread_id)
            | View::Review(thread_id)
            | View::Workspace(thread_id)
            | View::ManagedWorktrees(thread_id) => Some(thread_id.clone()),
            View::Registry | View::Board | View::Scratch(_) => None,
        }
    }

    fn search_origin_thread_id(&self) -> Option<ThreadId> {
        let return_view = self.search_return_view.as_ref()?;
        match return_view {
            View::Board | View::Scratch(_) => self
                .board_return_view
                .as_ref()
                .and_then(Self::thread_id_for_view),
            _ => Self::thread_id_for_view(return_view),
        }
    }

    fn thread_ids_for_cwd(&self, cwd: &str) -> Vec<ThreadId> {
        if let Some(indices) = self.thread_indices_by_cwd.get(cwd) {
            let indexed = indices
                .iter()
                .filter_map(|index| self.threads.get(*index))
                .filter(|thread| thread.metadata.cwd == cwd)
                .map(|thread| thread.id.clone())
                .collect::<Vec<_>>();
            if !indexed.is_empty() {
                return indexed;
            }
        }

        self.threads
            .iter()
            .filter(|thread| thread.metadata.cwd == cwd)
            .map(|thread| thread.id.clone())
            .collect()
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

    pub fn cwd_locality(&self, cwd: &str) -> CwdLocality {
        self.cwd_localities
            .get(cwd)
            .copied()
            .unwrap_or_else(|| classify_cwd(cwd))
    }

    pub fn cwd_locality_for_display(&self, cwd: &str) -> Option<CwdLocality> {
        self.cwd_localities
            .get(cwd)
            .copied()
            .or_else(|| classify_cwd_without_io(cwd))
    }

    fn refresh_cwd_locality(&mut self, cwd: &str) -> CwdLocality {
        let locality = classify_cwd(cwd);
        self.cwd_localities.insert(cwd.to_string(), locality);
        locality
    }

    fn reconcile_cwd_locality_cache(&mut self, fill_missing: bool) {
        let current_cwds = self
            .threads
            .iter()
            .map(|thread| thread.metadata.cwd.clone())
            .collect::<BTreeSet<_>>();
        self.cwd_localities
            .retain(|cwd, _| current_cwds.contains(cwd));

        if fill_missing {
            for cwd in current_cwds {
                self.cwd_localities
                    .entry(cwd.clone())
                    .or_insert_with(|| classify_cwd(&cwd));
            }
        }
    }

    pub fn selected_thread(&self) -> Option<&ThreadSummary> {
        let thread = self.threads.get(self.selected)?;
        self.thread_visible_in_registry(thread).then_some(thread)
    }

    fn thread_visible_in_registry(&self, thread: &ThreadSummary) -> bool {
        let query = self.filter.trim().to_lowercase();
        self.thread_visible_in_registry_with_query(thread, &query)
    }

    fn thread_visible_in_registry_with_query(
        &self,
        thread: &ThreadSummary,
        normalized_query: &str,
    ) -> bool {
        let locality = filter_requires_locality(normalized_query)
            .then(|| self.cwd_locality(&thread.metadata.cwd));
        matches_metadata_query(
            MetadataSearchContext {
                thread,
                locality,
                goal: self.goals.get(&thread.id.0),
                forge: self.forge_observation(&thread.id),
                card: self.work_card_for_thread(&thread.id),
            },
            normalized_query,
        ) && (!self.host_local_only
            || self.cwd_locality(&thread.metadata.cwd) == CwdLocality::LocalDirectory)
            && self.thread_matches_repo_scope(thread)
    }

    fn thread_matches_repo_scope(&self, thread: &ThreadSummary) -> bool {
        if !self.repo_backed_only {
            return true;
        }
        if self.cwd_locality(&thread.metadata.cwd) != CwdLocality::LocalDirectory {
            return false;
        }
        match self.git_context(&thread.id) {
            None => true,
            Some(context) if context.cwd != thread.metadata.cwd => true,
            Some(context) if context.observed_at_unix_ms == 0 => true,
            Some(context) if context.error.is_some() => true,
            Some(context) => context.is_repository,
        }
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

    fn freeze_visible_batch(&mut self, action: LocalBatchAction) {
        let plan = {
            let cards = self.visible_planning_cards();
            LocalBatchPlan::freeze(&cards, action, now_unix_ms())
        };
        match plan {
            Ok(plan) => {
                self.pending_operation = None;
                self.pending_forge_operation = None;
                self.pending_forge_payload = None;
                self.pending_local_batch = Some(plan);
                self.mutation_notice = None;
            }
            Err(error) => {
                self.pending_local_batch = None;
                self.mutation_notice = Some(format!(
                    "{}: {error:#}",
                    local_text(
                        self.language,
                        "cannot create local batch plan",
                        "无法创建本地批量计划",
                    )
                ));
            }
        }
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

    pub fn terminal_target_cwd(&self) -> Result<String, String> {
        let thread_id = match &self.view {
            View::Registry => self.selected_thread_id(),
            View::Thread(id)
            | View::Review(id)
            | View::Workspace(id)
            | View::ManagedWorktrees(id) => Some(id.clone()),
            View::Board => self.selected_planning_card().and_then(|card| {
                (card.anchor.kind == SourceKind::CodexThread)
                    .then(|| ThreadId::new(card.anchor.value.clone()))
            }),
            View::Scratch(_) => None,
        }
        .ok_or_else(|| {
            local_text(
                self.language,
                "terminal drawer unavailable: no Codex thread is selected",
                "终端抽屉不可用：未选择 Codex 会话",
            )
            .to_string()
        })?;

        let thread = self
            .threads
            .iter()
            .find(|thread| thread.id == thread_id)
            .ok_or_else(|| {
                local_text(
                    self.language,
                    "terminal drawer unavailable: selected Codex thread disappeared",
                    "终端抽屉不可用：所选 Codex 会话已消失",
                )
                .to_string()
            })?;
        let cwd = thread.metadata.cwd.trim();
        match classify_cwd(cwd) {
            CwdLocality::LocalDirectory => Ok(cwd.to_string()),
            CwdLocality::ForeignWindows => Err(
                local_text(
                    self.language,
                    "terminal drawer unavailable: selected session has a Windows cwd and is not local to this Linux host; search local in Mission Control",
                    "终端抽屉不可用：所选会话使用 Windows cwd，不属于当前 Linux 主机；请在 Mission Control 中筛选本机会话",
                )
                .into(),
            ),
            CwdLocality::ForeignUnix => Err(
                local_text(
                    self.language,
                    "terminal drawer unavailable: selected session has a Unix cwd and is not local to this Windows host; search local in Mission Control",
                    "终端抽屉不可用：所选会话使用 Unix cwd，不属于当前 Windows 主机；请在 Mission Control 中筛选本机会话",
                )
                .into(),
            ),
            CwdLocality::NativeMissing => Err(format!(
                "{}: {cwd}",
                local_text(
                    self.language,
                    "terminal drawer unavailable: selected session cwd does not exist on this host",
                    "终端抽屉不可用：所选会话 cwd 在本机不存在",
                )
            )),
            CwdLocality::Relative => Err(format!(
                "{}: {cwd}",
                local_text(
                    self.language,
                    "terminal drawer unavailable: selected session cwd is not absolute",
                    "终端抽屉不可用：所选会话 cwd 不是绝对路径",
                )
            )),
            CwdLocality::Empty => Err(
                local_text(
                    self.language,
                    "terminal drawer unavailable: selected Codex thread has no cwd; search local in Mission Control",
                    "终端抽屉不可用：所选 Codex 会话没有 cwd；请在 Mission Control 中筛选本机会话",
                )
                .into(),
            ),
        }
    }

    fn current_forge_mutation_target(&self) -> Option<ForgeMutationTarget> {
        let thread_id = match &self.view {
            View::Review(id) | View::Workspace(id) => id,
            _ => return None,
        };
        let context = self.git_context(thread_id)?;
        let branch = context.branch.clone()?;
        let observation = self.forge_observation(thread_id)?;
        let identity = observation.identity.clone()?;
        let change_request = observation.change_request_for_branch(&branch).cloned();
        Some(ForgeMutationTarget {
            cwd: context.cwd.clone(),
            branch,
            identity,
            change_request,
        })
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
        if self.lifecycle_capability_available("thread/start")
            && self
                .lifecycle_cwd()
                .is_some_and(|cwd| classify_cwd(&cwd) == CwdLocality::LocalDirectory)
        {
            choices.push(ContextChoice::NewCodexThread);
        }
        if self.lifecycle_capability_available("thread/fork")
            && self.lifecycle_thread_id().is_some()
        {
            choices.push(ContextChoice::ForkCodexThread);
        }
        if matches!(self.view, View::Board) {
            if !self.visible_planning_cards().is_empty() {
                choices.extend([
                    ContextChoice::BatchAddTag,
                    ContextChoice::BatchRemoveTag,
                    ContextChoice::BatchSetPriority,
                    ContextChoice::BatchClearPriority,
                    ContextChoice::BatchMarkReady,
                    ContextChoice::BatchClearReady,
                    ContextChoice::BatchMarkDone,
                    ContextChoice::BatchReopen,
                    ContextChoice::BatchSnooze,
                    ContextChoice::BatchClearSnooze,
                ]);
            }
            choices.push(ContextChoice::SaveCurrentView);
            if self.active_saved_view().id.starts_with("view:") {
                choices.push(ContextChoice::EditCurrentView);
                choices.push(ContextChoice::DeleteCurrentView);
            }
        }
        if matches!(self.view, View::Workspace(_) | View::Review(_))
            && self.current_thread_id().is_some_and(|thread_id| {
                self.git_context(thread_id)
                    .is_some_and(|context| context.repo.is_some())
            })
        {
            choices.push(ContextChoice::LaunchPreset);
        }
        if let Some(target) = self.current_forge_mutation_target() {
            if target.change_request.is_some() {
                choices.extend([
                    ContextChoice::ForgeComment,
                    ContextChoice::ForgeApprove,
                    ContextChoice::ForgeMerge,
                ]);
            } else if target
                .identity
                .default_branch
                .as_deref()
                .is_some_and(|default_branch| default_branch != target.branch)
            {
                choices.push(ContextChoice::ForgeCreateMergeRequest);
            }
        }
        choices
    }

    pub fn context_choice(&self) -> Option<ContextChoice> {
        self.context_choices().get(self.context_selected).copied()
    }

    pub fn work_card_for_thread(&self, thread_id: &ThreadId) -> Option<&WorkCardProjection> {
        if let Some(index) = self.work_card_by_thread.get(&thread_id.0) {
            return self.work_cards.get(*index);
        }
        self.work_cards
            .iter()
            .find(|card| card.anchor == SourceRef::codex_thread(thread_id))
    }

    pub fn worktree_collision_count(&self, thread_id: &ThreadId) -> usize {
        if let Some(count) = self.worktree_collision_counts.get(&thread_id.0) {
            return *count;
        }
        self.worktree_collision_count_uncached(thread_id)
    }

    fn worktree_collision_count_uncached(&self, thread_id: &ThreadId) -> usize {
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

    fn rebuild_pending_request_threads(&mut self) {
        self.pending_request_threads = self
            .pending_requests
            .iter()
            .map(|request| request.thread_id.0.clone())
            .collect();
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

    pub fn visible_indices_with_match_count(&self) -> (Vec<usize>, usize) {
        let query = self.filter.trim().to_lowercase();
        let show_all_matches = self.show_all_history || !query.is_empty();
        let mut visible = Vec::with_capacity(if show_all_matches {
            self.threads.len()
        } else {
            REGISTRY_RECENT_LIMIT.min(self.threads.len())
        });
        let mut matched_count = 0_usize;

        for (index, thread) in self.threads.iter().enumerate() {
            if !self.thread_visible_in_registry_with_query(thread, &query) {
                continue;
            }
            let matched_position = matched_count;
            matched_count = matched_count.saturating_add(1);
            if show_all_matches
                || matched_position < REGISTRY_RECENT_LIMIT
                || thread.pinned
                || self.thread_needs_attention(index)
            {
                visible.push(index);
            }
        }

        (visible, matched_count)
    }

    pub fn registry_match_count(&self) -> usize {
        let query = self.filter.trim().to_lowercase();
        self.threads
            .iter()
            .filter(|thread| self.thread_visible_in_registry_with_query(thread, &query))
            .count()
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        self.visible_indices_with_match_count().0
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
        let interactive = self.pending_request_threads.contains(&thread.id.0);
        actionable
            || interactive
            || (!thread.attention.is_empty() && !self.acknowledged_attention.contains(&thread.id.0))
    }

    pub(crate) fn planning_reconcile_count(&self) -> usize {
        self.planning_reconcile_count
    }

    pub fn apply_local_state(&mut self, local: &LocalStateV1) {
        self.thread_ui = local.thread_ui.clone();
        self.local_pins = local.pins.clone();
        self.local_aliases = local.aliases.clone();
        self.local_marked_unread = local.marked_unread.clone();
        self.acknowledged_attention = local.acknowledged_attention.clone();
        self.host_local_only = local.host_local_only;
        self.repo_backed_only = local.repo_backed_only;
        if self.host_local_only || self.repo_backed_only {
            self.reconcile_cwd_locality_cache(true);
        }
        for thread in &mut self.threads {
            thread.pinned = self.local_pins.contains(&thread.id.0);
            thread.alias = self.local_aliases.get(&thread.id.0).cloned();
            thread
                .attention
                .retain(|reason| *reason != AttentionReason::MarkedUnread);
            if self.local_marked_unread.contains(&thread.id.0) {
                thread.attention.push(AttentionReason::MarkedUnread);
            }
        }
        ensure_selection_visible(self);
    }

    pub fn to_local_state(&self) -> LocalStateV1 {
        LocalStateV1 {
            schema_version: 1,
            thread_ui: self.thread_ui.clone(),
            pins: self.local_pins.clone(),
            aliases: self.local_aliases.clone(),
            marked_unread: self.local_marked_unread.clone(),
            acknowledged_attention: self.acknowledged_attention.clone(),
            host_local_only: self.host_local_only,
            repo_backed_only: self.repo_backed_only,
        }
    }
}

pub fn reduce(state: &mut AppState, action: Action) -> Vec<Effect> {
    if matches!(
        &action,
        Action::ReplaceThreads(_)
            | Action::BackendStatus(_)
            | Action::GitContextLoaded(_)
            | Action::ForgeObservationLoaded(_)
            | Action::ForgeReviewLoaded(_)
            | Action::PlanningSnapshotLoaded(_)
            | Action::GoalObserved(_)
            | Action::GoalCleared(_)
            | Action::TogglePin
            | Action::MarkUnread
            | Action::AcknowledgeAttention
            | Action::CommitInput
    ) {
        state.planning_generation = state.planning_generation.wrapping_add(1);
    }
    match action {
        Action::ReplaceThreads(mut threads) => {
            state.worktree_collision_counts.clear();
            let selected_id = state.selected_thread_id();
            let existing_remote_by_id = state
                .threads
                .iter()
                .map(|thread| (thread.id.0.clone(), remote_attention(thread)))
                .collect::<HashMap<_, _>>();
            for fresh in &mut threads {
                if let Some(existing_remote) = existing_remote_by_id.get(&fresh.id.0) {
                    let fresh_remote = remote_attention(fresh);
                    if existing_remote != &fresh_remote && !fresh_remote.is_empty() {
                        state.acknowledged_attention.remove(&fresh.id.0);
                    }
                }

                fresh.pinned = state.local_pins.contains(&fresh.id.0);
                fresh.alias = state.local_aliases.get(&fresh.id.0).cloned();
                fresh
                    .attention
                    .retain(|reason| *reason != AttentionReason::MarkedUnread);
                if state.local_marked_unread.contains(&fresh.id.0) {
                    fresh.attention.push(AttentionReason::MarkedUnread);
                }
            }
            state.threads = threads;
            state.rebuild_thread_indexes();
            state.reconcile_cwd_locality_cache(state.host_local_only || state.repo_backed_only);
            if state.threads.is_empty() {
                state.selected = 0;
            } else if let Some(id) = selected_id {
                state.selected = state
                    .thread_index_by_id
                    .get(&id.0)
                    .copied()
                    .unwrap_or_else(|| state.selected.min(state.threads.len() - 1));
            } else {
                state.selected = state.selected.min(state.threads.len() - 1);
            }
            ensure_selection_visible(state);
        }
        Action::BackendStatus(status) => state.backend_status = status,
        Action::RefreshGitProjections => {
            return refresh_git_projections(state);
        }
        Action::RefreshActiveGitProjections => {
            return refresh_active_git_projections(state);
        }
        Action::RefreshForgeProjections => {
            return refresh_forge_projections(state);
        }
        Action::GitContextLoaded(context) => {
            state.worktree_collision_counts.clear();
            if context.observed_at_unix_ms > 0 && context.error.is_none() {
                state
                    .cwd_localities
                    .insert(context.cwd.clone(), CwdLocality::LocalDirectory);
            }
            propagate_git_context(state, context);
            ensure_selection_visible(state);
        }
        Action::ForgeObservationLoaded(observation) => {
            propagate_forge_observation(state, observation);
        }
        Action::ForgeReviewLoaded(review) => {
            if let Some(observation) = state.forge_observations.get_mut(&review.thread_id.0)
                && observation.cwd == review.cwd
                && observation
                    .change_requests
                    .iter()
                    .any(|change| change.iid == review.change_request_iid)
            {
                observation.capabilities.insert(
                    ForgeCapability::ApprovalSummary,
                    if review.approvals_available {
                        CapabilityState::Available
                    } else {
                        CapabilityState::Unavailable
                    },
                );
                observation.capabilities.insert(
                    ForgeCapability::Discussions,
                    if review.discussions_available {
                        CapabilityState::Available
                    } else {
                        CapabilityState::Unavailable
                    },
                );
                observation.review = Some(review);
            }
        }
        Action::GitReviewLoaded(review) => state.install_git_review(review),
        Action::PlanningSnapshotLoaded(snapshot) => {
            let selection = planning_selection::PlanningSelection::capture(state);
            state.planning_snapshot = snapshot;
            selection.restore(state);
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
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "current thread is not in a Git repository",
                        "当前会话不在 Git 仓库中",
                    )
                    .into(),
                );
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
                operation_state_text(receipt.state, state.language)
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
        Action::ForgeMutationReceipt(receipt) => {
            let receipt = *receipt;
            if state
                .pending_forge_operation
                .as_ref()
                .is_some_and(|plan| plan.operation_id == receipt.operation_id)
            {
                state.pending_forge_operation = None;
                state.pending_forge_payload = None;
            }
            state.mutation_notice = Some(format!(
                "{} · {}",
                receipt.plan.kind.label(),
                operation_state_text(receipt.state, state.language)
            ));

            for observation in state.forge_observations.values_mut() {
                if observation.identity.as_ref().is_some_and(|identity| {
                    identity.provider == receipt.plan.provider
                        && identity.host.eq_ignore_ascii_case(&receipt.plan.host)
                        && identity.project_id == receipt.plan.project_id
                }) {
                    observation.observed_at_unix_ms = 1;
                    observation.review = None;
                }
            }

            state
                .recent_forge_operations
                .retain(|item| item.operation_id != receipt.operation_id);
            state.recent_forge_operations.insert(0, receipt);
            state.recent_forge_operations.truncate(20);
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
            state.pending_forge_operation = None;
            state.pending_forge_payload = None;
            let Some(thread_id) = state.current_thread_id().cloned() else {
                return vec![];
            };
            let Some(context) = state.git_context(&thread_id) else {
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "Git context is unavailable",
                        "Git 上下文不可用",
                    )
                    .into(),
                );
                return vec![];
            };
            if context.repo.is_none() {
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "current cwd is not a Git repository",
                        "当前 cwd 不是 Git 仓库",
                    )
                    .into(),
                );
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
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "current cwd is not a Git worktree",
                        "当前 cwd 不是 Git worktree",
                    )
                    .into(),
                );
                return vec![];
            };
            if state.managed_worktrees.iter().any(|record| {
                record.repo == *repo && record.canonical_path == worktree.canonical_path
            }) {
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "current worktree is already managed/adopted",
                        "当前 worktree 已受管或已接管",
                    )
                    .into(),
                );
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
            if let Some(mutation) = state.pending_thread_queue_mutation.take() {
                return vec![Effect::MutateThreadQueue(mutation)];
            }
            if state.planning_store_error.is_some() {
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "local store is degraded; mutation receipts cannot be persisted",
                        "本地存储已降级；无法持久化变更回执",
                    )
                    .into(),
                );
                return vec![];
            }
            if let Some(plan) = state.pending_local_batch.take() {
                return vec![Effect::ApplyLocalBatch(Box::new(plan))];
            }
            if let Some(plan) = state.pending_launch_plan.take() {
                return vec![Effect::ExecuteLaunchPreset(Box::new(plan))];
            }
            if let Some(plan) = state.pending_forge_operation.take() {
                let payload = state.pending_forge_payload.take();
                return vec![Effect::ExecuteForgeOperation(Box::new(
                    ForgeMutationRequest { plan, payload },
                ))];
            }
            let Some(plan) = state.pending_operation.take() else {
                return vec![];
            };
            return vec![Effect::ExecuteOperation(Box::new(plan))];
        }
        Action::CancelPendingOperation => {
            state.pending_operation = None;
            state.pending_forge_operation = None;
            state.pending_forge_payload = None;
            state.pending_local_batch = None;
            state.pending_launch_plan = None;
            state.pending_thread_queue_mutation = None;
            state.mutation_notice = Some(
                local_text(
                    state.language,
                    "operation cancelled before execution",
                    "操作已在执行前取消",
                )
                .into(),
            );
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
        Action::ThreadCreated {
            thread_id,
            operation,
        } => {
            return state.apply_thread_created(thread_id, operation);
        }
        Action::ThreadLifecycleFailed { operation, error } => {
            state.apply_thread_lifecycle_failed(operation, error);
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
        Action::OpenThreadQueue => {
            let Some(thread_id) = state.current_thread_id().cloned() else {
                return vec![];
            };
            state.goal_actions_open = false;
            state.thread_queue_open = true;
            state.thread_queue_loading = true;
            state.thread_queue_error = None;
            state.thread_queue_selected = 0;
            if state
                .thread_queue_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.thread_id != thread_id)
            {
                state.thread_queue_snapshot = None;
            }
            return vec![Effect::RefreshThreadQueue(thread_id)];
        }
        Action::CloseThreadQueue => {
            let thread_id = state
                .thread_queue_snapshot
                .as_ref()
                .map(|snapshot| snapshot.thread_id.clone())
                .or_else(|| state.current_thread_id().cloned());
            state.thread_queue_open = false;
            state.thread_queue_loading = false;
            state.thread_queue_error = None;
            state.pending_thread_queue_mutation = None;
            state.input_mode = InputMode::Normal;
            state.input_buffer.clear();
            if let Some(thread_id) = thread_id {
                return vec![Effect::StopWatchingThreadQueue(thread_id)];
            }
        }
        Action::ThreadQueueLoaded(snapshot) => {
            if state.thread_queue_open
                && state
                    .current_thread_id()
                    .is_some_and(|thread_id| *thread_id == snapshot.thread_id)
            {
                let len = snapshot.submissions.len();
                state.thread_queue_snapshot = Some(snapshot);
                state.thread_queue_loading = false;
                state.thread_queue_error = None;
                state.thread_queue_selected = if len == 0 {
                    0
                } else {
                    state.thread_queue_selected.min(len - 1)
                };
            }
        }
        Action::ThreadQueueFailed { thread_id, error } => {
            if state.thread_queue_open
                && state
                    .current_thread_id()
                    .is_some_and(|current| *current == thread_id)
            {
                state.thread_queue_loading = false;
                state.thread_queue_error = Some(error);
            }
        }
        Action::MoveThreadQueue(delta) => {
            let len = state
                .thread_queue_snapshot
                .as_ref()
                .map(|snapshot| snapshot.submissions.len())
                .unwrap_or(0);
            if len == 0 {
                state.thread_queue_selected = 0;
            } else {
                state.thread_queue_selected =
                    (state.thread_queue_selected as i32 + delta).rem_euclid(len as i32) as usize;
            }
        }
        Action::BeginThreadQueueAdd => {
            if state.thread_queue_open {
                state.input_buffer.clear();
                state.input_mode = InputMode::ThreadQueueAdd;
            }
        }
        Action::BeginThreadQueueEdit => {
            let Some(text) = state
                .selected_thread_queue_submission()
                .and_then(|submission| submission.editable_text.clone())
            else {
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "this queued item contains non-text input and cannot be text-edited safely",
                        "该队列项包含非文本输入，无法安全地按文本编辑",
                    )
                    .into(),
                );
                return vec![];
            };
            state.input_buffer = text;
            state.input_mode = InputMode::ThreadQueueEdit;
        }
        Action::BeginThreadQueueDelete => {
            let Some(thread_id) = state.current_thread_id().cloned() else {
                return vec![];
            };
            let Some(id) = state
                .selected_thread_queue_submission()
                .map(|submission| submission.id.clone())
            else {
                return vec![];
            };
            match ThreadQueueMutation::delete(thread_id, id) {
                Ok(mutation) => state.pending_thread_queue_mutation = Some(mutation),
                Err(error) => state.thread_queue_error = Some(error.to_string()),
            }
        }
        Action::BeginThreadQueueStart => {
            let Some(thread_id) = state.current_thread_id().cloned() else {
                return vec![];
            };
            let Some(id) = state
                .selected_thread_queue_submission()
                .map(|submission| submission.id.clone())
            else {
                return vec![];
            };
            match ThreadQueueMutation::start(thread_id, id) {
                Ok(mutation) => state.pending_thread_queue_mutation = Some(mutation),
                Err(error) => state.thread_queue_error = Some(error.to_string()),
            }
        }
        Action::ReorderThreadQueue(delta) => {
            let Some(snapshot) = state.thread_queue_snapshot.as_ref() else {
                return vec![];
            };
            let len = snapshot.submissions.len();
            if len < 2 {
                return vec![];
            }
            let from = state.thread_queue_selected.min(len - 1);
            let to = (from as i32 + delta).clamp(0, (len - 1) as i32) as usize;
            if from == to {
                return vec![];
            }
            let mut ids = snapshot
                .submissions
                .iter()
                .map(|submission| submission.id.clone())
                .collect::<Vec<_>>();
            ids.swap(from, to);
            let thread_id = snapshot.thread_id.clone();
            match ThreadQueueMutation::reorder(thread_id, ids) {
                Ok(mutation) => {
                    state.thread_queue_selected = to;
                    state.thread_queue_loading = true;
                    return vec![Effect::MutateThreadQueue(mutation)];
                }
                Err(error) => state.thread_queue_error = Some(error.to_string()),
            }
        }
        Action::RefreshThreadQueue => {
            let Some(thread_id) = state.current_thread_id().cloned() else {
                return vec![];
            };
            state.thread_queue_loading = true;
            state.thread_queue_error = None;
            return vec![Effect::RefreshThreadQueue(thread_id)];
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
            ensure_selection_visible(state);
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
        Action::CloseSavedViewEditor => {
            state.saved_view_editor = None;
            state.saved_view_editor_error = None;
            if state.input_mode == InputMode::SavedViewField {
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
            }
        }
        Action::MoveSavedViewEditorField(delta) => {
            if let Some(editor) = state.saved_view_editor.as_mut() {
                editor.move_field(delta);
                state.saved_view_editor_error = None;
            }
        }
        Action::CycleSavedViewEditorValue(delta) => {
            if let Some(editor) = state.saved_view_editor.as_mut() {
                editor.cycle_value(delta);
                state.saved_view_editor_error = None;
            }
        }
        Action::BeginSavedViewFieldEdit => {
            if let Some(editor) = state.saved_view_editor.as_ref()
                && let Some(value) = editor.begin_text_value()
            {
                state.input_buffer = value;
                state.input_mode = InputMode::SavedViewField;
                state.saved_view_editor_error = None;
            }
        }
        Action::SaveSavedViewEditor => {
            let Some(editor) = state.saved_view_editor.as_ref() else {
                return vec![];
            };
            if let Err(error) = editor.validate() {
                state.saved_view_editor_error = Some(error);
                return vec![];
            }
            let view = editor.draft.clone();
            state.saved_view_editor = None;
            state.saved_view_editor_error = None;
            state.input_mode = InputMode::Normal;
            state.input_buffer.clear();
            return vec![Effect::SaveSavedView { view }];
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
                        state.prepare_conversation(&id);
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
        Action::OpenCommandPalette => state.open_command_palette(),
        Action::CloseCommandPalette => state.close_command_palette(),
        Action::MoveCommandPalette(delta) => state.move_command_palette(delta),
        Action::CommandPaletteInputChar(character) => {
            state.input_command_palette_char(character);
        }
        Action::CommandPaletteInputText(text) => state.input_command_palette_text(text),
        Action::CommandPaletteBackspace => state.backspace_command_palette(),
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

            if choice == ContextChoice::NewCodexThread {
                return state.plan_start_thread();
            }
            if choice == ContextChoice::ForkCodexThread {
                return state.plan_fork_thread();
            }

            if choice == ContextChoice::SaveCurrentView {
                state.saved_view_editor =
                    Some(SavedViewEditor::create_from(&state.active_saved_view()));
                state.saved_view_editor_error = None;
                state.show_help = false;
                return vec![];
            }
            if choice == ContextChoice::EditCurrentView {
                match SavedViewEditor::edit(&state.active_saved_view()) {
                    Ok(editor) => {
                        state.saved_view_editor = Some(editor);
                        state.saved_view_editor_error = None;
                    }
                    Err(error) => state.saved_view_editor_error = Some(error),
                }
                return vec![];
            }
            if choice == ContextChoice::DeleteCurrentView {
                let view_id = state.active_saved_view().id;
                if view_id.starts_with("view:") {
                    return vec![Effect::DeleteSavedView { view_id }];
                }
                return vec![];
            }

            if matches!(
                choice,
                ContextChoice::BatchAddTag
                    | ContextChoice::BatchRemoveTag
                    | ContextChoice::BatchSetPriority
                    | ContextChoice::BatchClearPriority
                    | ContextChoice::BatchMarkReady
                    | ContextChoice::BatchClearReady
                    | ContextChoice::BatchMarkDone
                    | ContextChoice::BatchReopen
                    | ContextChoice::BatchSnooze
                    | ContextChoice::BatchClearSnooze
            ) {
                match choice {
                    ContextChoice::BatchAddTag => {
                        state.input_buffer.clear();
                        state.input_mode = InputMode::BatchAddTag;
                    }
                    ContextChoice::BatchRemoveTag => {
                        state.input_buffer.clear();
                        state.input_mode = InputMode::BatchRemoveTag;
                    }
                    ContextChoice::BatchSetPriority => {
                        state.input_buffer = "0".into();
                        state.input_mode = InputMode::BatchPriority;
                    }
                    ContextChoice::BatchSnooze => {
                        state.input_buffer = "1h".into();
                        state.input_mode = InputMode::BatchSnooze;
                    }
                    ContextChoice::BatchClearPriority => {
                        state.freeze_visible_batch(LocalBatchAction::ClearPriority);
                    }
                    ContextChoice::BatchMarkReady => {
                        state.freeze_visible_batch(LocalBatchAction::SetReady(true));
                    }
                    ContextChoice::BatchClearReady => {
                        state.freeze_visible_batch(LocalBatchAction::SetReady(false));
                    }
                    ContextChoice::BatchMarkDone => {
                        state.freeze_visible_batch(LocalBatchAction::SetDone(true));
                    }
                    ContextChoice::BatchReopen => {
                        state.freeze_visible_batch(LocalBatchAction::SetDone(false));
                    }
                    ContextChoice::BatchClearSnooze => {
                        state.freeze_visible_batch(LocalBatchAction::SnoozeUntil(None));
                    }
                    _ => {
                        state.mutation_notice = Some(
                            local_text(
                                state.language,
                                "context action routing mismatch; no operation executed",
                                "上下文操作路由不匹配；未执行任何操作",
                            )
                            .into(),
                        );
                        return vec![];
                    }
                }
                return vec![];
            }

            if choice == ContextChoice::LaunchPreset {
                let Some(thread_id) = state.current_thread_id().cloned() else {
                    return vec![];
                };
                let Some(context) = state.git_context(&thread_id) else {
                    state.mutation_notice = Some(
                        local_text(
                            state.language,
                            "Git context unavailable for launch presets",
                            "启动预设所需的 Git 上下文不可用",
                        )
                        .into(),
                    );
                    return vec![];
                };
                let Some(repo) = context.repo.as_ref() else {
                    state.mutation_notice = Some(
                        local_text(
                            state.language,
                            "repository root unavailable for launch presets",
                            "启动预设所需的仓库根目录不可用",
                        )
                        .into(),
                    );
                    return vec![];
                };
                return vec![Effect::LoadLaunchPresets {
                    repo_root: repo.primary_root.clone(),
                    thread_cwd: context.cwd.clone(),
                }];
            }

            if matches!(
                choice,
                ContextChoice::ForgeCreateMergeRequest
                    | ContextChoice::ForgeComment
                    | ContextChoice::ForgeApprove
                    | ContextChoice::ForgeMerge
            ) {
                let Some(target) = state.current_forge_mutation_target() else {
                    state.mutation_notice = Some(
                        local_text(
                            state.language,
                            "forge mutation target is unavailable",
                            "Forge 变更目标不可用",
                        )
                        .into(),
                    );
                    return vec![];
                };
                state.pending_operation = None;
                match choice {
                    ContextChoice::ForgeCreateMergeRequest => {
                        state.pending_forge_operation = None;
                        state.pending_forge_payload = None;
                        state.input_buffer.clear();
                        state.input_mode = InputMode::ForgeMergeRequestTitle;
                    }
                    ContextChoice::ForgeComment => {
                        state.pending_forge_operation = None;
                        state.pending_forge_payload = None;
                        state.input_buffer.clear();
                        state.input_mode = InputMode::ForgeComment;
                    }
                    ContextChoice::ForgeApprove | ContextChoice::ForgeMerge => {
                        let Some(change) = target.change_request else {
                            state.mutation_notice = Some(
                                local_text(
                                    state.language,
                                    "current branch has no open forge change request",
                                    "当前分支没有打开的 Forge 变更请求",
                                )
                                .into(),
                            );
                            return vec![];
                        };
                        let planned_at = now_unix_ms();
                        let plan = match choice {
                            ContextChoice::ForgeApprove => {
                                ForgeMutationPlan::approve_merge_request(
                                    &target.identity,
                                    target.cwd,
                                    change.iid,
                                    change.source_branch,
                                    change.target_branch,
                                    planned_at,
                                )
                            }
                            ContextChoice::ForgeMerge => ForgeMutationPlan::merge_merge_request(
                                &target.identity,
                                target.cwd,
                                change.iid,
                                change.source_branch,
                                change.target_branch,
                                planned_at,
                            ),
                            _ => {
                                state.mutation_notice = Some(
                                    local_text(
                                        state.language,
                                        "forge action routing mismatch; no mutation executed",
                                        "Forge 操作路由不匹配；未执行变更",
                                    )
                                    .into(),
                                );
                                return vec![];
                            }
                        };
                        match plan {
                            Ok(plan) => {
                                state.pending_forge_operation = Some(plan);
                                state.pending_forge_payload = None;
                                state.mutation_notice = None;
                            }
                            Err(error) => {
                                state.mutation_notice = Some(format!(
                                    "{}: {error:#}",
                                    local_text(
                                        state.language,
                                        "cannot create forge mutation plan",
                                        "无法创建 Forge 变更计划",
                                    )
                                ));
                            }
                        }
                    }
                    _ => {
                        state.mutation_notice = Some(
                            local_text(
                                state.language,
                                "forge action routing mismatch; no mutation executed",
                                "Forge 操作路由不匹配；未执行变更",
                            )
                            .into(),
                        );
                        return vec![];
                    }
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
                        _ => {
                            state.mutation_notice = Some(
                                local_text(
                                    state.language,
                                    "scratch action routing mismatch; no write executed",
                                    "Scratch 操作路由不匹配；未执行写入",
                                )
                                .into(),
                            );
                            return vec![];
                        }
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
                ContextChoice::NewCodexThread
                | ContextChoice::ForkCodexThread
                | ContextChoice::SaveCurrentView
                | ContextChoice::EditCurrentView
                | ContextChoice::DeleteCurrentView
                | ContextChoice::BatchAddTag
                | ContextChoice::BatchRemoveTag
                | ContextChoice::BatchSetPriority
                | ContextChoice::BatchClearPriority
                | ContextChoice::BatchMarkReady
                | ContextChoice::BatchClearReady
                | ContextChoice::BatchMarkDone
                | ContextChoice::BatchReopen
                | ContextChoice::BatchSnooze
                | ContextChoice::BatchClearSnooze
                | ContextChoice::LaunchPreset
                | ContextChoice::ForgeCreateMergeRequest
                | ContextChoice::ForgeComment
                | ContextChoice::ForgeApprove
                | ContextChoice::ForgeMerge => {
                    state.mutation_notice = Some(
                        local_text(
                            state.language,
                            "context action is unavailable in the current state",
                            "当前状态下此上下文操作不可用",
                        )
                        .into(),
                    );
                }
            }
        }
        Action::LaunchPresetsLoaded {
            repo_root,
            thread_cwd,
            presets,
        } => {
            state.launch_repo_root = Some(repo_root);
            state.launch_thread_cwd = Some(thread_cwd);
            state.launch_presets = presets;
            state.launch_selected = 0;
            state.launch_menu_open = true;
            state.mutation_notice = None;
        }
        Action::CloseLaunchPresets => {
            state.launch_menu_open = false;
            state.launch_selected = 0;
        }
        Action::MoveLaunchPreset(delta) => {
            let len = state.launch_presets.len();
            if len == 0 {
                state.launch_selected = 0;
            } else {
                state.launch_selected =
                    (state.launch_selected as i32 + delta).rem_euclid(len as i32) as usize;
            }
        }
        Action::SelectLaunchPreset => {
            let Some(preset) = state.launch_presets.get(state.launch_selected).cloned() else {
                state.launch_menu_open = false;
                return vec![];
            };
            let (Some(repo_root), Some(thread_cwd)) = (
                state.launch_repo_root.clone(),
                state.launch_thread_cwd.clone(),
            ) else {
                state.launch_menu_open = false;
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "launch preset scope is unavailable",
                        "启动预设作用域不可用",
                    )
                    .into(),
                );
                return vec![];
            };
            state.launch_menu_open = false;
            return vec![Effect::PrepareLaunchPreset {
                preset,
                repo_root,
                thread_cwd,
            }];
        }
        Action::LaunchPlanPrepared(plan) => {
            state.pending_operation = None;
            state.pending_forge_operation = None;
            state.pending_forge_payload = None;
            state.pending_local_batch = None;
            state.pending_launch_plan = Some(plan);
            state.mutation_notice = None;
        }
        Action::ToggleTerminalDrawer => {
            if state.terminal_drawer_open {
                state.terminal_focused = true;
                return vec![];
            }
            let cwd = match state.terminal_target_cwd() {
                Ok(cwd) => cwd,
                Err(error) => {
                    state.mutation_notice = Some(error);
                    return vec![];
                }
            };
            state.mutation_notice = None;
            state.terminal_drawer_open = true;
            state.terminal_focused = true;
            state.terminal_snapshot = None;
            return vec![Effect::OpenTerminalDrawer { cwd }];
        }
        Action::CloseTerminalDrawer => {
            let was_open = state.terminal_drawer_open;
            state.terminal_drawer_open = false;
            state.terminal_focused = false;
            state.terminal_snapshot = None;
            if was_open {
                return vec![Effect::CloseTerminalDrawer];
            }
        }
        Action::SetTerminalFocus(focused) => {
            if state.terminal_drawer_open {
                state.terminal_focused = focused;
            }
        }
        Action::TerminalSnapshot(snapshot) => {
            if state.terminal_drawer_open {
                state.terminal_snapshot = Some(snapshot);
            }
        }
        Action::TerminalScroll(delta) => {
            if state.terminal_drawer_open {
                return vec![Effect::TerminalScroll(delta)];
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
                .or_insert_with(|| GitReview::pending(thread_id.clone(), ""));
            review.error = Some(error);
            state.touch_git_review_cache(&thread_id);
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
            state.prepare_git_review(&thread_id, cwd.clone());

            let forge_review_effect = state
                .git_context(&thread_id)
                .and_then(|context| context.branch.as_deref())
                .and_then(|branch| {
                    state.forge_observation(&thread_id).and_then(|observation| {
                        let identity = observation.identity.as_ref()?;
                        let change = observation.change_request_for_branch(branch)?;
                        let already_loaded = observation.review.as_ref().is_some_and(|review| {
                            review.change_request_iid == change.iid && review.cwd == cwd
                        });
                        (!already_loaded).then(|| {
                            Effect::ProbeForgeReview(ForgeReviewTarget {
                                thread_id: thread_id.clone(),
                                cwd: cwd.clone(),
                                provider: identity.provider,
                                host: identity.host.clone(),
                                project_id: identity.project_id.clone(),
                                project_path: identity.path_with_namespace.clone(),
                                change_request_iid: change.iid,
                            })
                        })
                    })
                });

            state.view = View::Review(thread_id.clone());
            let mut effects = vec![Effect::LoadGitReview { thread_id, cwd }];
            if let Some(effect) = forge_review_effect {
                effects.push(effect);
            }
            return effects;
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

            let Some(cwd) = state
                .thread_by_id(&thread_id)
                .map(|thread| thread.metadata.cwd.clone())
            else {
                return vec![];
            };
            if cwd.trim().is_empty() {
                return vec![];
            }
            let needs_probe = state
                .git_contexts
                .get(&thread_id.0)
                .is_none_or(|context| context.cwd != cwd);
            if needs_probe {
                state.git_contexts.insert(
                    thread_id.0.clone(),
                    GitContext::pending(thread_id.clone(), cwd.clone()),
                );
                return vec![Effect::ProbeGit { thread_id, cwd }];
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
        Action::OpenReviewExternal => {
            return state.open_review_external();
        }
        Action::ConversationLoaded(page) => {
            let thread_id = page.thread_id.clone();
            let key = thread_id.0.clone();
            state
                .conversations
                .entry(key)
                .or_insert_with(|| ConversationState::loading(thread_id.clone()))
                .replace_page(page);
            state.touch_conversation_cache(&thread_id);
        }
        Action::OlderConversationLoaded(page) => {
            let thread_id = page.thread_id.clone();
            let key = thread_id.0.clone();
            state
                .conversations
                .entry(key)
                .or_insert_with(|| ConversationState::loading(thread_id.clone()))
                .prepend_page(page);
            state.touch_conversation_cache(&thread_id);
        }
        Action::TranscriptSearchLoadedPage { page, item_id } => {
            let thread_id = page.thread_id.clone();
            let key = thread_id.0.clone();
            let target_index = page
                .items
                .iter()
                .position(|item| item.item_id == item_id)
                .unwrap_or(0);
            state
                .conversations
                .entry(key)
                .or_insert_with(|| ConversationState::loading(thread_id.clone()))
                .replace_page(page);
            let ui = state.thread_ui.entry(thread_id.0.clone()).or_default();
            ui.follow = false;
            ui.scroll = u16::try_from(target_index.saturating_sub(2)).unwrap_or(u16::MAX);
            state.touch_conversation_cache(&thread_id);
        }
        Action::ConversationFailed { thread_id, error } => {
            let conversation = state
                .conversations
                .entry(thread_id.0.clone())
                .or_insert_with(|| ConversationState::loading(thread_id.clone()));
            conversation.loading = false;
            conversation.loading_older = false;
            conversation.error = Some(error);
            state.touch_conversation_cache(&thread_id);
        }
        Action::PromptSubmitted {
            thread_id,
            request_id,
        } => {
            return prompt::acknowledge(state, thread_id, request_id);
        }
        Action::PromptFailed {
            thread_id,
            request_id,
            error,
        } => {
            prompt::fail(state, thread_id, request_id, error);
        }
        Action::InteractiveRequested(request) => {
            state.acknowledged_attention.remove(&request.thread_id.0);
            let is_current_thread = state.current_thread_id() == Some(&request.thread_id);
            state
                .pending_requests
                .retain(|pending| pending.request_id != request.request_id);
            state.pending_requests.push(request);
            state.rebuild_pending_request_threads();
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
            state.rebuild_pending_request_threads();
            if state.user_input_request_id.as_ref() == Some(&request_id) {
                clear_user_input_editor(state);
            }
            ensure_selection_visible(state);
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
                state.prepare_conversation(&id);
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
                    state.view = View::Thread(id.clone());
                    id
                }
            };
            state.thread_ui.entry(id.0.clone()).or_default();
            state.prepare_conversation(&id);
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
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "operation cancelled before execution",
                        "操作已在执行前取消",
                    )
                    .into(),
                );
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
                state.prompt_submissions.edited(&id);
                state.thread_ui.entry(id.0).or_default().draft = draft;
                return vec![Effect::PersistOperatorStateDeferred];
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
                return vec![Effect::PersistOperatorStateDeferred];
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
                state.local_marked_unread.insert(thread.id.0.clone());
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
                if thread.pinned {
                    state.local_pins.insert(thread.id.0.clone());
                } else {
                    state.local_pins.remove(&thread.id.0);
                }
                ensure_selection_visible(state);
                return vec![Effect::PersistOperatorState];
            }
        }
        Action::AcknowledgeAttention => {
            if state.selected_thread().is_some()
                && let Some(thread) = state.threads.get_mut(state.selected)
            {
                state.acknowledged_attention.insert(thread.id.0.clone());
                state.local_marked_unread.remove(&thread.id.0);
                thread
                    .attention
                    .retain(|reason| *reason != AttentionReason::MarkedUnread);
                ensure_selection_visible(state);
                return vec![Effect::PersistOperatorState];
            }
        }
        Action::ToggleHostLocalFilter => {
            state.host_local_only = !state.host_local_only;
            if state.host_local_only {
                state.cwd_localities.clear();
                state.reconcile_cwd_locality_cache(true);
            }
            ensure_selection_visible(state);
            return vec![Effect::PersistOperatorState];
        }
        Action::ToggleRepoBackedFilter => {
            state.repo_backed_only = !state.repo_backed_only;
            if state.repo_backed_only {
                state.cwd_localities.clear();
                state.reconcile_cwd_locality_cache(true);
            }
            ensure_selection_visible(state);
            return vec![Effect::PersistOperatorState];
        }
        Action::ToggleAllHistory => {
            state.show_all_history = !state.show_all_history;
            ensure_selection_visible(state);
        }
        Action::BeginSearch => {
            state.search_return_view = if matches!(state.view, View::Registry) {
                None
            } else {
                Some(state.view.clone())
            };
            if state.search_return_view.is_some() {
                state.view = View::Registry;
                ensure_selection_visible(state);
            }
            state.input_original.clone_from(&state.filter);
            state.input_buffer.clone_from(&state.filter);
            state.input_mode = InputMode::Search;
        }
        Action::BeginTranscriptSearch => {
            state.input_buffer.clear();
            state.input_original.clear();
            state.input_mode = InputMode::TranscriptSearch;
            state.transcript_search_error = None;
        }
        Action::TranscriptSearchLoaded(results) => {
            if results.query == state.transcript_search_query {
                let replace = state
                    .transcript_search_results
                    .as_ref()
                    .is_none_or(|current| {
                        current.source != TranscriptSearchSource::AppServer
                            || results.source == TranscriptSearchSource::AppServer
                    });
                if replace {
                    if results.source == TranscriptSearchSource::AppServer {
                        state.transcript_search_loading = false;
                        state.transcript_search_error = None;
                    }
                    state.transcript_search_results = Some(results);
                    let len = state
                        .transcript_search_results
                        .as_ref()
                        .map(|results| results.hits.len())
                        .unwrap_or(0);
                    if len == 0 {
                        state.transcript_search_selected = 0;
                    } else {
                        state.transcript_search_selected =
                            state.transcript_search_selected.min(len - 1);
                    }
                }
            }
        }
        Action::TranscriptSearchServerFailed { query, error } => {
            if query == state.transcript_search_query {
                state.transcript_search_loading = false;
                state.transcript_search_error = Some(error);
            }
        }
        Action::CloseTranscriptSearch => {
            state.transcript_search_open = false;
            state.transcript_search_selected = 0;
        }
        Action::MoveTranscriptSearch(delta) => {
            let len = state
                .transcript_search_results
                .as_ref()
                .map(|results| results.hits.len())
                .unwrap_or(0);
            if len == 0 {
                state.transcript_search_selected = 0;
            } else {
                state.transcript_search_selected = (state.transcript_search_selected as i32 + delta)
                    .rem_euclid(len as i32)
                    as usize;
            }
        }
        Action::OpenTranscriptSearchSelected => {
            let Some(hit) = state.transcript_search_hit().cloned() else {
                return vec![];
            };
            let thread_id = hit.thread_id.clone();
            state.transcript_search_open = false;
            state.previous_target = state.current_thread_id().cloned();
            state.thread_ui.entry(thread_id.0.clone()).or_default();
            state.prepare_conversation(&thread_id);
            state.view = View::Thread(thread_id.clone());
            state.transcript_search_active_hit = Some(hit.clone());
            return if hit.turn_id.is_some() && hit.item_id.is_some() {
                vec![Effect::JumpToTranscriptHit(hit)]
            } else {
                vec![Effect::LoadConversation(thread_id)]
            };
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
                    state.prompt_submissions.edited(&id);
                    state
                        .thread_ui
                        .entry(id.0)
                        .or_default()
                        .draft
                        .push(character);
                    return vec![Effect::PersistOperatorStateDeferred];
                }
            }
            InputMode::Search
            | InputMode::TranscriptSearch
            | InputMode::ThreadQueueAdd
            | InputMode::ThreadQueueEdit
            | InputMode::Alias
            | InputMode::UserInput
            | InputMode::ScratchTitle
            | InputMode::Snooze
            | InputMode::Note
            | InputMode::SavedViewField
            | InputMode::BatchAddTag
            | InputMode::BatchRemoveTag
            | InputMode::BatchPriority
            | InputMode::BatchSnooze
            | InputMode::GoalObjective
            | InputMode::WorktreeCreateBranch
            | InputMode::WorktreeCreatePath
            | InputMode::WorktreeCreateStartPoint
            | InputMode::WorktreeDeleteBranch
            | InputMode::ForgeMergeRequestTitle
            | InputMode::ForgeComment => {
                state.input_buffer.push(character);
                if state.input_mode == InputMode::Search {
                    state.filter.clone_from(&state.input_buffer);
                    if filter_requires_locality(&state.filter) {
                        state.reconcile_cwd_locality_cache(true);
                    }
                    ensure_selection_visible(state);
                }
            }
        },
        Action::InputText(text) => {
            if text.is_empty() {
                return vec![];
            }
            let text = normalize_pasted_input(state.input_mode, &text);
            if text.is_empty() {
                return vec![];
            }
            match state.input_mode {
                InputMode::Normal => {}
                InputMode::Composer => {
                    if let Some(id) = state.current_thread_id().cloned() {
                        state.prompt_submissions.edited(&id);
                        state
                            .thread_ui
                            .entry(id.0)
                            .or_default()
                            .draft
                            .push_str(&text);
                        return vec![Effect::PersistOperatorStateDeferred];
                    }
                }
                InputMode::Search
                | InputMode::TranscriptSearch
                | InputMode::ThreadQueueAdd
                | InputMode::ThreadQueueEdit
                | InputMode::Alias
                | InputMode::UserInput
                | InputMode::ScratchTitle
                | InputMode::Snooze
                | InputMode::Note
                | InputMode::SavedViewField
                | InputMode::BatchAddTag
                | InputMode::BatchRemoveTag
                | InputMode::BatchPriority
                | InputMode::BatchSnooze
                | InputMode::GoalObjective
                | InputMode::WorktreeCreateBranch
                | InputMode::WorktreeCreatePath
                | InputMode::WorktreeCreateStartPoint
                | InputMode::WorktreeDeleteBranch
                | InputMode::ForgeMergeRequestTitle
                | InputMode::ForgeComment => {
                    state.input_buffer.push_str(&text);
                    if state.input_mode == InputMode::Search {
                        state.filter.clone_from(&state.input_buffer);
                        if filter_requires_locality(&state.filter) {
                            state.reconcile_cwd_locality_cache(true);
                        }
                        ensure_selection_visible(state);
                    }
                }
            }
        }
        Action::InputBackspace => match state.input_mode {
            InputMode::Normal => {}
            InputMode::Composer => {
                if let Some(id) = state.current_thread_id().cloned() {
                    state.prompt_submissions.edited(&id);
                    state.thread_ui.entry(id.0).or_default().draft.pop();
                    return vec![Effect::PersistOperatorStateDeferred];
                }
            }
            InputMode::Search
            | InputMode::TranscriptSearch
            | InputMode::ThreadQueueAdd
            | InputMode::ThreadQueueEdit
            | InputMode::Alias
            | InputMode::UserInput
            | InputMode::ScratchTitle
            | InputMode::Snooze
            | InputMode::Note
            | InputMode::SavedViewField
            | InputMode::BatchAddTag
            | InputMode::BatchRemoveTag
            | InputMode::BatchPriority
            | InputMode::BatchSnooze
            | InputMode::GoalObjective
            | InputMode::WorktreeCreateBranch
            | InputMode::WorktreeCreatePath
            | InputMode::WorktreeCreateStartPoint
            | InputMode::WorktreeDeleteBranch
            | InputMode::ForgeMergeRequestTitle
            | InputMode::ForgeComment => {
                state.input_buffer.pop();
                if state.input_mode == InputMode::Search {
                    state.filter.clone_from(&state.input_buffer);
                    if filter_requires_locality(&state.filter) {
                        state.reconcile_cwd_locality_cache(true);
                    }
                    ensure_selection_visible(state);
                }
            }
        },
        Action::CommitInput => {
            let mode = state.input_mode;
            if matches!(mode, InputMode::ThreadQueueAdd | InputMode::ThreadQueueEdit) {
                let text = state.input_buffer.trim().to_string();
                if text.is_empty() {
                    return vec![];
                }
                let Some(thread_id) = state.current_thread_id().cloned() else {
                    return vec![];
                };
                let mutation = if mode == InputMode::ThreadQueueAdd {
                    ThreadQueueMutation::add(thread_id, text, now_unix_ms())
                } else {
                    let Some(id) = state
                        .selected_thread_queue_submission()
                        .map(|submission| submission.id.clone())
                    else {
                        return vec![];
                    };
                    ThreadQueueMutation::update(thread_id, id, text)
                };
                match mutation {
                    Ok(mutation) => {
                        state.input_mode = InputMode::Normal;
                        state.input_buffer.clear();
                        state.thread_queue_loading = true;
                        state.thread_queue_error = None;
                        return vec![Effect::MutateThreadQueue(mutation)];
                    }
                    Err(error) => {
                        state.thread_queue_error = Some(error.to_string());
                        return vec![];
                    }
                }
            }
            if mode == InputMode::TranscriptSearch {
                let query = state.input_buffer.trim().to_string();
                if query.is_empty() {
                    return vec![];
                }
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                state.transcript_search_open = true;
                state.transcript_search_query.clone_from(&query);
                state.transcript_search_results = None;
                state.transcript_search_selected = 0;
                state.transcript_search_loading = true;
                state.transcript_search_error = None;
                return vec![Effect::SearchTranscript { query }];
            }
            if mode == InputMode::ForgeMergeRequestTitle {
                let title = state.input_buffer.trim().to_string();
                if title.is_empty() {
                    return vec![];
                }
                let Some(target) = state.current_forge_mutation_target() else {
                    state.mutation_notice = Some(
                        local_text(
                            state.language,
                            "forge mutation target is unavailable",
                            "Forge 变更目标不可用",
                        )
                        .into(),
                    );
                    return vec![];
                };
                let Some(target_branch) = target.identity.default_branch.clone() else {
                    state.mutation_notice = Some(
                        local_text(
                            state.language,
                            "forge default branch is unavailable; create change-request plan refused",
                            "Forge 默认分支不可用；已拒绝创建变更请求计划",
                        )
                        .into(),
                    );
                    return vec![];
                };
                match ForgeMutationPlan::create_merge_request(
                    &target.identity,
                    target.cwd,
                    target.branch,
                    target_branch,
                    title,
                    now_unix_ms(),
                ) {
                    Ok(plan) => {
                        state.pending_forge_operation = Some(plan);
                        state.pending_forge_payload = None;
                        state.input_mode = InputMode::Normal;
                        state.input_buffer.clear();
                        state.mutation_notice = None;
                    }
                    Err(error) => {
                        state.mutation_notice = Some(format!(
                            "{}: {error:#}",
                            local_text(
                                state.language,
                                "cannot create forge mutation plan",
                                "无法创建 Forge 变更计划",
                            )
                        ));
                    }
                }
                return vec![];
            }
            if mode == InputMode::ForgeComment {
                let body = state.input_buffer.trim().to_string();
                if body.is_empty() {
                    return vec![];
                }
                let Some(target) = state.current_forge_mutation_target() else {
                    state.mutation_notice = Some(
                        local_text(
                            state.language,
                            "forge mutation target is unavailable",
                            "Forge 变更目标不可用",
                        )
                        .into(),
                    );
                    return vec![];
                };
                let Some(change) = target.change_request else {
                    state.mutation_notice = Some(
                        local_text(
                            state.language,
                            "current branch has no open merge request",
                            "当前分支没有打开的合并请求",
                        )
                        .into(),
                    );
                    return vec![];
                };
                match ForgeMutationPlan::comment_merge_request(
                    &target.identity,
                    target.cwd,
                    change.iid,
                    change.source_branch,
                    change.target_branch,
                    body.len(),
                    now_unix_ms(),
                ) {
                    Ok(plan) => {
                        state.pending_forge_operation = Some(plan);
                        state.pending_forge_payload = Some(body);
                        state.input_mode = InputMode::Normal;
                        state.input_buffer.clear();
                        state.mutation_notice = None;
                    }
                    Err(error) => {
                        state.mutation_notice = Some(format!(
                            "{}: {error:#}",
                            local_text(
                                state.language,
                                "cannot create forge mutation plan",
                                "无法创建 Forge 变更计划",
                            )
                        ));
                    }
                }
                return vec![];
            }
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
                    state.mutation_notice = Some(
                        local_text(
                            state.language,
                            "worktree path must be absolute before a plan can be created",
                            "创建计划前 worktree 路径必须是绝对路径",
                        )
                        .into(),
                    );
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
            if matches!(
                mode,
                InputMode::BatchAddTag
                    | InputMode::BatchRemoveTag
                    | InputMode::BatchPriority
                    | InputMode::BatchSnooze
            ) {
                let raw = state.input_buffer.trim().to_string();
                if raw.is_empty() {
                    return vec![];
                }
                let action = match mode {
                    InputMode::BatchAddTag => LocalBatchAction::AddTag(raw),
                    InputMode::BatchRemoveTag => LocalBatchAction::RemoveTag(raw),
                    InputMode::BatchPriority => match parse_priority(&raw) {
                        Ok(priority) => LocalBatchAction::SetPriority(priority),
                        Err(error) => {
                            state.mutation_notice = Some(format!(
                                "{}: {error:#}",
                                local_text(
                                    state.language,
                                    "invalid local batch priority",
                                    "本地批量优先级无效",
                                )
                            ));
                            return vec![];
                        }
                    },
                    InputMode::BatchSnooze => {
                        let Some(duration_ms) = parse_snooze_duration(&raw) else {
                            state.mutation_notice = Some(
                                local_text(
                                    state.language,
                                    "invalid batch snooze; use examples such as 15m, 1h, 1d",
                                    "批量稍后提醒无效；请使用 15m、1h、1d 等格式",
                                )
                                .into(),
                            );
                            return vec![];
                        };
                        LocalBatchAction::SnoozeUntil(Some(
                            now_unix_ms().saturating_add(duration_ms),
                        ))
                    }
                    _ => {
                        state.mutation_notice = Some(
                            local_text(
                                state.language,
                                "batch input routing mismatch; no write executed",
                                "批量输入路由不匹配；未执行写入",
                            )
                            .into(),
                        );
                        return vec![];
                    }
                };
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                state.freeze_visible_batch(action);
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
            if mode == InputMode::SavedViewField {
                let value = state.input_buffer.clone();
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                let Some(editor) = state.saved_view_editor.as_mut() else {
                    return vec![];
                };
                if let Err(error) = editor.commit_text_value(value) {
                    state.saved_view_editor_error = Some(error);
                } else {
                    state.saved_view_editor_error = None;
                }
                return vec![];
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
                return prompt::submit(state);
            }
            if mode == InputMode::Alias {
                let alias = state.input_buffer.trim().to_string();
                if let Some(thread) = state.threads.get_mut(state.selected) {
                    thread.alias = (!alias.is_empty()).then_some(alias);
                    if let Some(alias) = thread.alias.clone() {
                        state.local_aliases.insert(thread.id.0.clone(), alias);
                    } else {
                        state.local_aliases.remove(&thread.id.0);
                    }
                    state.input_mode = InputMode::Normal;
                    state.input_buffer.clear();
                    state.input_original.clear();
                    return vec![Effect::PersistOperatorState];
                }
            }
            let was_search = mode == InputMode::Search;
            state.input_mode = InputMode::Normal;
            state.input_buffer.clear();
            state.input_original.clear();
            if was_search {
                let origin_thread_id = state.search_origin_thread_id();
                state.search_return_view = None;
                let mut effects = refresh_git_projections(state);
                if let Some(thread_id) = origin_thread_id {
                    effects.push(Effect::StopWatchingConversation(thread_id));
                }
                return effects;
            }
        }
        Action::CancelInput => {
            let mut search_watch_to_release = None;
            if matches!(
                state.input_mode,
                InputMode::ThreadQueueAdd | InputMode::ThreadQueueEdit
            ) {
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![];
            }
            if state.input_mode == InputMode::TranscriptSearch {
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![];
            }
            if matches!(
                state.input_mode,
                InputMode::BatchAddTag
                    | InputMode::BatchRemoveTag
                    | InputMode::BatchPriority
                    | InputMode::BatchSnooze
            ) {
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![];
            }
            if matches!(
                state.input_mode,
                InputMode::ForgeMergeRequestTitle | InputMode::ForgeComment
            ) {
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![];
            }
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
            if state.input_mode == InputMode::SavedViewField {
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![];
            }
            if state.input_mode == InputMode::Search {
                let origin_thread_id = state.search_origin_thread_id();
                state.filter.clone_from(&state.input_original);
                ensure_selection_visible(state);
                if let Some(return_view) = state.search_return_view.take() {
                    if state.search_return_view_is_valid(&return_view) {
                        state.view = return_view;
                    } else {
                        search_watch_to_release = origin_thread_id;
                    }
                }
            }
            if state.input_mode == InputMode::UserInput {
                clear_user_input_editor(state);
            } else {
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                state.input_original.clear();
            }
            if let Some(thread_id) = search_watch_to_release {
                return vec![Effect::StopWatchingConversation(thread_id)];
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
    let (number, suffix) = value.split_at_checked(value.len() - 1)?;
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

const FORGE_REFRESH_TTL_MS: u64 = 60_000;

fn forge_scope_key(state: &AppState, thread_id: &ThreadId, cwd: &str) -> String {
    if let Some(identity) = state
        .forge_observations
        .get(&thread_id.0)
        .and_then(|observation| observation.identity.as_ref())
    {
        format!(
            "forge:{}:{}:{}",
            identity.provider.label(),
            identity.host,
            identity.project_id
        )
    } else {
        format!("cwd:{cwd}")
    }
}

fn refresh_forge_projections(state: &mut AppState) -> Vec<Effect> {
    let now = now_unix_ms();
    let mut groups = BTreeMap::<String, Vec<(ThreadId, String)>>::new();

    for context in state.git_contexts.values() {
        if !context.is_repository || context.error.is_some() {
            continue;
        }
        let Some(thread) = state.thread_by_id(&context.thread_id) else {
            continue;
        };
        if context.cwd != thread.metadata.cwd {
            continue;
        }
        groups
            .entry(forge_scope_key(state, &context.thread_id, &context.cwd))
            .or_default()
            .push((context.thread_id.clone(), context.cwd.clone()));
    }

    let mut effects = Vec::new();
    for targets in groups.into_values() {
        let newest = targets
            .iter()
            .filter_map(|(thread_id, cwd)| {
                state
                    .forge_observations
                    .get(&thread_id.0)
                    .filter(|observation| observation.cwd == *cwd)
            })
            .max_by_key(|observation| observation.observed_at_unix_ms);

        if newest.is_some_and(|observation| {
            observation.observed_at_unix_ms == 0
                || now.saturating_sub(observation.observed_at_unix_ms) <= FORGE_REFRESH_TTL_MS
        }) {
            continue;
        }

        let Some((thread_id, cwd)) = targets.first().cloned() else {
            continue;
        };
        state.forge_observations.insert(
            thread_id.0.clone(),
            ForgeObservation::pending(thread_id.clone(), cwd.clone()),
        );
        effects.push(Effect::ProbeForge { thread_id, cwd });
    }

    effects
}

fn git_projection_target_ids(state: &AppState) -> Vec<ThreadId> {
    let mut target_ids = Vec::new();
    let mut seen = BTreeSet::new();
    let visible = state.visible_indices();

    for index in visible.into_iter().take(REGISTRY_RECENT_LIMIT) {
        let thread = &state.threads[index];
        if seen.insert(thread.id.0.clone()) {
            target_ids.push(thread.id.clone());
        }
    }

    let selected_thread_id = match &state.view {
        View::Registry => state.selected_thread_id(),
        View::Thread(id) | View::Review(id) | View::Workspace(id) | View::ManagedWorktrees(id) => {
            Some(id.clone())
        }
        View::Board => state.selected_planning_card().and_then(|card| {
            (card.anchor.kind == SourceKind::CodexThread)
                .then(|| ThreadId::new(card.anchor.value.clone()))
        }),
        View::Scratch(_) => None,
    };
    if let Some(thread_id) = selected_thread_id
        && seen.insert(thread_id.0.clone())
    {
        target_ids.push(thread_id);
    }

    for thread in &state.threads {
        if matches!(
            thread.runtime,
            RuntimeStatus::Working | RuntimeStatus::WaitingHuman
        ) && seen.insert(thread.id.0.clone())
        {
            target_ids.push(thread.id.clone());
        }
    }

    target_ids
}

fn refresh_git_projections(state: &mut AppState) -> Vec<Effect> {
    state.reconcile_cwd_locality_cache(state.host_local_only || state.repo_backed_only);

    let thread_cwds = state
        .threads
        .iter()
        .map(|thread| (thread.id.0.clone(), thread.metadata.cwd.clone()))
        .collect::<BTreeMap<_, _>>();
    state.git_contexts.retain(|thread_id, context| {
        thread_cwds
            .get(thread_id)
            .is_some_and(|cwd| *cwd == context.cwd)
    });
    state.forge_observations.retain(|thread_id, observation| {
        thread_cwds
            .get(thread_id)
            .is_some_and(|cwd| *cwd == observation.cwd)
    });

    let target_ids = git_projection_target_ids(state);
    let mut threads_by_cwd = BTreeMap::<String, Vec<ThreadId>>::new();
    for thread_id in target_ids {
        let Some(cwd) = thread_cwds.get(&thread_id.0).cloned() else {
            continue;
        };
        threads_by_cwd.entry(cwd).or_default().push(thread_id);
    }

    let mut effects = Vec::new();
    for (cwd, thread_ids) in threads_by_cwd {
        if !state.refresh_cwd_locality(&cwd).terminal_usable() {
            for thread_id in &thread_ids {
                state.git_contexts.remove(&thread_id.0);
                state.forge_observations.remove(&thread_id.0);
            }
            continue;
        }

        let existing = thread_ids.iter().find_map(|thread_id| {
            state
                .git_contexts
                .get(&thread_id.0)
                .filter(|context| context.cwd == cwd)
                .cloned()
        });

        if let Some(existing) = existing {
            for thread_id in thread_ids {
                let needs_projection = state
                    .git_contexts
                    .get(&thread_id.0)
                    .is_none_or(|context| context.cwd != cwd);
                if needs_projection {
                    let mut projection = existing.clone();
                    projection.thread_id = thread_id.clone();
                    state.git_contexts.insert(thread_id.0, projection);
                }
            }
            continue;
        }

        let Some(leader) = thread_ids.first().cloned() else {
            continue;
        };
        for thread_id in thread_ids {
            state.git_contexts.insert(
                thread_id.0.clone(),
                GitContext::pending(thread_id, cwd.clone()),
            );
        }
        effects.push(Effect::ProbeGit {
            thread_id: leader,
            cwd,
        });
    }

    effects
}

fn refresh_active_git_projections(state: &mut AppState) -> Vec<Effect> {
    let selected_thread_id = match &state.view {
        View::Registry => state.selected_thread_id(),
        View::Thread(id) | View::Review(id) | View::Workspace(id) | View::ManagedWorktrees(id) => {
            Some(id.clone())
        }
        View::Board => state.selected_planning_card().and_then(|card| {
            (card.anchor.kind == SourceKind::CodexThread)
                .then(|| ThreadId::new(card.anchor.value.clone()))
        }),
        View::Scratch(_) => None,
    };

    let mut seen_ids = BTreeSet::new();
    let mut threads_by_cwd = BTreeMap::<String, Vec<ThreadId>>::new();
    if let Some(thread_id) = selected_thread_id
        && let Some(thread) = state.thread_by_id(&thread_id)
        && seen_ids.insert(thread.id.0.clone())
    {
        threads_by_cwd
            .entry(thread.metadata.cwd.clone())
            .or_default()
            .push(thread.id.clone());
    }

    for thread in &state.threads {
        if matches!(
            thread.runtime,
            RuntimeStatus::Working | RuntimeStatus::WaitingHuman
        ) && seen_ids.insert(thread.id.0.clone())
        {
            threads_by_cwd
                .entry(thread.metadata.cwd.clone())
                .or_default()
                .push(thread.id.clone());
        }
    }

    let mut effects = Vec::new();
    for (cwd, thread_ids) in threads_by_cwd {
        if state.refresh_cwd_locality(&cwd).terminal_usable() {
            let probe_pending = thread_ids.iter().any(|thread_id| {
                state
                    .git_contexts
                    .get(&thread_id.0)
                    .is_some_and(|context| context.cwd == cwd && context.observed_at_unix_ms == 0)
            });
            if probe_pending {
                continue;
            }

            if let Some(thread_id) = thread_ids.first() {
                let has_projection = thread_ids.iter().any(|candidate| {
                    state
                        .git_contexts
                        .get(&candidate.0)
                        .is_some_and(|context| context.cwd == cwd)
                });
                if !has_projection {
                    state.git_contexts.insert(
                        thread_id.0.clone(),
                        GitContext::pending(thread_id.clone(), cwd.clone()),
                    );
                }
                effects.push(Effect::ProbeGit {
                    thread_id: thread_id.clone(),
                    cwd,
                });
            }
        } else {
            for thread_id in thread_ids {
                state.git_contexts.remove(&thread_id.0);
                state.forge_observations.remove(&thread_id.0);
            }
        }
    }

    effects
}

fn propagate_git_context(state: &mut AppState, context: GitContext) {
    let target_ids = state.thread_ids_for_cwd(&context.cwd);

    for thread_id in target_ids {
        let mut projection = context.clone();
        projection.thread_id = thread_id.clone();
        state.git_contexts.insert(thread_id.0, projection);
    }
}

fn propagate_forge_observation(state: &mut AppState, observation: ForgeObservation) {
    let source_cwd = observation.cwd.clone();
    let source_identity = observation.identity.clone();
    let mut targets = state
        .git_contexts
        .values()
        .filter_map(|context| {
            if !context.is_repository || context.error.is_some() {
                return None;
            }
            let thread = state.thread_by_id(&context.thread_id)?;
            if context.cwd != thread.metadata.cwd {
                return None;
            }
            if context.cwd == source_cwd {
                return Some((context.thread_id.clone(), context.cwd.clone()));
            }
            source_identity.as_ref().and_then(|identity| {
                (state
                    .forge_observations
                    .get(&context.thread_id.0)
                    .and_then(|existing| existing.identity.as_ref())
                    == Some(identity))
                .then(|| (context.thread_id.clone(), context.cwd.clone()))
            })
        })
        .collect::<Vec<_>>();

    if targets.is_empty() {
        targets.push((observation.thread_id.clone(), source_cwd));
    }

    for (thread_id, cwd) in targets {
        let existing_review = state
            .forge_observations
            .get(&thread_id.0)
            .and_then(|existing| existing.review.clone())
            .filter(|review| {
                review.cwd == cwd
                    && observation
                        .change_requests
                        .iter()
                        .any(|change| change.iid == review.change_request_iid)
            });

        let mut projected = observation.clone();
        projected.thread_id = thread_id.clone();
        projected.cwd = cwd;
        projected.review = existing_review;
        if let Some(review) = &projected.review {
            projected.capabilities.insert(
                ForgeCapability::ApprovalSummary,
                if review.approvals_available {
                    CapabilityState::Available
                } else {
                    CapabilityState::Unavailable
                },
            );
            projected.capabilities.insert(
                ForgeCapability::Discussions,
                if review.discussions_available {
                    CapabilityState::Available
                } else {
                    CapabilityState::Unavailable
                },
            );
        }
        state
            .forge_observations
            .insert(thread_id.0.clone(), projected);
    }
}

fn active_worktree_counts(state: &AppState) -> BTreeMap<(LocalRepoIdentity, String), usize> {
    let mut counts = BTreeMap::new();
    for context in state.git_contexts.values() {
        let Some(thread) = state.thread_by_id(&context.thread_id) else {
            continue;
        };
        if context.cwd != thread.metadata.cwd
            || !matches!(
                thread.runtime,
                RuntimeStatus::Working | RuntimeStatus::WaitingHuman
            )
        {
            continue;
        }
        let Some(worktree) = context.worktree.as_ref() else {
            continue;
        };
        *counts
            .entry((worktree.repo.clone(), worktree.canonical_path.clone()))
            .or_default() += 1;
    }
    counts
}

fn collision_count_from_active_worktrees(
    state: &AppState,
    thread: &ThreadSummary,
    active_counts: &BTreeMap<(LocalRepoIdentity, String), usize>,
) -> usize {
    let Some(worktree) = state
        .git_context(&thread.id)
        .and_then(|context| context.worktree.as_ref())
    else {
        return 0;
    };
    let active = active_counts
        .get(&(worktree.repo.clone(), worktree.canonical_path.clone()))
        .copied()
        .unwrap_or(0);
    if matches!(
        thread.runtime,
        RuntimeStatus::Working | RuntimeStatus::WaitingHuman
    ) {
        active.saturating_sub(1)
    } else {
        active
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PlanningReconcilePhaseTimings {
    pub(crate) setup_ms: f64,
    pub(crate) thread_projection_ms: f64,
    pub(crate) supplemental_projection_ms: f64,
    pub(crate) sort_ms: f64,
    pub(crate) collision_index_ms: f64,
    pub(crate) work_card_index_ms: f64,
    pub(crate) work_cards_commit_ms: f64,
    pub(crate) index_commit_ms: f64,
    pub(crate) rebuild_total_ms: f64,
    pub(crate) selection_refresh_ms: f64,
    pub(crate) total_ms: f64,
}

#[inline(always)]
fn planning_phase_start<const PROFILE: bool>() -> Option<Instant> {
    if PROFILE { Some(Instant::now()) } else { None }
}

#[inline(always)]
fn planning_phase_elapsed<const PROFILE: bool>(started: Option<Instant>) -> f64 {
    if PROFILE {
        started
            .expect("profiled planning phase start")
            .elapsed()
            .as_secs_f64()
            * 1000.0
    } else {
        0.0
    }
}

fn rebuild_planning(state: &mut AppState, now_unix_ms: u64) {
    let _ = rebuild_planning_inner::<false>(state, now_unix_ms);
}

pub(crate) fn profile_planning_reconcile(
    state: &mut AppState,
    now_unix_ms: u64,
) -> PlanningReconcilePhaseTimings {
    let total_started = Instant::now();
    let mut timings = rebuild_planning_inner::<true>(state, now_unix_ms);

    let selection_started = Instant::now();
    ensure_selection_visible(state);
    timings.selection_refresh_ms = selection_started.elapsed().as_secs_f64() * 1000.0;
    timings.total_ms = total_started.elapsed().as_secs_f64() * 1000.0;
    timings
}

fn rebuild_planning_inner<const PROFILE: bool>(
    state: &mut AppState,
    now_unix_ms: u64,
) -> PlanningReconcilePhaseTimings {
    let selection = planning_selection::PlanningSelection::capture(state);
    state.planning_reconcile_count = state.planning_reconcile_count.saturating_add(1);
    let rebuild_started = planning_phase_start::<PROFILE>();
    let setup_started = planning_phase_start::<PROFILE>();

    let local_by_anchor = state
        .planning_snapshot
        .cards
        .iter()
        .map(|card| (card.anchor.clone(), card))
        .collect::<BTreeMap<_, _>>();
    let notes_by_owner = state
        .planning_snapshot
        .notes
        .iter()
        .map(|note| (note.owner.clone(), note))
        .collect::<BTreeMap<_, _>>();

    let mut projections =
        Vec::with_capacity(state.threads.len() + state.planning_snapshot.scratch.len());
    let active_worktrees = active_worktree_counts(state);
    let mut worktree_collision_counts = Vec::with_capacity(state.threads.len());

    let setup_ms = planning_phase_elapsed::<PROFILE>(setup_started);
    let thread_projection_started = planning_phase_start::<PROFILE>();

    for thread in &state.threads {
        let anchor = SourceRef::codex_thread(&thread.id);
        let collision_count =
            collision_count_from_active_worktrees(state, thread, &active_worktrees);
        worktree_collision_counts.push((thread.id.0.clone(), collision_count));
        let projection = reconcile_thread_card_with_goal_and_forge(
            ReconcileInput {
                thread,
                git: state.git_context(&thread.id),
                local: local_by_anchor.get(&anchor).copied(),
                collision_count,
                backend_observed_at_unix_ms: state.backend_status.last_refresh_unix_ms,
                backend_error: state.backend_status.error.as_deref(),
                now_unix_ms,
            },
            state.goals.get(&thread.id.0),
            state.forge_observation(&thread.id),
        );
        projections.push(projection);
    }

    let thread_projection_ms = planning_phase_elapsed::<PROFILE>(thread_projection_started);
    let supplemental_projection_started = planning_phase_start::<PROFILE>();

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

    let mut forge_issues = BTreeMap::new();
    for observation in state.forge_observations.values() {
        if observation.observed_at_unix_ms == 0 || observation.identity.is_none() {
            continue;
        }
        for issue in &observation.issues {
            let Some(anchor) = forge_issue_source_ref(observation, issue) else {
                continue;
            };
            let candidate = (observation.observed_at_unix_ms, observation, issue);
            match forge_issues.get(&anchor) {
                Some((observed_at, _, _)) if *observed_at >= observation.observed_at_unix_ms => {}
                _ => {
                    forge_issues.insert(anchor, candidate);
                }
            }
        }
    }

    for (anchor, (_, observation, issue)) in forge_issues {
        if let Some(projection) = reconcile_forge_issue_card(
            observation,
            issue,
            local_by_anchor.get(&anchor).copied(),
            now_unix_ms,
        ) {
            projections.push(projection);
        }
    }

    for projection in &mut projections {
        if let Some(note) = notes_by_owner.get(&projection.anchor) {
            projection.overlay.note = Some(note.text.clone());
        }
    }

    let supplemental_projection_ms =
        planning_phase_elapsed::<PROFILE>(supplemental_projection_started);
    let sort_started = planning_phase_start::<PROFILE>();

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

    let sort_ms = planning_phase_elapsed::<PROFILE>(sort_started);
    let index_commit_started = planning_phase_start::<PROFILE>();

    let collision_index_started = planning_phase_start::<PROFILE>();
    state.worktree_collision_counts = worktree_collision_counts.into_iter().collect();
    let collision_index_ms = planning_phase_elapsed::<PROFILE>(collision_index_started);

    let work_card_index_started = planning_phase_start::<PROFILE>();
    state.work_card_by_thread = projections
        .iter()
        .enumerate()
        .filter(|(_, card)| card.anchor.kind == SourceKind::CodexThread)
        .map(|(index, card)| (card.anchor.value.clone(), index))
        .collect();
    let work_card_index_ms = planning_phase_elapsed::<PROFILE>(work_card_index_started);

    let work_cards_commit_started = planning_phase_start::<PROFILE>();
    state.work_cards = projections;
    selection.restore(state);
    let work_cards_commit_ms = planning_phase_elapsed::<PROFILE>(work_cards_commit_started);

    let index_commit_ms = planning_phase_elapsed::<PROFILE>(index_commit_started);
    let rebuild_total_ms = planning_phase_elapsed::<PROFILE>(rebuild_started);

    PlanningReconcilePhaseTimings {
        setup_ms,
        thread_projection_ms,
        supplemental_projection_ms,
        sort_ms,
        collision_index_ms,
        work_card_index_ms,
        work_cards_commit_ms,
        index_commit_ms,
        rebuild_total_ms,
        selection_refresh_ms: 0.0,
        total_ms: rebuild_total_ms,
    }
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

fn normalize_pasted_input(mode: InputMode, value: &str) -> String {
    if matches!(
        mode,
        InputMode::Composer
            | InputMode::UserInput
            | InputMode::Note
            | InputMode::GoalObjective
            | InputMode::ForgeComment
    ) {
        let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
        normalized
            .chars()
            .map(|ch| {
                if matches!(ch, '\n' | '\t') || !ch.is_control() {
                    ch
                } else {
                    '\u{fffd}'
                }
            })
            .collect()
    } else {
        sanitize_inline(value)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::ForgeProviderKind;

    #[test]
    fn production_reducer_routes_do_not_panic_on_declared_unreachable_states() {
        let source = include_str!("app.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        assert!(
            !production.contains("unreachable!()"),
            "user-driven reducer routing must fail closed instead of panicking"
        );
    }
    use crate::backend::{CodexBackend, FakeBackend};

    fn app() -> AppState {
        AppState::new(FakeBackend::seeded().snapshot().threads)
    }

    #[test]
    fn simplified_chinese_local_notice_follows_ui_language() {
        let mut app = app();
        app.language = UiLanguage::SimplifiedChinese;
        let thread_id = app.threads[0].id.clone();
        app.view = View::Thread(thread_id);

        assert!(reduce(&mut app, Action::OpenManagedWorktrees).is_empty());
        assert_eq!(
            app.mutation_notice.as_deref(),
            Some("当前会话不在 Git 仓库中")
        );
    }

    #[test]
    fn global_search_enters_registry_and_cancel_restores_origin_view() {
        let mut app = app();
        let thread_id = app.threads[0].id.clone();
        app.view = View::Thread(thread_id.clone());
        app.filter = "existing".into();

        reduce(&mut app, Action::BeginSearch);
        assert_eq!(app.view, View::Registry);
        assert_eq!(app.input_mode, InputMode::Search);
        assert_eq!(app.input_buffer, "existing");

        reduce(&mut app, Action::InputText(" audio".into()));
        assert_eq!(app.filter, "existing audio");

        reduce(&mut app, Action::CancelInput);
        assert_eq!(app.view, View::Thread(thread_id));
        assert_eq!(app.filter, "existing");
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.search_return_view.is_none());
    }

    #[test]
    fn global_search_cancel_falls_back_to_registry_when_origin_thread_disappears() {
        let mut app = app();
        let thread_id = app.threads[0].id.clone();
        app.view = View::Workspace(thread_id.clone());
        app.filter = "existing".into();

        reduce(&mut app, Action::BeginSearch);
        let remaining = app
            .threads
            .iter()
            .filter(|thread| thread.id != thread_id)
            .cloned()
            .collect();
        reduce(&mut app, Action::ReplaceThreads(remaining));

        let effects = reduce(&mut app, Action::CancelInput);
        assert_eq!(app.view, View::Registry);
        assert_eq!(app.filter, "existing");
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.search_return_view.is_none());
        assert_eq!(effects, vec![Effect::StopWatchingConversation(thread_id)]);
    }

    #[test]
    fn committed_global_search_stays_in_registry_with_filter_and_releases_watch() {
        let mut app = app();
        let thread_id = app.threads[0].id.clone();
        app.view = View::Thread(thread_id.clone());

        reduce(&mut app, Action::BeginSearch);
        reduce(&mut app, Action::InputText("audio".into()));
        let effects = reduce(&mut app, Action::CommitInput);

        assert_eq!(app.view, View::Registry);
        assert_eq!(app.filter, "audio");
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.search_return_view.is_none());
        assert!(
            effects
                .iter()
                .any(|effect| effect == &Effect::StopWatchingConversation(thread_id.clone()))
        );
        assert!(effects.iter().all(|effect| {
            matches!(
                effect,
                Effect::ProbeGit { .. } | Effect::StopWatchingConversation(_)
            )
        }));
    }

    #[test]
    fn unicode_search_alias_and_composer_input_are_lossless() {
        let mut app = app();
        app.threads[0].title = "音频管线回归".into();

        reduce(&mut app, Action::BeginSearch);
        for character in "音频".chars() {
            reduce(&mut app, Action::InputChar(character));
        }
        assert_eq!(app.visible_indices(), vec![0]);
        reduce(&mut app, Action::CancelInput);

        app.selected = 0;
        app.threads[0].alias = None;
        reduce(&mut app, Action::BeginAlias);
        for character in "主会话".chars() {
            reduce(&mut app, Action::InputChar(character));
        }
        reduce(&mut app, Action::CommitInput);
        assert_eq!(app.threads[0].alias.as_deref(), Some("主会话"));

        reduce(&mut app, Action::OpenSelected);
        reduce(&mut app, Action::QuickPrompt);
        for character in "继续检查".chars() {
            reduce(&mut app, Action::InputChar(character));
        }
        let thread_id = app.current_thread_id().expect("thread");
        assert_eq!(
            app.thread_ui.get(&thread_id.0).expect("thread ui").draft,
            "继续检查"
        );
    }

    #[test]
    fn terminal_drawer_open_focus_and_close_are_explicit_effects() {
        let mut app = app();
        let cwd = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned();
        app.threads[0].metadata.cwd = cwd.clone();

        let effects = reduce(&mut app, Action::ToggleTerminalDrawer);
        assert_eq!(effects, vec![Effect::OpenTerminalDrawer { cwd }]);
        assert!(app.terminal_drawer_open);
        assert!(app.terminal_focused);

        assert!(reduce(&mut app, Action::SetTerminalFocus(false)).is_empty());
        assert!(!app.terminal_focused);
        assert!(reduce(&mut app, Action::ToggleTerminalDrawer).is_empty());
        assert!(app.terminal_focused);

        let effects = reduce(&mut app, Action::CloseTerminalDrawer);
        assert_eq!(effects, vec![Effect::CloseTerminalDrawer]);
        assert!(!app.terminal_drawer_open);
        assert!(!app.terminal_focused);
        assert!(app.terminal_snapshot.is_none());
    }

    #[test]
    fn terminal_drawer_target_errors_follow_simplified_chinese_ui_language() {
        let mut app = app();
        app.language = UiLanguage::SimplifiedChinese;
        app.view = View::Scratch("scratch:1".into());

        let error = app
            .terminal_target_cwd()
            .expect_err("scratch has no Codex cwd");
        assert_eq!(error, "终端抽屉不可用：未选择 Codex 会话");
    }

    #[test]
    fn terminal_drawer_rejects_views_without_thread_cwd() {
        let mut app = app();
        app.view = View::Scratch("scratch:1".into());
        let effects = reduce(&mut app, Action::ToggleTerminalDrawer);
        assert!(effects.is_empty());
        assert!(!app.terminal_drawer_open);
        assert!(
            app.mutation_notice
                .as_deref()
                .is_some_and(|notice| notice.contains("no Codex thread is selected"))
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn terminal_drawer_rejects_foreign_windows_session_cwd() {
        let mut app = app();
        app.threads[0].metadata.cwd = r"/vsdata/leiwenjun/llm/codex-tui/C:\Users\jun\repo".into();

        let effects = reduce(&mut app, Action::ToggleTerminalDrawer);
        assert!(effects.is_empty());
        assert!(!app.terminal_drawer_open);
        assert!(
            app.mutation_notice
                .as_deref()
                .is_some_and(|notice| notice.contains("Windows cwd"))
        );
    }

    #[test]
    fn ordinary_registry_search_does_not_probe_filesystem_locality() {
        let mut app = AppState::new(FakeBackend::scaled(10_000).snapshot().threads);
        app.filter = "repo-031 synthetic 099".into();

        let _ = app.visible_indices_with_match_count();

        assert!(
            app.cwd_localities.is_empty(),
            "ordinary text search must not populate cwd filesystem locality"
        );
    }

    #[test]
    fn registry_host_local_toggle_composes_with_text_search() {
        let mut app = app();
        app.threads[0].workspace = "focus-repo".into();
        app.threads[0].metadata.cwd = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned();
        if app.threads.len() > 1 {
            app.threads[1].workspace = "focus-repo".into();
            app.threads[1].metadata.cwd = if cfg!(windows) {
                "/foreign/unix/repo".into()
            } else {
                r"C:\Users\jun\repo".into()
            };
        }
        app.filter = "focus-repo".into();

        reduce(&mut app, Action::ToggleHostLocalFilter);
        assert!(app.host_local_only);
        let visible = app.visible_indices();
        assert_eq!(visible, vec![0]);
        assert_eq!(app.filter, "focus-repo");

        reduce(&mut app, Action::ToggleHostLocalFilter);
        assert!(!app.host_local_only);
        let visible = app.visible_indices();
        assert!(visible.contains(&0));
        if app.threads.len() > 1 {
            assert!(visible.contains(&1));
        }
        assert_eq!(app.filter, "focus-repo");
    }

    #[test]
    fn successful_git_context_backfills_display_locality_without_render_io() {
        let mut app = app();
        let cwd = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned();
        app.threads[0].metadata.cwd.clone_from(&cwd);
        assert_eq!(app.cwd_locality_for_display(&cwd), None);

        let thread_id = app.threads[0].id.clone();
        let mut context = GitContext::pending(thread_id, cwd.clone());
        context.observed_at_unix_ms = 1;
        reduce(&mut app, Action::GitContextLoaded(context));

        assert_eq!(
            app.cwd_locality_for_display(&cwd),
            Some(CwdLocality::LocalDirectory)
        );
    }

    #[test]
    fn registry_locality_cache_is_refreshed_by_active_git_reconcile() {
        let root = tempfile::tempdir().expect("tempdir");
        let cwd = root.path().join("repo");
        std::fs::create_dir_all(&cwd).expect("create cwd");
        let cwd = cwd.to_string_lossy().into_owned();

        let mut app = app();
        app.threads[0].metadata.cwd.clone_from(&cwd);
        app.selected = 0;

        reduce(&mut app, Action::ToggleHostLocalFilter);
        assert_eq!(app.cwd_locality(&cwd), CwdLocality::LocalDirectory);
        assert!(app.visible_indices().contains(&0));

        std::fs::remove_dir(&cwd).expect("remove cwd");
        assert_eq!(
            app.cwd_locality(&cwd),
            CwdLocality::LocalDirectory,
            "render-time lookup must reuse the cached filesystem classification"
        );

        reduce(&mut app, Action::RefreshActiveGitProjections);
        assert_eq!(app.cwd_locality(&cwd), CwdLocality::NativeMissing);
        assert!(!app.visible_indices().contains(&0));
    }

    #[test]
    fn registry_repo_only_filter_keeps_unresolved_and_degraded_candidates() {
        let cwd = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned();
        let mut app = app();
        for thread in app.threads.iter_mut().take(3) {
            thread.metadata.cwd.clone_from(&cwd);
        }
        app.threads[3].metadata.cwd = if cfg!(windows) {
            "/foreign/unix/repo".into()
        } else {
            r"C:\Users\jun\repo".into()
        };

        let repo_id = app.threads[0].id.clone();
        let mut repo = GitContext::pending(repo_id.clone(), cwd.clone());
        repo.observed_at_unix_ms = 1;
        repo.is_repository = true;
        app.git_contexts.insert(repo_id.0.clone(), repo);

        let nonrepo_id = app.threads[1].id.clone();
        let mut nonrepo = GitContext::pending(nonrepo_id.clone(), cwd.clone());
        nonrepo.observed_at_unix_ms = 1;
        app.git_contexts.insert(nonrepo_id.0.clone(), nonrepo);

        let degraded_id = app.threads[2].id.clone();
        let mut degraded = GitContext::pending(degraded_id.clone(), cwd.clone());
        degraded.observed_at_unix_ms = 1;
        degraded.error = Some("git unavailable".into());
        app.git_contexts.insert(degraded_id.0.clone(), degraded);

        let effects = reduce(&mut app, Action::ToggleRepoBackedFilter);
        assert_eq!(effects, vec![Effect::PersistOperatorState]);
        assert!(app.repo_backed_only);
        assert_eq!(app.visible_indices(), vec![0, 2]);

        app.git_contexts.remove(&degraded_id.0);
        assert_eq!(app.visible_indices(), vec![0, 2]);

        app.git_contexts.insert(
            degraded_id.0.clone(),
            GitContext::pending(degraded_id.clone(), cwd.clone()),
        );
        assert_eq!(app.visible_indices(), vec![0, 2]);

        let context = app.git_contexts.get_mut(&degraded_id.0).expect("context");
        context.observed_at_unix_ms = 1;
        assert_eq!(app.visible_indices(), vec![0]);

        reduce(&mut app, Action::ToggleRepoBackedFilter);
        assert!(!app.repo_backed_only);
        assert_eq!(app.visible_indices(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn registry_recent_window_is_bounded_but_search_and_pins_reach_full_history() {
        let mut app = AppState::new(FakeBackend::scaled(150).snapshot().threads);
        app.threads[149].pinned = true;

        let (visible, matched_count) = app.visible_indices_with_match_count();
        assert_eq!(matched_count, 150);
        assert!(visible.len() > REGISTRY_RECENT_LIMIT);
        assert!(visible.len() < 150);
        assert!(visible.contains(&149));
        assert!(visible.iter().any(|index| *index >= REGISTRY_RECENT_LIMIT));

        app.selected = 149;
        let effects = reduce(&mut app, Action::TogglePin);
        assert_eq!(effects, vec![Effect::PersistOperatorState]);
        assert!(!app.visible_indices().contains(&149));
        assert_ne!(app.selected, 149);

        app.filter = "Synthetic work item 00149".into();
        assert_eq!(app.visible_indices(), vec![149]);

        app.filter.clear();
        reduce(&mut app, Action::ToggleAllHistory);
        assert!(app.show_all_history);
        assert_eq!(app.visible_indices().len(), 150);
    }

    #[test]
    fn registry_search_can_filter_host_local_sessions() {
        let mut app = app();
        app.threads[0].metadata.cwd = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned();
        if app.threads.len() > 1 {
            app.threads[1].title = "local-looking foreign history".into();
            app.threads[1].metadata.cwd = if cfg!(windows) {
                "/foreign/unix/repo".into()
            } else {
                r"C:\Users\jun\repo".into()
            };
        }

        app.filter = "local".into();
        let visible = app.visible_indices();
        assert!(visible.contains(&0));
        if app.threads.len() > 1 {
            assert!(!visible.contains(&1));
        }
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
    fn replace_threads_preserves_local_overlays_and_invalidates_changed_attention() {
        let mut app = app();
        let thread_id = app.threads[0].id.clone();
        let mut local = LocalStateV1::default();
        local.pins.insert(thread_id.0.clone());
        local.aliases.insert(thread_id.0.clone(), "primary".into());
        local.marked_unread.insert(thread_id.0.clone());
        local.acknowledged_attention.insert(thread_id.0.clone());
        app.apply_local_state(&local);

        let mut fresh = app.threads.clone();
        let refreshed = fresh
            .iter_mut()
            .find(|thread| thread.id == thread_id)
            .expect("fresh thread");
        refreshed.pinned = false;
        refreshed.alias = None;
        refreshed.attention = vec![AttentionReason::ApprovalRequired];

        reduce(&mut app, Action::ReplaceThreads(fresh));

        let refreshed = app
            .threads
            .iter()
            .find(|thread| thread.id == thread_id)
            .expect("refreshed thread");
        assert!(refreshed.pinned);
        assert_eq!(refreshed.alias.as_deref(), Some("primary"));
        assert!(
            refreshed
                .attention
                .contains(&AttentionReason::ApprovalRequired)
        );
        assert!(refreshed.attention.contains(&AttentionReason::MarkedUnread));
        assert!(!app.acknowledged_attention.contains(&thread_id.0));
    }

    #[test]
    fn durable_local_overlays_survive_partial_then_full_registry_hydration() {
        let full = FakeBackend::scaled(3).snapshot().threads;
        let late = full[2].id.clone();
        let mut app = AppState::new(full[..1].to_vec());

        let mut local = LocalStateV1::default();
        local.pins.insert(late.0.clone());
        local.aliases.insert(late.0.clone(), "late-history".into());
        local.marked_unread.insert(late.0.clone());
        app.apply_local_state(&local);

        assert!(app.to_local_state().pins.contains(&late.0));
        reduce(&mut app, Action::ReplaceThreads(full));

        let hydrated = app
            .threads
            .iter()
            .find(|thread| thread.id == late)
            .expect("hydrated thread");
        assert!(hydrated.pinned);
        assert_eq!(hydrated.alias.as_deref(), Some("late-history"));
        assert!(hydrated.attention.contains(&AttentionReason::MarkedUnread));
    }

    #[test]
    fn replace_threads_scales_to_10k_and_preserves_tail_overlay() {
        let mut app = AppState::new(FakeBackend::scaled(10_000).snapshot().threads);
        let tail_id = app.threads.last().expect("tail thread").id.clone();
        let mut local = LocalStateV1::default();
        local.pins.insert(tail_id.0.clone());
        local.aliases.insert(tail_id.0.clone(), "tail".into());
        app.apply_local_state(&local);

        let fresh = FakeBackend::scaled(10_000).snapshot().threads;
        reduce(&mut app, Action::ReplaceThreads(fresh));

        let tail = app
            .threads
            .iter()
            .find(|thread| thread.id == tail_id)
            .expect("tail after replacement");
        assert!(tail.pinned);
        assert_eq!(tail.alias.as_deref(), Some("tail"));
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
    fn launch_preset_requires_load_select_plan_and_confirmation() {
        let mut app = app();
        let thread_id = app.threads[0].id.clone();
        app.view = View::Workspace(thread_id.clone());
        app.git_contexts.insert(
            thread_id.0.clone(),
            GitContext {
                thread_id,
                cwd: "/repo/subdir".into(),
                is_repository: true,
                repo: Some(crate::domain::LocalRepoIdentity {
                    git_common_dir: "/repo/.git".into(),
                    primary_root: "/repo".into(),
                }),
                worktree: None,
                head: None,
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

        reduce(&mut app, Action::OpenContext);
        let launch_index = app
            .context_choices()
            .iter()
            .position(|choice| *choice == ContextChoice::LaunchPreset)
            .expect("launch context");
        app.context_selected = launch_index;
        let effects = reduce(&mut app, Action::ExecuteContext);
        assert_eq!(
            effects,
            vec![Effect::LoadLaunchPresets {
                repo_root: "/repo".into(),
                thread_cwd: "/repo/subdir".into(),
            }]
        );

        let preset = LaunchPreset {
            name: "Tests".into(),
            argv: vec!["cargo".into(), "test".into()],
            cwd: crate::launch::LaunchCwd::Repo,
        };
        reduce(
            &mut app,
            Action::LaunchPresetsLoaded {
                repo_root: "/repo".into(),
                thread_cwd: "/repo/subdir".into(),
                presets: vec![preset.clone()],
            },
        );
        assert!(app.launch_menu_open);
        let effects = reduce(&mut app, Action::SelectLaunchPreset);
        assert_eq!(
            effects,
            vec![Effect::PrepareLaunchPreset {
                preset,
                repo_root: "/repo".into(),
                thread_cwd: "/repo/subdir".into(),
            }]
        );

        let plan = LaunchPlan {
            name: "Tests".into(),
            argv: vec!["cargo".into(), "test".into()],
            cwd: std::path::PathBuf::from("/repo"),
            config_path: std::path::PathBuf::from("/repo/.codex-tui.toml"),
        };
        reduce(&mut app, Action::LaunchPlanPrepared(plan.clone()));
        assert!(app.pending_launch_plan.is_some());
        let effects = reduce(&mut app, Action::ConfirmPendingOperation);
        assert_eq!(effects, vec![Effect::ExecuteLaunchPreset(Box::new(plan))]);
        assert!(app.pending_launch_plan.is_none());
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
    fn pending_request_thread_index_preserves_multi_request_attention() {
        let mut app = app();
        let thread_id = app.threads[0].id.clone();
        app.threads[0].attention.clear();

        for request_id in [101_i64, 102_i64] {
            reduce(
                &mut app,
                Action::InteractiveRequested(InteractiveRequest {
                    request_id: RpcRequestId::Integer(request_id),
                    thread_id: thread_id.clone(),
                    turn_id: "turn-1".into(),
                    item_id: format!("item-{request_id}"),
                    kind: InteractiveRequestKind::FileChangeApproval { reason: None },
                }),
            );
        }

        assert_eq!(app.pending_requests.len(), 2);
        assert_eq!(app.pending_request_threads.len(), 1);
        assert!(app.pending_request_threads.contains(&thread_id.0));
        assert!(app.thread_needs_attention(0));

        reduce(
            &mut app,
            Action::InteractiveResolved {
                request_id: RpcRequestId::Integer(101),
            },
        );
        assert!(app.pending_request_threads.contains(&thread_id.0));
        assert!(app.thread_needs_attention(0));

        reduce(
            &mut app,
            Action::InteractiveResolved {
                request_id: RpcRequestId::Integer(102),
            },
        );
        assert!(!app.pending_request_threads.contains(&thread_id.0));
        assert!(!app.thread_needs_attention(0));
    }

    #[test]
    fn pending_request_thread_index_tracks_request_retargeting() {
        let mut app = app();
        for thread in &mut app.threads {
            thread.attention.clear();
        }
        let first = app.threads[0].id.clone();
        let second = app.threads[1].id.clone();

        reduce(
            &mut app,
            Action::InteractiveRequested(InteractiveRequest {
                request_id: RpcRequestId::Integer(201),
                thread_id: first.clone(),
                turn_id: "turn-1".into(),
                item_id: "item-1".into(),
                kind: InteractiveRequestKind::FileChangeApproval { reason: None },
            }),
        );
        reduce(
            &mut app,
            Action::InteractiveRequested(InteractiveRequest {
                request_id: RpcRequestId::Integer(201),
                thread_id: second.clone(),
                turn_id: "turn-2".into(),
                item_id: "item-2".into(),
                kind: InteractiveRequestKind::FileChangeApproval { reason: None },
            }),
        );

        assert_eq!(app.pending_requests.len(), 1);
        assert!(!app.pending_request_threads.contains(&first.0));
        assert!(app.pending_request_threads.contains(&second.0));
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
    fn git_review_cache_is_lru_bounded() {
        let mut app = AppState::new(
            FakeBackend::scaled(GIT_REVIEW_CACHE_LIMIT + 1)
                .snapshot()
                .threads,
        );
        let ids = app
            .threads
            .iter()
            .map(|thread| thread.id.clone())
            .collect::<Vec<_>>();
        for (index, thread) in app.threads.iter_mut().enumerate() {
            thread.metadata.cwd = format!("/repo/{index}");
        }

        for index in 0..GIT_REVIEW_CACHE_LIMIT {
            app.view = View::Registry;
            app.selected = index;
            assert!(
                reduce(&mut app, Action::OpenReview)
                    .iter()
                    .any(|effect| matches!(effect, Effect::LoadGitReview { .. }))
            );
        }
        assert_eq!(app.git_reviews.len(), GIT_REVIEW_CACHE_LIMIT);

        app.view = View::Registry;
        app.selected = 0;
        reduce(&mut app, Action::OpenReview);

        app.view = View::Registry;
        app.selected = GIT_REVIEW_CACHE_LIMIT;
        reduce(&mut app, Action::OpenReview);

        assert_eq!(app.git_reviews.len(), GIT_REVIEW_CACHE_LIMIT);
        assert!(app.git_reviews.contains_key(&ids[0].0));
        assert!(!app.git_reviews.contains_key(&ids[1].0));
        assert!(app.git_reviews.contains_key(&ids[GIT_REVIEW_CACHE_LIMIT].0));
    }

    #[test]
    fn git_review_cache_never_evicts_current_review_on_background_load() {
        let mut app = AppState::new(
            FakeBackend::scaled(GIT_REVIEW_CACHE_LIMIT + 1)
                .snapshot()
                .threads,
        );
        // Cached and incoming snapshots belong to the live thread cwd.
        for (index, thread) in app.threads.iter_mut().enumerate() {
            thread.metadata.cwd = if index == GIT_REVIEW_CACHE_LIMIT {
                "/repo/new"
            } else {
                "/repo"
            }
            .into();
        }
        let ids = app
            .threads
            .iter()
            .map(|thread| thread.id.clone())
            .collect::<Vec<_>>();

        for thread_id in ids.iter().take(GIT_REVIEW_CACHE_LIMIT) {
            app.git_reviews.insert(
                thread_id.0.clone(),
                GitReview::pending(thread_id.clone(), "/repo"),
            );
            app.touch_git_review_cache(thread_id);
        }
        app.view = View::Review(ids[0].clone());

        reduce(
            &mut app,
            Action::GitReviewLoaded(GitReview::pending(
                ids[GIT_REVIEW_CACHE_LIMIT].clone(),
                "/repo/new",
            )),
        );

        assert_eq!(app.git_reviews.len(), GIT_REVIEW_CACHE_LIMIT);
        assert!(app.git_reviews.contains_key(&ids[0].0));
        assert!(app.git_reviews.contains_key(&ids[GIT_REVIEW_CACHE_LIMIT].0));
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
    fn batch_plan_freezes_visible_targets_before_confirmation() {
        let mut app = app();
        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 100 });
        reduce(&mut app, Action::OpenBoard);

        let before = app
            .visible_planning_cards()
            .iter()
            .map(|card| card.local_id.clone())
            .collect::<Vec<_>>();
        assert!(!before.is_empty());

        reduce(&mut app, Action::OpenContext);
        let index = app
            .context_choices()
            .iter()
            .position(|choice| *choice == ContextChoice::BatchMarkDone)
            .expect("batch mark-done");
        app.context_selected = index;
        assert!(reduce(&mut app, Action::ExecuteContext).is_empty());

        let frozen = app.pending_local_batch.clone().expect("frozen batch plan");
        assert_eq!(
            frozen
                .targets
                .iter()
                .map(|target| target.local_id.clone())
                .collect::<Vec<_>>(),
            before
        );

        reduce(&mut app, Action::CycleSavedView(1));
        assert_ne!(
            app.visible_planning_cards()
                .iter()
                .map(|card| card.local_id.clone())
                .collect::<Vec<_>>(),
            before
        );

        let effects = reduce(&mut app, Action::ConfirmPendingOperation);
        assert!(matches!(
            effects.as_slice(),
            [Effect::ApplyLocalBatch(plan)] if plan.targets == frozen.targets
        ));
        assert!(app.pending_local_batch.is_none());
    }

    #[test]
    fn batch_input_creates_plan_before_any_write_effect() {
        let mut app = app();
        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 100 });
        reduce(&mut app, Action::OpenBoard);
        reduce(&mut app, Action::OpenContext);
        let index = app
            .context_choices()
            .iter()
            .position(|choice| *choice == ContextChoice::BatchAddTag)
            .expect("batch tag");
        app.context_selected = index;
        assert!(reduce(&mut app, Action::ExecuteContext).is_empty());
        assert_eq!(app.input_mode, InputMode::BatchAddTag);

        for ch in "focus".chars() {
            reduce(&mut app, Action::InputChar(ch));
        }
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        let plan = app.pending_local_batch.as_ref().expect("batch preview");
        assert_eq!(plan.action, LocalBatchAction::AddTag("focus".into()));
        assert!(!plan.targets.is_empty());

        let effects = reduce(&mut app, Action::ConfirmPendingOperation);
        assert!(matches!(
            effects.as_slice(),
            [Effect::ApplyLocalBatch(plan)] if plan.action == LocalBatchAction::AddTag("focus".into())
        ));
    }

    #[test]
    fn cancelling_batch_preview_emits_no_write_effect() {
        let mut app = app();
        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 100 });
        reduce(&mut app, Action::OpenBoard);
        app.freeze_visible_batch(LocalBatchAction::ClearPriority);
        assert!(app.pending_local_batch.is_some());

        let effects = reduce(&mut app, Action::CancelPendingOperation);
        assert!(effects.is_empty());
        assert!(app.pending_local_batch.is_none());
        assert_eq!(
            app.mutation_notice.as_deref(),
            Some("operation cancelled before execution")
        );
    }

    #[test]
    fn board_context_creates_structured_copy_and_keeps_builtin_immutable() {
        let mut app = app();
        reduce(&mut app, Action::OpenBoard);
        let choices = app.context_choices();
        assert!(choices.contains(&ContextChoice::SaveCurrentView));
        assert!(!choices.contains(&ContextChoice::EditCurrentView));
        assert!(!choices.contains(&ContextChoice::DeleteCurrentView));

        reduce(&mut app, Action::OpenContext);
        let save_index = app
            .context_choices()
            .iter()
            .position(|choice| *choice == ContextChoice::SaveCurrentView)
            .expect("save view action");
        app.context_selected = save_index;
        reduce(&mut app, Action::ExecuteContext);

        let editor = app.saved_view_editor.as_ref().expect("structured editor");
        assert!(editor.creating);
        assert!(editor.draft.id.is_empty());
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn saved_view_editor_validates_before_write_and_custom_views_are_editable() {
        let mut app = app();
        reduce(&mut app, Action::OpenBoard);
        app.saved_view_editor = Some(SavedViewEditor::create_from(&app.active_saved_view()));
        app.saved_view_editor.as_mut().expect("editor").draft.filter = "unknown:value".into();

        assert!(reduce(&mut app, Action::SaveSavedViewEditor).is_empty());
        assert!(app.saved_view_editor.is_some());
        assert!(app.saved_view_editor_error.is_some());

        let editor = app.saved_view_editor.as_mut().expect("editor");
        editor.draft.filter = "stage:review".into();
        editor.draft.name = "Review only".into();
        let effects = reduce(&mut app, Action::SaveSavedViewEditor);
        assert!(matches!(
            effects.as_slice(),
            [Effect::SaveSavedView { view }]
                if view.name == "Review only" && view.filter == "stage:review"
        ));

        app.planning_snapshot.saved_views.push(SavedView {
            id: "view:7".into(),
            name: "Custom".into(),
            source_scope: "all".into(),
            filter: String::new(),
            group_by: Some("workspace".into()),
            order_by: Some("priority".into()),
            layout: SavedViewLayout::List,
            visible_fields: vec!["stage".into()],
        });
        app.planning_view_index = builtin_saved_views().len();
        let choices = app.context_choices();
        assert!(choices.contains(&ContextChoice::EditCurrentView));
        assert!(choices.contains(&ContextChoice::DeleteCurrentView));
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
    fn command_palette_is_contextual_and_wraps_selection() {
        let mut app = app();
        let registry_choices = app.command_palette_choices();
        assert!(
            registry_choices
                .iter()
                .copied()
                .all(Command::palette_capable),
            "palette must be a projection of palette-capable Command values"
        );
        assert!(registry_choices.contains(&Command::Search));
        assert!(registry_choices.contains(&Command::NextAttention));
        assert!(registry_choices.contains(&Command::QuickPrompt));
        assert!(registry_choices.contains(&Command::Board));
        assert!(registry_choices.contains(&Command::Review));
        assert!(registry_choices.contains(&Command::Workspace));
        assert!(registry_choices.contains(&Command::TogglePin));
        assert!(registry_choices.contains(&Command::Help));

        reduce(&mut app, Action::OpenCommandPalette);
        assert!(app.command_palette_open);
        assert_eq!(app.command_palette_selected, 0);

        reduce(&mut app, Action::MoveCommandPalette(-1));
        assert_eq!(
            app.command_palette_selected,
            app.command_palette_choices().len() - 1
        );

        reduce(&mut app, Action::CloseCommandPalette);
        assert!(!app.command_palette_open);
        assert_eq!(app.command_palette_selected, 0);

        app.view = View::Scratch("scratch:missing".into());
        let scratch_choices = app.command_palette_choices();
        assert!(scratch_choices.contains(&Command::Board));
        assert!(!scratch_choices.contains(&Command::TerminalDrawer));
        assert!(!scratch_choices.contains(&Command::CloseTerminalDrawer));
    }

    #[test]
    fn command_palette_uses_thread_backed_board_selection_for_thread_commands() {
        let mut app = app();
        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1 });
        reduce(&mut app, Action::OpenBoard);

        let choices = app.command_palette_choices();
        assert!(choices.contains(&Command::QuickPrompt));
        assert!(choices.contains(&Command::Review));
        assert!(choices.contains(&Command::Workspace));
    }

    #[test]
    fn command_palette_freezes_choices_while_background_state_changes() {
        let mut app = app();
        reduce(&mut app, Action::OpenCommandPalette);

        let toggle_pin_index = app
            .command_palette_choices()
            .iter()
            .position(|choice| *choice == Command::TogglePin)
            .expect("TogglePin palette entry");
        app.command_palette_selected = toggle_pin_index;
        assert_eq!(app.command_palette_choice(), Some(Command::TogglePin));

        app.threads.clear();
        app.rebuild_thread_indexes();
        assert!(
            !app.build_command_palette_choices()
                .contains(&Command::TogglePin)
        );
        assert_eq!(
            app.command_palette_choice(),
            Some(Command::TogglePin),
            "background state changes must not retarget the highlighted command"
        );

        reduce(&mut app, Action::CloseCommandPalette);
        assert!(app.command_palette_items.is_empty());
        assert!(!app.command_palette_choices().contains(&Command::TogglePin));
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
    fn profiled_planning_reconcile_matches_normal_reducer_semantics() {
        let threads = FakeBackend::scaled(64).snapshot().threads;
        let mut normal = AppState::new(threads.clone());
        let mut profiled = AppState::new(threads);

        reduce(&mut normal, Action::ReconcilePlanning { now_unix_ms: 1 });
        let timings = profile_planning_reconcile(&mut profiled, 1);

        assert_eq!(profiled.work_cards, normal.work_cards);
        assert_eq!(profiled.work_card_by_thread, normal.work_card_by_thread);
        assert_eq!(
            profiled.worktree_collision_counts,
            normal.worktree_collision_counts
        );
        assert_eq!(profiled.selected, normal.selected);
        assert!(timings.total_ms >= timings.rebuild_total_ms);
        assert!(timings.rebuild_total_ms >= timings.thread_projection_ms);
    }

    #[test]
    fn planning_rebuild_indexes_thread_cards_and_preserves_collision_counts() {
        let mut app = app();
        let repo = LocalRepoIdentity {
            git_common_dir: "/repo/.git".into(),
            primary_root: "/repo".into(),
        };
        for thread in app.threads.iter_mut().take(2) {
            thread.runtime = RuntimeStatus::Working;
            thread.metadata.cwd = "/repo".into();
            let mut context = GitContext::pending(thread.id.clone(), "/repo");
            context.is_repository = true;
            context.repo = Some(repo.clone());
            context.worktree = Some(crate::domain::WorktreeIdentity {
                repo: repo.clone(),
                canonical_path: "/repo".into(),
                branch: Some("main".into()),
                managed_by_codex_tui: false,
            });
            app.git_contexts.insert(thread.id.0.clone(), context);
        }

        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1 });

        for thread in app.threads.iter().take(2) {
            let card = app.work_card_for_thread(&thread.id).expect("indexed card");
            assert!(
                card.attention
                    .contains(&crate::planning::PlanningAttention::ConflictRisk)
            );
            assert_eq!(app.worktree_collision_count(&thread.id), 1);
        }
        assert_eq!(app.work_card_by_thread.len(), app.threads.len());
    }

    #[test]
    fn thread_projection_indexes_rebuild_for_10k_registry_replacement() {
        let mut app = AppState::new(FakeBackend::scaled(10_000).snapshot().threads);
        assert_eq!(app.thread_index_by_id.len(), 10_000);

        let shared_cwd = "/shared/indexed/repo";
        let mut fresh = FakeBackend::scaled(10_000).snapshot().threads;
        for thread in fresh.iter_mut().take(1_000) {
            thread.metadata.cwd = shared_cwd.into();
        }
        let tail_id = fresh.last().expect("tail").id.clone();

        reduce(&mut app, Action::ReplaceThreads(fresh));

        assert_eq!(app.thread_index_by_id.len(), 10_000);
        assert_eq!(app.thread_ids_for_cwd(shared_cwd).len(), 1_000);
        assert_eq!(
            app.thread_by_id(&tail_id).map(|thread| &thread.id),
            Some(&tail_id)
        );
    }

    #[test]
    fn indexed_cwd_fanout_preserves_git_projection_semantics() {
        let mut app = app();
        let shared_cwd = "/shared/fanout";
        let mut fresh = app.threads.clone();
        fresh[0].metadata.cwd = shared_cwd.into();
        fresh[1].metadata.cwd = shared_cwd.into();
        reduce(&mut app, Action::ReplaceThreads(fresh));

        let source = app.threads[0].id.clone();
        let peer = app.threads[1].id.clone();
        let mut context = GitContext::pending(source, shared_cwd);
        context.observed_at_unix_ms = 1;
        context.is_repository = true;
        context.branch = Some("main".into());

        reduce(&mut app, Action::GitContextLoaded(context));

        for thread_id in [peer, app.threads[0].id.clone()] {
            let projected = app.git_context(&thread_id).expect("projected git context");
            assert_eq!(projected.cwd, shared_cwd);
            assert_eq!(projected.branch.as_deref(), Some("main"));
        }
    }

    #[test]
    fn conversation_cache_is_lru_bounded_without_evicting_thread_ui() {
        let mut app = AppState::new(
            FakeBackend::scaled(CONVERSATION_CACHE_LIMIT + 1)
                .snapshot()
                .threads,
        );
        let ids = app
            .threads
            .iter()
            .map(|thread| thread.id.clone())
            .collect::<Vec<_>>();

        for index in 0..CONVERSATION_CACHE_LIMIT {
            app.view = View::Registry;
            app.selected = index;
            assert!(matches!(
                reduce(&mut app, Action::OpenSelected).as_slice(),
                [Effect::LoadConversation(_)]
            ));
        }
        assert_eq!(app.conversations.len(), CONVERSATION_CACHE_LIMIT);

        app.view = View::Registry;
        app.selected = 0;
        reduce(&mut app, Action::OpenSelected);

        app.view = View::Registry;
        app.selected = CONVERSATION_CACHE_LIMIT;
        reduce(&mut app, Action::OpenSelected);

        assert_eq!(app.conversations.len(), CONVERSATION_CACHE_LIMIT);
        assert!(app.conversations.contains_key(&ids[0].0));
        assert!(!app.conversations.contains_key(&ids[1].0));
        assert!(
            app.conversations
                .contains_key(&ids[CONVERSATION_CACHE_LIMIT].0)
        );
        assert_eq!(
            app.thread_ui.len(),
            CONVERSATION_CACHE_LIMIT + 1,
            "resident conversation eviction must not discard per-thread local UI state"
        );
    }

    #[test]
    fn reconciled_10k_registry_keeps_recent_projection_bounded() {
        let mut app = AppState::new(FakeBackend::scaled(10_000).snapshot().threads);
        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1 });

        let (visible, matched) = app.visible_indices_with_match_count();
        assert_eq!(matched, 10_000);
        assert!(visible.len() >= REGISTRY_RECENT_LIMIT);
        assert!(visible.len() < 10_000);
        assert_eq!(app.work_card_by_thread.len(), 10_000);
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
    fn git_projection_eager_work_is_bounded_by_recent_registry_scope() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut app = AppState::new(FakeBackend::scaled(150).snapshot().threads);
        for (index, thread) in app.threads.iter_mut().enumerate() {
            thread.runtime = RuntimeStatus::Inactive;
            let cwd = root.path().join(format!("repo-{index:03}"));
            std::fs::create_dir_all(&cwd).expect("create cwd");
            thread.metadata.cwd = cwd.to_string_lossy().into_owned();
        }

        let effects = reduce(&mut app, Action::RefreshGitProjections);
        assert_eq!(effects.len(), REGISTRY_RECENT_LIMIT);
        assert_eq!(app.git_contexts.len(), REGISTRY_RECENT_LIMIT);
        assert_eq!(
            app.cwd_localities.len(),
            REGISTRY_RECENT_LIMIT,
            "Git projection must not stat every historical cwd"
        );

        app.filter = "00149".into();
        let effects = reduce(&mut app, Action::RefreshGitProjections);
        assert_eq!(effects.len(), 1);
        assert!(app.git_contexts.contains_key("thread-scale-00149"));
    }

    #[test]
    fn git_projection_probe_is_emitted_once_until_cwd_changes() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut app = app();
        for (index, thread) in app.threads.iter_mut().enumerate() {
            let cwd = root.path().join(format!("repo-{index}"));
            std::fs::create_dir_all(&cwd).expect("create cwd");
            thread.metadata.cwd = cwd.to_string_lossy().into_owned();
        }
        let effects = reduce(&mut app, Action::RefreshGitProjections);
        assert_eq!(effects.len(), 4);
        assert!(reduce(&mut app, Action::RefreshGitProjections).is_empty());

        let next = root.path().join("new-cwd");
        std::fs::create_dir_all(&next).expect("create changed cwd");
        let next = next.to_string_lossy().into_owned();
        app.threads[0].metadata.cwd.clone_from(&next);
        let effects = reduce(&mut app, Action::RefreshGitProjections);
        assert_eq!(
            effects,
            vec![Effect::ProbeGit {
                thread_id: ThreadId::new("thread-impl"),
                cwd: next,
            }]
        );
    }

    #[test]
    fn git_projection_deduplicates_shared_cwd_and_fans_out_result() {
        let root = tempfile::tempdir().expect("tempdir");
        let cwd = root.path().join("shared-repo");
        std::fs::create_dir_all(&cwd).expect("create cwd");
        let cwd = cwd.to_string_lossy().into_owned();

        let mut app = app();
        for thread in &mut app.threads {
            thread.metadata.cwd.clone_from(&cwd);
        }

        let effects = reduce(&mut app, Action::RefreshGitProjections);
        assert_eq!(effects.len(), 1);
        let Effect::ProbeGit {
            thread_id: leader,
            cwd: probed_cwd,
        } = effects[0].clone()
        else {
            panic!("expected one git probe");
        };
        assert_eq!(probed_cwd, cwd);
        assert_eq!(app.git_contexts.len(), app.threads.len());
        assert!(
            app.git_contexts
                .values()
                .all(|context| context.observed_at_unix_ms == 0)
        );

        let mut loaded = GitContext::pending(leader, cwd.clone());
        loaded.observed_at_unix_ms = 1;
        loaded.is_repository = true;
        loaded.branch = Some("main".into());
        reduce(&mut app, Action::GitContextLoaded(loaded));

        for thread in &app.threads {
            let context = app.git_context(&thread.id).expect("projected git context");
            assert_eq!(context.thread_id, thread.id);
            assert_eq!(context.cwd, cwd);
            assert_eq!(context.branch.as_deref(), Some("main"));
            assert_eq!(context.observed_at_unix_ms, 1);
        }
        assert!(reduce(&mut app, Action::RefreshGitProjections).is_empty());
    }

    #[test]
    fn active_git_refresh_is_selected_first_and_deduplicated_by_cwd() {
        let root = tempfile::tempdir().expect("tempdir");
        let shared = root.path().join("shared");
        let inactive = root.path().join("inactive");
        std::fs::create_dir_all(&shared).expect("create shared cwd");
        std::fs::create_dir_all(&inactive).expect("create inactive cwd");
        let shared = shared.to_string_lossy().into_owned();
        let inactive = inactive.to_string_lossy().into_owned();

        let mut app = app();
        app.threads[0].runtime = RuntimeStatus::Working;
        app.threads[0].metadata.cwd.clone_from(&shared);
        app.threads[1].runtime = RuntimeStatus::WaitingHuman;
        app.threads[1].metadata.cwd.clone_from(&shared);
        app.threads[2].runtime = RuntimeStatus::Inactive;
        app.threads[2].metadata.cwd.clone_from(&inactive);
        app.threads[3].runtime = RuntimeStatus::Inactive;
        app.threads[3].metadata.cwd.clone_from(&inactive);
        app.selected = 2;

        let effects = reduce(&mut app, Action::RefreshActiveGitProjections);
        assert_eq!(
            effects,
            vec![
                Effect::ProbeGit {
                    thread_id: app.threads[2].id.clone(),
                    cwd: inactive,
                },
                Effect::ProbeGit {
                    thread_id: app.threads[0].id.clone(),
                    cwd: shared,
                },
            ]
        );
        assert!(
            reduce(&mut app, Action::RefreshActiveGitProjections).is_empty(),
            "pending active probes must not be enqueued again"
        );
    }

    #[test]
    fn git_projection_skips_nonlocal_cwds_and_drops_old_projection() {
        let root = tempfile::tempdir().expect("tempdir");
        let local = root.path().join("local");
        std::fs::create_dir_all(&local).expect("create local cwd");

        let mut app = app();
        app.threads[0].metadata.cwd = local.to_string_lossy().into_owned();
        app.threads[1].metadata.cwd = if cfg!(windows) {
            "/foreign/unix/repo".into()
        } else {
            r"C:\Users\jun\repo".into()
        };
        app.threads[2].metadata.cwd = "relative/repo".into();
        app.threads[3].metadata.cwd = root.path().join("missing").to_string_lossy().into_owned();

        for thread in app.threads.iter().skip(1) {
            app.git_contexts.insert(
                thread.id.0.clone(),
                GitContext::pending(thread.id.clone(), thread.metadata.cwd.clone()),
            );
        }
        let foreign = app.threads[1].clone();
        app.forge_observations.insert(
            foreign.id.0.clone(),
            ForgeObservation::pending(foreign.id.clone(), foreign.metadata.cwd.clone()),
        );

        let effects = reduce(&mut app, Action::RefreshGitProjections);
        assert_eq!(
            effects,
            vec![Effect::ProbeGit {
                thread_id: ThreadId::new("thread-impl"),
                cwd: app.threads[0].metadata.cwd.clone(),
            }]
        );
        assert_eq!(
            app.git_contexts.keys().cloned().collect::<Vec<_>>(),
            vec!["thread-impl".to_string()]
        );
        assert!(app.forge_observations.is_empty());
    }

    #[test]
    fn planning_deduplicates_same_gitlab_issue_across_threads() {
        use crate::forge::{
            CapabilityState, ForgeCapability, ForgeFreshness, ForgeIdentity, ForgeIssueSummary,
            ForgeProviderKind,
        };

        let mut app = app();
        let issue = ForgeIssueSummary {
            iid: 12,
            title: "Shared issue".into(),
            state: "opened".into(),
            web_url: "https://gitlab.example.com/team/repo/-/issues/12".into(),
            updated_at: Some("2026-09-30T00:00:00Z".into()),
        };

        for (index, thread) in app.threads.iter().take(2).enumerate() {
            app.forge_observations.insert(
                thread.id.0.clone(),
                ForgeObservation {
                    thread_id: thread.id.clone(),
                    cwd: thread.metadata.cwd.clone(),
                    remote_name: Some("origin".into()),
                    remote_url: Some("git@gitlab.example.com:team/repo.git".into()),
                    identity: Some(ForgeIdentity {
                        provider: ForgeProviderKind::GitLab,
                        host: "gitlab.example.com".into(),
                        project_id: "42".into(),
                        path_with_namespace: "team/repo".into(),
                        web_url: "https://gitlab.example.com/team/repo".into(),
                        default_branch: Some("main".into()),
                    }),
                    capabilities: BTreeMap::from([(
                        ForgeCapability::Issues,
                        CapabilityState::Available,
                    )]),
                    issues: vec![issue.clone()],
                    change_requests: vec![],
                    pipelines: vec![],
                    review: None,
                    observed_at_unix_ms: 100 + index as u64,
                    freshness: ForgeFreshness::Fresh,
                    error: None,
                },
            );
        }

        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 102 });

        let issue_cards = app
            .work_cards
            .iter()
            .filter(|card| card.anchor.kind == SourceKind::ForgeWorkItem)
            .collect::<Vec<_>>();
        assert_eq!(issue_cards.len(), 1);
        assert_eq!(
            issue_cards[0].anchor.value,
            "gitlab://gitlab.example.com/projects/42/issues/12"
        );
    }

    #[test]
    fn forge_refresh_coalesces_same_checkout_and_reprobes_after_ttl() {
        use crate::forge::{ForgeFreshness, ForgeIdentity, ForgeProviderKind};

        let mut app = app();
        let ids = app
            .threads
            .iter()
            .take(2)
            .map(|thread| thread.id.clone())
            .collect::<Vec<_>>();
        for thread in app.threads.iter_mut().take(2) {
            thread.metadata.cwd = "/repo".into();
        }
        for thread_id in &ids {
            let mut context = GitContext::pending(thread_id.clone(), "/repo");
            context.is_repository = true;
            app.git_contexts.insert(thread_id.0.clone(), context);
        }

        let effects = reduce(&mut app, Action::RefreshForgeProjections);
        assert_eq!(effects.len(), 1);
        let Effect::ProbeForge {
            thread_id: source_thread,
            cwd,
        } = effects[0].clone()
        else {
            panic!("expected one forge probe");
        };
        assert_eq!(cwd, "/repo");

        reduce(
            &mut app,
            Action::ForgeObservationLoaded(ForgeObservation {
                thread_id: source_thread,
                cwd: "/repo".into(),
                remote_name: Some("origin".into()),
                remote_url: Some("git@gitlab.example.com:team/repo.git".into()),
                identity: Some(ForgeIdentity {
                    provider: ForgeProviderKind::GitLab,
                    host: "gitlab.example.com".into(),
                    project_id: "42".into(),
                    path_with_namespace: "team/repo".into(),
                    web_url: "https://gitlab.example.com/team/repo".into(),
                    default_branch: Some("main".into()),
                }),
                capabilities: BTreeMap::new(),
                issues: vec![],
                change_requests: vec![],
                pipelines: vec![],
                review: None,
                observed_at_unix_ms: now_unix_ms(),
                freshness: ForgeFreshness::Fresh,
                error: None,
            }),
        );

        assert!(ids.iter().all(|thread_id| {
            app.forge_observation(thread_id)
                .is_some_and(|observation| observation.identity.is_some())
        }));
        assert!(reduce(&mut app, Action::RefreshForgeProjections).is_empty());

        for thread_id in &ids {
            app.forge_observations
                .get_mut(&thread_id.0)
                .expect("forge observation")
                .observed_at_unix_ms = 1;
        }
        assert_eq!(reduce(&mut app, Action::RefreshForgeProjections).len(), 1);
    }

    #[test]
    fn forge_projection_probe_is_git_authoritative_and_deduplicated() {
        let mut app = app();
        app.threads[0].metadata.cwd = "/repo".into();
        let thread_id = app.threads[0].id.clone();

        let mut context = GitContext::pending(thread_id.clone(), "/repo");
        context.is_repository = true;
        reduce(&mut app, Action::GitContextLoaded(context));

        let effects = reduce(&mut app, Action::RefreshForgeProjections);
        assert_eq!(
            effects,
            vec![Effect::ProbeForge {
                thread_id: thread_id.clone(),
                cwd: "/repo".into(),
            }]
        );
        assert_eq!(
            app.forge_observation(&thread_id)
                .expect("pending forge observation")
                .observed_at_unix_ms,
            0
        );
        assert!(reduce(&mut app, Action::RefreshForgeProjections).is_empty());

        app.threads[0].metadata.cwd = "/new/repo".into();
        assert!(reduce(&mut app, Action::RefreshForgeProjections).is_empty());
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
    fn pasted_text_preserves_multiline_composer_and_sanitizes_single_line_search() {
        let mut app = app();
        reduce(&mut app, Action::OpenSelected);
        reduce(&mut app, Action::QuickPrompt);

        assert_eq!(
            reduce(&mut app, Action::InputText("line1\r\nline2\u{0007}".into())),
            vec![Effect::PersistOperatorStateDeferred]
        );
        let thread_id = app.current_thread_id().expect("thread");
        assert_eq!(
            app.thread_ui.get(&thread_id.0).expect("thread ui").draft,
            "line1\nline2\u{fffd}"
        );

        reduce(&mut app, Action::CancelInput);
        reduce(&mut app, Action::Back);
        reduce(&mut app, Action::BeginSearch);
        assert!(reduce(&mut app, Action::InputText("repo\nproject\tname".into())).is_empty());
        assert_eq!(app.filter, "repo project name");
    }

    #[test]
    fn high_frequency_thread_ui_edits_use_deferred_persistence() {
        let mut app = app();
        reduce(&mut app, Action::OpenSelected);
        reduce(&mut app, Action::QuickPrompt);
        assert_eq!(app.input_mode, InputMode::Composer);

        assert_eq!(
            reduce(&mut app, Action::InputChar('x')),
            vec![Effect::PersistOperatorStateDeferred]
        );
        assert_eq!(
            reduce(&mut app, Action::InputBackspace),
            vec![Effect::PersistOperatorStateDeferred]
        );
        assert_eq!(
            reduce(&mut app, Action::ScrollBy(1)),
            vec![Effect::PersistOperatorStateDeferred]
        );

        assert_eq!(
            reduce(&mut app, Action::ToggleFollow),
            vec![Effect::PersistOperatorState]
        );
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
        app.threads[0].metadata.cwd = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned();
        app.selected = 0;
        let effects = reduce(&mut app, Action::ToggleHostLocalFilter);
        assert_eq!(effects, vec![Effect::PersistOperatorState]);
        let local = app.to_local_state();

        let mut restored = AppState::new(FakeBackend::seeded().snapshot().threads);
        restored.threads[0].metadata.cwd = app.threads[0].metadata.cwd.clone();
        restored.apply_local_state(&local);
        assert!(restored.threads[1].pinned);
        assert_eq!(restored.threads[1].alias.as_deref(), Some("primary"));
        assert!(restored.acknowledged_attention.contains("thread-kws"));
        assert!(restored.host_local_only);
        assert!(!restored.repo_backed_only);
        assert_eq!(restored.visible_indices(), vec![0]);
    }
    fn seed_gitlab_mutation_target(app: &mut AppState, with_merge_request: bool) -> ThreadId {
        use crate::forge::{
            ChangeRequestSummary, ForgeFreshness, ForgeIdentity, ForgeObservation,
            ForgeProviderKind,
        };

        let thread_id = app.threads[0].id.clone();
        app.threads[0].metadata.cwd = "/repo".into();
        let mut git = GitContext::pending(thread_id.clone(), "/repo");
        git.is_repository = true;
        git.branch = Some("feature/m6b".into());
        git.observed_at_unix_ms = 100;
        app.git_contexts.insert(thread_id.0.clone(), git);

        let change_requests = with_merge_request
            .then(|| ChangeRequestSummary {
                iid: 7,
                title: "M6b".into(),
                state: "opened".into(),
                source_branch: "feature/m6b".into(),
                target_branch: "main".into(),
                web_url: "https://gitlab.example.com/team/repo/-/merge_requests/7".into(),
                updated_at: None,
                draft: false,
                detailed_merge_status: Some("mergeable".into()),
                blocking_discussions_resolved: Some(true),
            })
            .into_iter()
            .collect();

        app.forge_observations.insert(
            thread_id.0.clone(),
            ForgeObservation {
                thread_id: thread_id.clone(),
                cwd: "/repo".into(),
                remote_name: Some("origin".into()),
                remote_url: Some("git@gitlab.example.com:team/repo.git".into()),
                identity: Some(ForgeIdentity {
                    provider: ForgeProviderKind::GitLab,
                    host: "gitlab.example.com".into(),
                    project_id: "42".into(),
                    path_with_namespace: "team/repo".into(),
                    web_url: "https://gitlab.example.com/team/repo".into(),
                    default_branch: Some("main".into()),
                }),
                capabilities: BTreeMap::new(),
                issues: vec![],
                change_requests,
                pipelines: vec![],
                review: None,
                observed_at_unix_ms: 100,
                freshness: ForgeFreshness::Fresh,
                error: None,
            },
        );
        app.view = View::Workspace(thread_id.clone());
        thread_id
    }

    fn seed_github_mutation_target(app: &mut AppState, with_pull_request: bool) -> ThreadId {
        use crate::forge::{
            ChangeRequestSummary, ForgeFreshness, ForgeIdentity, ForgeObservation,
            ForgeProviderKind,
        };

        let thread_id = app.threads[0].id.clone();
        app.threads[0].metadata.cwd = "/repo".into();
        let mut git = GitContext::pending(thread_id.clone(), "/repo");
        git.is_repository = true;
        git.branch = Some("feature/github".into());
        git.observed_at_unix_ms = 100;
        app.git_contexts.insert(thread_id.0.clone(), git);

        let change_requests = with_pull_request
            .then(|| ChangeRequestSummary {
                iid: 9,
                title: "GitHub PR".into(),
                state: "open".into(),
                source_branch: "feature/github".into(),
                target_branch: "main".into(),
                web_url: "https://github.com/octo/repo/pull/9".into(),
                updated_at: None,
                draft: false,
                detailed_merge_status: None,
                blocking_discussions_resolved: None,
            })
            .into_iter()
            .collect();

        app.forge_observations.insert(
            thread_id.0.clone(),
            ForgeObservation {
                thread_id: thread_id.clone(),
                cwd: "/repo".into(),
                remote_name: Some("origin".into()),
                remote_url: Some("git@github.com:octo/repo.git".into()),
                identity: Some(ForgeIdentity {
                    provider: ForgeProviderKind::GitHub,
                    host: "github.com".into(),
                    project_id: "123".into(),
                    path_with_namespace: "octo/repo".into(),
                    web_url: "https://github.com/octo/repo".into(),
                    default_branch: Some("main".into()),
                }),
                capabilities: BTreeMap::new(),
                issues: vec![],
                change_requests,
                pipelines: vec![],
                review: None,
                observed_at_unix_ms: 100,
                freshness: ForgeFreshness::Fresh,
                error: None,
            },
        );
        app.view = View::Workspace(thread_id.clone());
        thread_id
    }

    #[test]
    fn github_pull_request_is_plan_only_until_explicit_confirmation() {
        let mut app = app();
        seed_github_mutation_target(&mut app, false);

        let choices = app.context_choices();
        assert!(choices.contains(&ContextChoice::ForgeCreateMergeRequest));
        app.context_selected = choices
            .iter()
            .position(|choice| *choice == ContextChoice::ForgeCreateMergeRequest)
            .expect("create GitHub PR choice");
        assert!(reduce(&mut app, Action::ExecuteContext).is_empty());

        for ch in "Ship GitHub mutations".chars() {
            reduce(&mut app, Action::InputChar(ch));
        }
        assert!(reduce(&mut app, Action::CommitInput).is_empty());

        let plan = app.pending_forge_operation.clone().expect("GitHub plan");
        assert_eq!(plan.provider, ForgeProviderKind::GitHub);
        assert_eq!(plan.source_branch.as_deref(), Some("feature/github"));
        assert!(plan.expected_side_effect.contains("GitHub pull request"));

        let effects = reduce(&mut app, Action::ConfirmPendingOperation);
        assert!(matches!(
            effects.as_slice(),
            [Effect::ExecuteForgeOperation(request)]
                if request.plan.provider == ForgeProviderKind::GitHub
        ));
    }

    #[test]
    fn github_existing_pull_exposes_comment_approve_and_merge_actions() {
        let mut app = app();
        seed_github_mutation_target(&mut app, true);
        let choices = app.context_choices();
        assert!(choices.contains(&ContextChoice::ForgeComment));
        assert!(choices.contains(&ContextChoice::ForgeApprove));
        assert!(choices.contains(&ContextChoice::ForgeMerge));
        assert!(!choices.contains(&ContextChoice::ForgeCreateMergeRequest));
    }

    #[test]
    fn create_merge_request_is_plan_only_until_explicit_confirmation() {
        let mut app = app();
        seed_gitlab_mutation_target(&mut app, false);

        let choices = app.context_choices();
        assert!(choices.contains(&ContextChoice::ForgeCreateMergeRequest));
        app.context_selected = choices
            .iter()
            .position(|choice| *choice == ContextChoice::ForgeCreateMergeRequest)
            .expect("create MR choice");
        assert!(reduce(&mut app, Action::ExecuteContext).is_empty());
        assert_eq!(app.input_mode, InputMode::ForgeMergeRequestTitle);

        for ch in "Ship M6b".chars() {
            assert!(reduce(&mut app, Action::InputChar(ch)).is_empty());
        }
        assert!(reduce(&mut app, Action::CommitInput).is_empty());

        let plan = app
            .pending_forge_operation
            .clone()
            .expect("pending forge plan");
        assert_eq!(
            plan.kind,
            crate::forge_mutation::ForgeMutationKind::CreateMergeRequest
        );
        assert_eq!(plan.source_branch.as_deref(), Some("feature/m6b"));
        assert_eq!(plan.target_branch.as_deref(), Some("main"));
        assert!(app.pending_forge_payload.is_none());

        let effects = reduce(&mut app, Action::ConfirmPendingOperation);
        let [Effect::ExecuteForgeOperation(request)] = effects.as_slice() else {
            panic!("expected explicit forge execution effect");
        };
        assert_eq!(request.plan, plan);
        assert!(request.payload.is_none());
        assert!(app.pending_forge_operation.is_none());
    }

    #[test]
    fn merge_request_comment_body_stays_memory_only_until_confirmation() {
        let mut app = app();
        seed_gitlab_mutation_target(&mut app, true);

        let choices = app.context_choices();
        assert!(choices.contains(&ContextChoice::ForgeComment));
        assert!(choices.contains(&ContextChoice::ForgeApprove));
        assert!(choices.contains(&ContextChoice::ForgeMerge));
        assert!(!choices.contains(&ContextChoice::ForgeCreateMergeRequest));

        app.context_selected = choices
            .iter()
            .position(|choice| *choice == ContextChoice::ForgeComment)
            .expect("comment choice");
        assert!(reduce(&mut app, Action::ExecuteContext).is_empty());
        assert_eq!(app.input_mode, InputMode::ForgeComment);

        let body = "please re-run the target DUT check";
        for ch in body.chars() {
            reduce(&mut app, Action::InputChar(ch));
        }
        assert!(reduce(&mut app, Action::CommitInput).is_empty());

        let plan = app.pending_forge_operation.as_ref().expect("comment plan");
        assert_eq!(
            plan.kind,
            crate::forge_mutation::ForgeMutationKind::CommentMergeRequest
        );
        assert_eq!(plan.payload_bytes, Some(body.len()));
        assert!(
            !serde_json::to_string(plan)
                .expect("serialize plan")
                .contains(body)
        );
        assert_eq!(app.pending_forge_payload.as_deref(), Some(body));

        let effects = reduce(&mut app, Action::ConfirmPendingOperation);
        let [Effect::ExecuteForgeOperation(request)] = effects.as_slice() else {
            panic!("expected comment execution effect");
        };
        assert_eq!(request.payload.as_deref(), Some(body));
        assert!(app.pending_forge_payload.is_none());
    }

    #[test]
    fn approve_and_merge_context_actions_only_create_pending_plans() {
        let mut app = app();
        seed_gitlab_mutation_target(&mut app, true);

        for (choice, expected_kind) in [
            (
                ContextChoice::ForgeApprove,
                crate::forge_mutation::ForgeMutationKind::ApproveMergeRequest,
            ),
            (
                ContextChoice::ForgeMerge,
                crate::forge_mutation::ForgeMutationKind::MergeMergeRequest,
            ),
        ] {
            let choices = app.context_choices();
            app.context_selected = choices
                .iter()
                .position(|candidate| *candidate == choice)
                .expect("forge action choice");
            assert!(reduce(&mut app, Action::ExecuteContext).is_empty());
            assert_eq!(
                app.pending_forge_operation
                    .as_ref()
                    .expect("pending plan")
                    .kind,
                expected_kind
            );
            assert!(reduce(&mut app, Action::CancelPendingOperation).is_empty());
            assert!(app.pending_forge_operation.is_none());
        }
    }

    #[test]
    fn forge_mutation_receipt_invalidates_projection_and_enters_recent_audit() {
        use crate::forge_mutation::{ForgeMutationPlan, ForgeMutationReceipt};

        let mut app = app();
        let thread_id = seed_gitlab_mutation_target(&mut app, true);
        let target = app.current_forge_mutation_target().expect("target");
        let change = target.change_request.expect("MR");
        let plan = ForgeMutationPlan::approve_merge_request(
            &target.identity,
            target.cwd,
            change.iid,
            change.source_branch,
            change.target_branch,
            10,
        )
        .expect("plan");
        let mut receipt = ForgeMutationReceipt::planned(plan);
        receipt.start(11);
        receipt.succeed(12, "mr:7".into(), "verified".into());

        reduce(
            &mut app,
            Action::ForgeMutationReceipt(Box::new(receipt.clone())),
        );
        assert_eq!(app.recent_forge_operations, vec![receipt]);
        let observation = app.forge_observation(&thread_id).expect("observation");
        assert_eq!(observation.observed_at_unix_ms, 1);
        assert!(observation.review.is_none());
    }
}

#[cfg(test)]
mod thread_queue_reducer_tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};
    use serde_json::json;

    fn queue_app() -> (AppState, ThreadId) {
        let threads = FakeBackend::seeded().snapshot().threads;
        let thread_id = threads[0].id.clone();
        let mut app = AppState::new(threads);
        app.view = View::Thread(thread_id.clone());
        (app, thread_id)
    }

    fn snapshot(thread_id: ThreadId) -> ThreadQueueSnapshot {
        ThreadQueueSnapshot {
            thread_id,
            submissions: vec![
                QueuedSubmission {
                    id: "q1".into(),
                    client_user_message_id: "c1".into(),
                    input: vec![json!({
                        "type": "text",
                        "text": "first",
                        "textElements": []
                    })],
                    summary: "first".into(),
                    editable_text: Some("first".into()),
                },
                QueuedSubmission {
                    id: "q2".into(),
                    client_user_message_id: "c2".into(),
                    input: vec![json!({"type": "localImage", "path": "/tmp/a.png"})],
                    summary: "[localImage]".into(),
                    editable_text: None,
                },
            ],
            next_cursor: None,
        }
    }

    #[test]
    fn queue_open_load_edit_and_close_stay_upstream_authoritative() {
        let (mut app, thread_id) = queue_app();
        assert_eq!(
            reduce(&mut app, Action::OpenThreadQueue),
            vec![Effect::RefreshThreadQueue(thread_id.clone())]
        );
        assert!(app.thread_queue_open);
        assert!(app.thread_queue_loading);

        reduce(
            &mut app,
            Action::ThreadQueueLoaded(snapshot(thread_id.clone())),
        );
        assert!(!app.thread_queue_loading);
        assert_eq!(app.selected_thread_queue_submission().unwrap().id, "q1");

        reduce(&mut app, Action::BeginThreadQueueEdit);
        assert_eq!(app.input_mode, InputMode::ThreadQueueEdit);
        assert_eq!(app.input_buffer, "first");
        app.input_buffer = "updated".into();
        let effects = reduce(&mut app, Action::CommitInput);
        assert!(matches!(
            effects.as_slice(),
            [Effect::MutateThreadQueue(ThreadQueueMutation::Update {
                queued_submission_id,
                text,
                ..
            })] if queued_submission_id == "q1" && text == "updated"
        ));

        app.input_mode = InputMode::Normal;
        app.thread_queue_selected = 1;
        reduce(&mut app, Action::BeginThreadQueueEdit);
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.mutation_notice.is_some());

        let effects = reduce(&mut app, Action::CloseThreadQueue);
        assert_eq!(effects, vec![Effect::StopWatchingThreadQueue(thread_id)]);
        assert!(!app.thread_queue_open);
    }

    #[test]
    fn queue_reorder_and_destructive_actions_follow_confirmation_contract() {
        let (mut app, thread_id) = queue_app();
        app.thread_queue_open = true;
        app.thread_queue_snapshot = Some(snapshot(thread_id.clone()));

        let effects = reduce(&mut app, Action::ReorderThreadQueue(1));
        assert_eq!(app.thread_queue_selected, 1);
        assert!(matches!(
            effects.as_slice(),
            [Effect::MutateThreadQueue(ThreadQueueMutation::Reorder {
                queued_submission_ids,
                ..
            })] if queued_submission_ids == &vec!["q2".to_string(), "q1".to_string()]
        ));

        app.thread_queue_selected = 0;
        reduce(&mut app, Action::BeginThreadQueueStart);
        assert!(matches!(
            app.pending_thread_queue_mutation,
            Some(ThreadQueueMutation::Start { .. })
        ));
        let effects = reduce(&mut app, Action::ConfirmPendingOperation);
        assert!(matches!(
            effects.as_slice(),
            [Effect::MutateThreadQueue(ThreadQueueMutation::Start { .. })]
        ));

        reduce(&mut app, Action::BeginThreadQueueDelete);
        assert!(matches!(
            app.pending_thread_queue_mutation,
            Some(ThreadQueueMutation::Delete { .. })
        ));
        reduce(&mut app, Action::CancelPendingOperation);
        assert!(app.pending_thread_queue_mutation.is_none());
    }
}
