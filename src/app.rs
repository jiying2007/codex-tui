use crate::backend::BackendStatus;
use crate::batch_local::{LocalBatchAction, LocalBatchPlan, parse_priority};
use crate::command::Command;
#[cfg(test)]
use crate::conversation::{ConversationPage, RpcRequestId, UserInputQuestion};
use crate::conversation::{
    ConversationState, InteractiveRequest, InteractiveRequestKind, InteractiveResolution,
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

mod context_menu;
mod lifecycle;
mod local_edit;
mod mutation_editor;
pub(crate) mod palette;
mod palette_intent;
mod planning_selection;
pub mod planning_worker;
mod prompt;
mod queue_confirmation;
mod queue_editor;
mod reducer;
mod review;
mod state;
mod types;
mod user_input;
mod user_response;

pub use queue_confirmation::QueueConfirmation;
pub use reducer::reduce;
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
    local_edit_revision: u64,
    pending_local_edit: Option<local_edit::PendingLocalEdit>,
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
    pub pending_queue_confirmation: Option<QueueConfirmation>,
    thread_queue_editor: Option<queue_editor::QueueEditor>,
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
    command_palette_intent: Option<palette_intent::PaletteIntent>,
    pub context_open: bool,
    context_menu: Option<context_menu::ContextMenu>,
    pub context_selected: usize,
    pub hot_slot_bind_pending: Option<SourceRef>,
    pub review_selected: usize,
    pub review_scroll: u16,
    pub review_word_diff: bool,
    pub pending_requests: Vec<InteractiveRequest>,
    pending_request_threads: BTreeSet<String>,
    user_input_editor: Option<user_input::UserInputEditor>,
    user_responses: user_response::UserResponses,
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
    alias_target: Option<ThreadId>,
    mutation_editor: Option<mutation_editor::MutationEditor>,
    search_return_view: Option<View>,
}

#[derive(Clone, Debug)]
struct ForgeMutationTarget {
    cwd: String,
    branch: String,
    identity: ForgeIdentity,
    change_request: Option<ChangeRequestSummary>,
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
    if state
        .thread_by_id(&observation.thread_id)
        .is_none_or(|thread| thread.metadata.cwd != observation.cwd)
    {
        return;
    }
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
            .filter(|existing| {
                existing.identity.is_some() && existing.identity == observation.identity
            })
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
mod tests;

#[cfg(test)]
mod thread_queue_reducer_tests;
