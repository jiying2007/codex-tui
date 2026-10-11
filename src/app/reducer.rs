use super::*;

pub fn reduce(state: &mut AppState, action: Action) -> Vec<Effect> {
    state.observe_user_response_edit(&action);
    state.observe_local_edit_action(&action);
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
        Action::ForgeReviewLoaded(result) => {
            let crate::forge::ForgeReviewResult {
                target,
                summary: review,
            } = *result;
            if state
                .thread_by_id(&target.thread_id)
                .is_none_or(|thread| thread.metadata.cwd != target.cwd)
                || target.thread_id != review.thread_id
                || target.cwd != review.cwd
                || target.change_request_iid != review.change_request_iid
            {
                return vec![];
            }
            if let Some(observation) = state.forge_observations.get_mut(&review.thread_id.0)
                && observation.cwd == review.cwd
                && observation.identity.as_ref().is_some_and(|identity| {
                    identity.provider == target.provider
                        && identity.host == target.host
                        && identity.project_id == target.project_id
                        && identity.path_with_namespace == target.project_path
                })
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
            if !state.begin_mutation_editor(InputMode::WorktreeCreateBranch) {
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
            if !state.begin_mutation_editor(InputMode::WorktreeDeleteBranch) {
                return vec![];
            }
            state.input_buffer = state
                .selected_managed_worktree()
                .and_then(|record| record.branch.clone())
                .or_else(|| {
                    state
                        .git_context(&thread_id)
                        .and_then(|context| context.branch.clone())
                })
                .unwrap_or_default();
            state.input_mode = InputMode::WorktreeDeleteBranch;
        }
        Action::ConfirmPendingOperation => {
            if state.pending_queue_confirmation.is_some() {
                return state.confirm_queue_operation();
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
            state.pending_queue_confirmation = None;
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
            state.pending_queue_confirmation = None;
            state.thread_queue_editor = None;
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
            state.thread_queue_editor = None;
            let thread_id = state
                .thread_queue_snapshot
                .as_ref()
                .map(|snapshot| snapshot.thread_id.clone())
                .or_else(|| state.current_thread_id().cloned());
            state.thread_queue_open = false;
            state.thread_queue_loading = false;
            state.thread_queue_error = None;
            state.pending_queue_confirmation = None;
            state.input_mode = InputMode::Normal;
            state.input_buffer.clear();
            if let Some(thread_id) = thread_id {
                return vec![Effect::StopWatchingThreadQueue(thread_id)];
            }
        }
        Action::ThreadQueueLoaded(snapshot) => state.install_thread_queue(snapshot),
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
        Action::BeginThreadQueueAdd => state.begin_queue_input(false),
        Action::BeginThreadQueueEdit => state.begin_queue_input(true),
        Action::BeginThreadQueueDelete => state.begin_queue_confirmation(false),
        Action::BeginThreadQueueStart => state.begin_queue_confirmation(true),
        Action::ReorderThreadQueue(delta) => {
            let Some(snapshot) = state.ready_queue_snapshot() else {
                state.refuse_queue_mutation();
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
                    // Keep the cursor on the observed item until upstream confirms order.
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
            if !state.begin_mutation_editor(InputMode::GoalObjective) {
                return vec![];
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
            return state.begin_local_edit_write(Effect::SaveSavedView { view });
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
        Action::OpenContext => state.open_context_menu(),
        Action::CloseContext => state.close_context_menu(),
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
            let Some(choice) = state.take_context_choice() else {
                return vec![];
            };

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
                        if !state.begin_mutation_editor(InputMode::ForgeMergeRequestTitle) {
                            return vec![];
                        }
                        state.pending_forge_operation = None;
                        state.pending_forge_payload = None;
                        state.input_buffer.clear();
                        state.input_mode = InputMode::ForgeMergeRequestTitle;
                    }
                    ContextChoice::ForgeComment => {
                        if !state.begin_mutation_editor(InputMode::ForgeComment) {
                            return vec![];
                        }
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
            state.hot_slot_bind_pending = state.selected_local_target();
        }
        Action::UseHotSlot(slot) => {
            if let Some(target) = state.hot_slot_bind_pending.take() {
                if state.local_target_live(&target) {
                    return vec![Effect::SetHotSlot { slot, target }];
                }
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "shortcut target disappeared; no binding saved",
                        "快捷槽目标已消失；未保存绑定",
                    )
                    .into(),
                );
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
            if let Some(pending) = state
                .pending_requests
                .iter_mut()
                .find(|pending| pending.request_id == request.request_id)
            {
                *pending = request;
            } else {
                state.pending_requests.push(request);
            }
            state.notice_changed_user_input();
            state.rebuild_pending_request_threads();
            if is_current_thread {
                state.goal_actions_open = false;
                if state.input_mode == InputMode::Composer {
                    state.input_mode = InputMode::Normal;
                }
            }
        }
        Action::InteractiveResolved { request_id } => {
            state.forget_user_response(&request_id);
            state
                .pending_requests
                .retain(|request| request.request_id != request_id);
            state.rebuild_pending_request_threads();
            state.resolve_user_input_editor(&request_id);
            ensure_selection_visible(state);
        }
        Action::ResolvePending(resolution) => {
            if let Some(request) = state.current_pending_request().cloned() {
                if state.user_response_blocked(&request.request_id) {
                    return vec![];
                }
                let allowed = match request.kind {
                    InteractiveRequestKind::UserInput { .. } => matches!(
                        resolution,
                        InteractiveResolution::Decline | InteractiveResolution::Cancel
                    ),
                    _ => !matches!(resolution, InteractiveResolution::UserInput(_)),
                };
                if allowed {
                    return state.begin_interactive_response(request, resolution);
                }
            }
        }
        Action::BeginUserInput => state.begin_user_input(),
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
                    state.clear_user_input_editor();
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
                state.close_context_menu();
                return vec![];
            }
            if state.hot_slot_bind_pending.take().is_some() {
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
                state.clear_user_input_editor();
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
            if let Some((target, alias)) = state
                .selected_thread()
                .map(|thread| (thread.id.clone(), thread.alias.clone().unwrap_or_default()))
            {
                state.alias_target = Some(target);
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
            if !state.guard_mutation_editor(mode) {
                return vec![];
            }
            if matches!(mode, InputMode::ThreadQueueAdd | InputMode::ThreadQueueEdit) {
                return state.commit_queue_input();
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
                        state.mutation_editor = None;
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
                        state.mutation_editor = None;
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
                state.mutation_editor = None;
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
                state.mutation_editor = None;
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
                    state.mutation_editor = None;
                    state.input_mode = InputMode::Normal;
                    state.input_buffer.clear();
                    return vec![];
                };
                let objective = state.input_buffer.trim().to_string();
                if objective.is_empty() {
                    return vec![];
                }
                let is_new = !state.goals.contains_key(&thread_id.0);
                state.mutation_editor = None;
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
                let workspace = state.new_scratch_workspace.clone();
                return state.begin_local_edit_write(Effect::CreateScratch { title, workspace });
            }
            if mode == InputMode::SavedViewField {
                let value = state.input_buffer.clone();
                let Some(editor) = state.saved_view_editor.as_mut() else {
                    return vec![];
                };
                if let Err(error) = editor.commit_text_value(value) {
                    state.saved_view_editor_error = Some(error);
                } else {
                    state.saved_view_editor_error = None;
                    state.input_mode = InputMode::Normal;
                    state.input_buffer.clear();
                }
                return vec![];
            }
            if mode == InputMode::Note {
                let text = state.input_buffer.trim().to_string();
                let Some(target) = state.note_target.clone() else {
                    return vec![];
                };
                if target.kind == SourceKind::ScratchWork {
                    return state.begin_local_edit_write(Effect::UpdateScratchNote {
                        scratch_id: target.value,
                        note: (!text.is_empty()).then_some(text),
                    });
                }
                return state.begin_local_edit_write(Effect::SaveSourceNote {
                    owner: target,
                    text,
                });
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
                return state.commit_user_input();
            }
            if mode == InputMode::Composer {
                return prompt::submit(state);
            }
            if mode == InputMode::Alias {
                let Some(index) = state
                    .alias_target
                    .as_ref()
                    .and_then(|id| state.thread_index_by_id.get(&id.0))
                    .copied()
                else {
                    state.mutation_notice = Some(
                        local_text(
                            state.language,
                            "alias target is no longer available; edit retained, no changes saved",
                            "别名目标已不可用；已保留输入，未保存任何更改",
                        )
                        .into(),
                    );
                    return vec![];
                };
                let alias = state.input_buffer.trim().to_string();
                if let Some(thread) = state.threads.get_mut(index) {
                    thread.alias = (!alias.is_empty()).then_some(alias);
                    if let Some(alias) = thread.alias.clone() {
                        state.local_aliases.insert(thread.id.0.clone(), alias);
                    } else {
                        state.local_aliases.remove(&thread.id.0);
                    }
                    state.alias_target = None;
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
            state.mutation_editor = None;
            state.thread_queue_editor = None;
            state.alias_target = None;
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
                state.clear_user_input_editor();
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
