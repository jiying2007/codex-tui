use super::*;

/// Wrap even stale/oversized indices without truncating usize to i32.
pub(super) fn wrap_selection(selected: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    ((selected as i128 + i128::from(delta)).rem_euclid(len as i128)) as usize
}

pub fn reduce(state: &mut AppState, action: Action) -> Vec<Effect> {
    let old_view = state.view.clone();
    let old_previous = state.previous_target.clone();
    let hot_slot_navigation = matches!(
        &action,
        Action::UseHotSlot(_) | Action::OpenRecentNotification
    );
    let status_may_change = matches!(
        &action,
        Action::ReplaceThreads(_)
            | Action::ConversationLoaded(_)
            | Action::OlderConversationLoaded { .. }
            | Action::TranscriptSearchLoadedPage { .. }
            | Action::ConversationFailed { .. }
            | Action::BeginSearch
            | Action::CommitInput
            | Action::CancelInput
            | Action::PromptSubmitted { .. }
            | Action::PromptFailed { .. }
            | Action::Quit
    );
    let mut effects = reduce_inner(state, action);
    if old_view != state.view
        && matches!(old_view, View::Thread(_))
        && !(hot_slot_navigation && matches!(state.view, View::Thread(_)))
    {
        // A hot slot between two threads retains one return hop; all other
        // departures invalidate the stale navigation route.
        state.quick_prompt_return_view = None;
        state.hot_slot_return_selected = None;
    }
    state.invalidate_request_details();
    state.invalidate_reading_snapshot();
    // Return routes may outlive their bounded presentation cache entry.
    if old_view != state.view
        && !state.should_quit
        && let Some(effect) = state.rehydrate_evicted_thread()
    {
        effects.push(effect);
    }
    let watch_command = effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::LoadConversation(_)
                | Effect::LoadOlderConversation { .. }
                | Effect::JumpToTranscriptHit(_)
                | Effect::SubmitPrompt { .. }
                | Effect::StopWatchingConversation(_)
        )
    });
    // Avoid a 50k-thread scan on every keystroke, cursor movement or redraw.
    if status_may_change
        || old_view != state.view
        || old_previous != state.previous_target
        || watch_command
    {
        super::watch::reconcile(state, effects)
    } else {
        effects
    }
}

fn reduce_inner(state: &mut AppState, action: Action) -> Vec<Effect> {
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
    if matches!(
        &action,
        Action::PlanningSnapshotLoaded(_)
            | Action::TogglePin
            | Action::MarkUnread
            | Action::AcknowledgeAttention
            | Action::CommitInput
    ) {
        state.planning_intent_generation = state.planning_intent_generation.wrapping_add(1);
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
            state.refresh_session_collisions();
            state.reconcile_cwd_locality_cache(state.host_local_only || state.repo_backed_only);
            restore_selection(state, selected_id);
        }
        Action::BackendStatus(status) => {
            if state.backend_status.source != status.source {
                state.cwd_localities.clear();
                state.git_contexts.clear();
                state.forge_observations.clear();
                state.worktree_collision_counts.clear();
            }
            state.backend_status = status;
            state.bind_operator_status();
        }
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
            if state.app_server_target_is_remote() {
                return vec![];
            }
            state.worktree_collision_counts.clear();
            if context.observed_at_unix_ms > 0 && context.error.is_none() {
                state
                    .cwd_localities
                    .insert(context.cwd.clone(), CwdLocality::LocalDirectory);
            }
            propagate_git_context(state, context);
            state.refresh_session_collisions();
            ensure_selection_visible(state);
        }
        Action::ForgeObservationLoaded(observation) => {
            if state.app_server_target_is_remote() {
                return vec![];
            }
            propagate_forge_observation(state, observation);
        }
        Action::ForgeReviewLoaded(result) => super::forge_review::install(state, *result),
        Action::GitReviewLoaded(review) => state.install_git_review(review),
        Action::PlanningSnapshotLoaded(snapshot) => {
            let selection = planning_selection::PlanningSelection::capture(state);
            state.planning_snapshot = snapshot;
            state.refresh_session_attention_policy();
            selection.restore(state);
        }
        Action::PlanningStoreDegraded(error) => {
            state.planning_store_error = error;
        }
        Action::MutationNotice(notice) => {
            state.mutation_notice = Some(notice);
        }
        Action::RecentNotificationObserved(recent) => {
            state.recent_notification = Some(recent);
        }
        Action::ClearRecentNotification => {
            state.recent_notification = None;
        }
        Action::GoalObserved(goal) => {
            state.goal_checked.insert(goal.thread_id.0.clone());
            state.goals.insert(goal.thread_id.0.clone(), goal);
        }
        Action::GoalCleared(thread_id) => {
            state.goal_checked.insert(thread_id.0.clone());
            state.goals.remove(&thread_id.0);
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
        Action::RefreshGoal => {
            if !matches!(state.view, View::Thread(_)) || state.current_pending_request().is_some() {
                return vec![];
            }
            if let Some(thread_id) = state.current_thread_id().cloned()
                && state.thread_by_id(&thread_id).is_some()
            {
                return vec![Effect::RefreshGoal(thread_id)];
            }
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
            state.board_stage_index =
                wrap_selection(state.board_stage_index, delta, WorkflowStage::ALL.len());
            state.board_selected = 0;
        }
        Action::MovePlanningSelection(delta) => {
            let len = state.visible_planning_cards().len();
            state.board_selected = wrap_selection(state.board_selected, delta, len);
        }
        Action::CycleSavedView(delta) => {
            let len = state.planning_views().len();
            state.planning_view_index = wrap_selection(state.planning_view_index, delta, len);
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
        Action::BeginWorkrefLink => state.begin_workref_link(),
        Action::WorkrefLinkSubmissionRejected(error) => {
            state.workref_link_submission_rejected(error);
        }
        Action::TargetLookup(action) => state.reduce_target_lookup(action),
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
            state.context_selected = wrap_selection(state.context_selected, delta, len);
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

            let Some(target) = state
                .selected_local_target()
                .filter(|target| target.kind == SourceKind::CodexThread)
            else {
                return vec![];
            };
            match choice {
                ContextChoice::Snooze => {
                    state.snooze_target = Some(target);
                    state.input_buffer = "1h".into();
                    state.input_mode = InputMode::Snooze;
                }
                ContextChoice::EditNote => {
                    let existing = state
                        .planning_snapshot
                        .notes
                        .iter()
                        .find(|note| note.owner == target)
                        .map(|note| note.text.clone());
                    state.note_target = Some(target);
                    state.input_buffer = existing.unwrap_or_default();
                    state.input_mode = InputMode::Note;
                }
                ContextChoice::LinkWorkItem => {
                    state.begin_workref_link();
                }
                ContextChoice::Bookmark => {
                    let label = state
                        .thread_by_id(&ThreadId::new(target.value.clone()))
                        .map(|thread| thread.display_title().to_owned());
                    return vec![Effect::CreateBookmark {
                        source: target,
                        label,
                    }];
                }
                ContextChoice::NewCodexThread | ContextChoice::ForkCodexThread => {
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
            if let Some(target) = state
                .selected_local_target()
                .filter(|target| target.kind == SourceKind::CodexThread)
            {
                state.snooze_target = Some(target);
                state.input_buffer = "1h".into();
                state.input_mode = InputMode::Snooze;
            }
        }
        Action::BeginHotSlotBind => super::shortcuts::begin_bind(state),
        Action::OpenRecentNotification => {
            if !state.recent_alert_controls_available() {
                // Hidden by an overlay or active editor: do not navigate behind
                // it and do not consume the event before its real expiry.
                return vec![];
            }
            let Some(recent) = state.recent_notification.take() else {
                return vec![];
            };
            // The main-loop expiry sweep is a presentation convenience, not
            // authorization to open a source after its alert has expired.
            if recent.visible_until <= Instant::now() {
                return vec![];
            }
            let Some(id) = recent.thread_id else {
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "this update has no active Codex thread; check the authoritative source",
                        "该更新没有可打开的 Codex 会话；请核对权威来源",
                    )
                    .into(),
                );
                return vec![];
            };
            let Some(index) = state
                .thread_index_by_id
                .get(&id.0)
                .copied()
                .filter(|index| {
                    state
                        .threads
                        .get(*index)
                        .is_some_and(|thread| thread.id == id)
                })
            else {
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "notified thread is no longer available",
                        "收到提醒的会话已不可用",
                    )
                    .into(),
                );
                return vec![];
            };
            if matches!(&state.view, View::Thread(current) if *current == id) {
                return vec![];
            }
            state.hot_slot_return_selected = state.selected_thread_id();
            state.previous_target = state.current_thread_id().cloned();
            state.quick_prompt_return_view = Some(state.view.clone());
            state.selected = index;
            state.thread_ui.entry(id.0.clone()).or_default();
            state.prepare_conversation(&id);
            state.view = View::Thread(id.clone());
            return vec![Effect::LoadConversation(id)];
        }
        Action::UseHotSlot(slot) => {
            if !(1..=9).contains(&slot) {
                return vec![];
            }
            if let Some((target, scope)) = state.hot_slot_bind_pending.take() {
                if state.shortcut_scope() == Some(scope.as_str())
                    && target.kind == SourceKind::CodexThread
                    && state.local_target_live(&target)
                    && state.planning_store_error.is_none()
                {
                    state.operator_local.hot_slots.insert(slot, target.value);
                    return vec![Effect::PersistOperatorState];
                }
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "shortcut scope, target or store unavailable; no binding saved",
                        "快捷槽作用域、目标或存储不可用；未保存绑定",
                    )
                    .into(),
                );
                return vec![];
            }

            if state.shortcut_scope().is_none() {
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "shortcut scope unavailable",
                        "快捷槽作用域不可用",
                    )
                    .into(),
                );
                return vec![];
            }
            let Some(id) = state
                .operator_local
                .hot_slots
                .get(&slot)
                .cloned()
                .map(ThreadId::new)
            else {
                if state
                    .planning_snapshot
                    .hot_slots
                    .iter()
                    .any(|old| old.slot == slot)
                {
                    state.mutation_notice = Some(
                        local_text(
                            state.language,
                            "historical shortcut retained; select a thread and press = to bind",
                            "历史快捷槽已保留；选择会话后按 = 重新绑定",
                        )
                        .into(),
                    );
                } else {
                    super::shortcuts::empty_slot(state, slot);
                }
                return vec![];
            };
            let Some(index) = state
                .thread_index_by_id
                .get(&id.0)
                .copied()
                .filter(|index| state.threads.get(*index).is_some_and(|thread| thread.id == id))
            else {
                state.mutation_notice = Some(
                    local_text(
                        state.language,
                        "shortcut thread is not available on this target",
                        "快捷槽会话在当前目标中不可用",
                    )
                    .into(),
                );
                return vec![];
            };
            if matches!(&state.view, View::Thread(current) if *current == id) {
                return vec![];
            }
            state.hot_slot_return_selected = state.selected_thread_id();
            state.previous_target = state.current_thread_id().cloned();
            state.quick_prompt_return_view = Some(state.view.clone());
            state.selected = index;
            state.thread_ui.entry(id.0.clone()).or_default();
            state.prepare_conversation(&id);
            state.view = View::Thread(id.clone());
            return vec![Effect::LoadConversation(id)];
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
                View::Thread(id) | View::Review(id) | View::Workspace(id) => Some(id.clone()),
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
            if cwd.trim().is_empty() || state.app_server_target_is_remote() {
                state.mutation_notice = Some("Review requires a local App Server target".into());
                return vec![];
            }
            if !matches!(state.view, View::Review(_)) {
                state.review_return_view = Some(state.view.clone());
            }
            state.review_selected = 0;
            state.review_scroll = 0;
            state.prepare_git_review(&thread_id, cwd.clone());

            let forge_review_effect = super::forge_review::probe(state, &thread_id, &cwd);

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
                View::Thread(id) | View::Review(id) | View::Workspace(id) => Some(id.clone()),
                View::Board => state.selected_planning_card().and_then(|card| {
                    (card.anchor.kind == SourceKind::CodexThread)
                        .then(|| ThreadId::new(card.anchor.value.clone()))
                }),
                View::Scratch(_) => None,
            };
            let Some(thread_id) = thread_id else {
                return vec![];
            };
            if state.app_server_target_is_remote() {
                state.mutation_notice = Some("Workspace requires a local App Server target".into());
                return vec![];
            }
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
            state.review_selected =
                wrap_selection(state.review_selected, delta, review.changes.len());
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
            let thread_id = thread_id.clone();
            let Some(review) = state.git_reviews.get(&thread_id.0) else {
                return vec![];
            };
            let Some(change) = review.changes.get(state.review_selected) else {
                return vec![];
            };
            let cwd = review.cwd.clone();
            let path = change.path.clone();
            let live_thread = !state.app_server_target_is_remote()
                && state
                    .thread_by_id(&thread_id)
                    .is_some_and(|thread| thread.metadata.cwd == cwd);
            let worktree_root = state
                .git_context(&thread_id)
                .filter(|context| {
                    context.is_repository && context.error.is_none() && context.cwd == cwd
                })
                .and_then(|context| context.worktree.as_ref())
                .map(|worktree| worktree.canonical_path.clone());
            let Some(worktree_root) = worktree_root.filter(|_| live_thread) else {
                let message = if state.language.is_simplified_chinese() {
                    "Git 工作树身份不可用或已过期，请刷新 Review 后再打开文件"
                } else {
                    "Git worktree identity unavailable or stale; refresh Review before opening a file"
                };
                if let Some(review) = state.git_reviews.get_mut(&thread_id.0) {
                    review.error = Some(message.into());
                }
                return vec![];
            };
            // Porcelain v2 paths are worktree-relative even when the thread cwd is nested.
            return vec![Effect::OpenExternalEditor {
                thread_id,
                cwd,
                worktree_root,
                path,
            }];
        }
        Action::OpenReviewExternal => {
            return state.open_review_external();
        }
        Action::ConversationLoaded(page) => {
            state.prompt_submissions.history_resolved(&page.thread_id);
            let thread_id = page.thread_id.clone();
            state.install_latest_conversation(page);
            state.observe_history_activity(&thread_id);
            state.touch_conversation_cache(&thread_id);
        }
        Action::OlderConversationLoaded {
            page,
            turn_cursor,
            item_cursor,
        } => {
            let thread_id = page.thread_id.clone();
            if state.install_older_conversation(page, turn_cursor, item_cursor) {
                // Older pages are not authoritative current-turn observations.
                state.touch_conversation_cache(&thread_id);
            }
        }
        Action::OlderConversationFailed {
            thread_id,
            turn_cursor,
            item_cursor,
            error,
        } => {
            state.older_conversation_failed(thread_id, turn_cursor, item_cursor, error);
        }
        Action::TranscriptSearchLoadedPage { page, item_id } => {
            let thread_id = page.thread_id.clone();
            state.discard_reading_snapshot(&thread_id);
            let key = thread_id.0.clone();
            let target_index = page
                .items
                .iter()
                .position(|item| {
                    item.item_id == item_id
                        && state
                            .transcript_search_active_hit
                            .as_ref()
                            .filter(|hit| hit.thread_id == thread_id)
                            .and_then(|hit| hit.turn_id.as_deref())
                            .is_none_or(|turn| item.turn_id == turn)
                })
                .unwrap_or(0);
            let target_turn = page
                .items
                .get(target_index)
                .map(|item| item.turn_id.clone());
            state
                .conversations
                .entry(key)
                .or_insert_with(|| ConversationState::loading(thread_id.clone()))
                .replace_page(page);
            let ui = state.thread_ui.entry(thread_id.0.clone()).or_default();
            ui.follow = false;
            ui.scroll = u16::try_from(target_index).unwrap_or(u16::MAX);
            ui.scroll_anchor = Some(item_id);
            ui.scroll_turn_id = target_turn;
            ui.scroll_offset = 0;
            // An exact hit contains no current-turn status; retain known activity.
            state.touch_conversation_cache(&thread_id);
        }
        Action::ConversationFailed { thread_id, error } => {
            state.discard_reading_snapshot(&thread_id);
            state.active_history_watches.remove(&thread_id.0);
            state.unregistered_watches.remove(&thread_id.0);
            state.prompt_submissions.history_resolved(&thread_id);
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
            return prompt::fail(state, thread_id, request_id, error);
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
            if is_current_thread && state.input_mode == InputMode::Composer {
                state.input_mode = InputMode::Normal;
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
            if state.request_details_open() {
                return vec![];
            }
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
        Action::BeginUserInput => {
            if !state.request_details_open() {
                state.begin_user_input();
            }
        }
        Action::OpenRequestDetails => state.open_request_details(),
        Action::CloseRequestDetails => state.close_request_details(),
        Action::ScrollRequestDetails(delta) => state.scroll_request_details(delta),
        Action::PageRequestDetails(delta) => state.page_request_details(delta),
        Action::RequestDetailsBoundary(end) => state.request_details_boundary(end),
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
        Action::QuickPrompt => return super::prompt::open(state),
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
            if state.context_open {
                state.close_context_menu();
                return vec![];
            }
            if state.hot_slot_bind_pending.take().is_some() {
                super::shortcuts::cancelled(state);
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
            state.view = state
                .quick_prompt_return_view
                .take()
                .filter(|view| state.search_return_view_is_valid(view))
                .unwrap_or(View::Registry);
            if thread_id.is_some() {
                if let Some(selected_id) = state.hot_slot_return_selected.take()
                    && let Some(index) = state.thread_index_by_id.get(&selected_id.0).copied()
                    && state
                        .threads
                        .get(index)
                        .is_some_and(|thread| thread.id == selected_id)
                {
                    state.selected = index;
                }
                if matches!(state.view, View::Registry) {
                    ensure_selection_visible(state);
                }
            }
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
        Action::ToggleHelp => {
            state.show_help = !state.show_help;
            if state.show_help {
                state.help_top = 0;
            }
        }
        Action::MoveHelp(delta) => {
            if state.show_help {
                state.help_top = (state.help_top as i128 + i128::from(delta))
                    .clamp(0, state.help_max_top.get() as i128)
                    as usize;
            }
        }
        Action::SetDraft(draft) => {
            if let Some(id) = state.current_thread_id().cloned() {
                state.composer_cursor = draft.chars().count();
                state.composer_scroll_back = 0;
                state.prompt_submissions.edited(&id);
                state.thread_ui.entry(id.0).or_default().draft = draft;
                return vec![Effect::PersistOperatorStateDeferred];
            }
        }
        Action::ScrollBy(delta) => return super::thread_navigation::scroll(state, delta),
        Action::ToggleFollow => return super::thread_navigation::toggle_follow(state),
        Action::FollowNewest => return super::thread_navigation::position(state, true),
        Action::ConversationStart => return super::thread_navigation::position(state, false),
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
            state.transcript_search_selected =
                wrap_selection(state.transcript_search_selected, delta, len);
        }
        Action::OpenTranscriptSearchSelected => {
            let Some(hit) = state.transcript_search_hit().cloned() else {
                return vec![];
            };
            let thread_id = hit.thread_id.clone();
            state.transcript_search_open = false;
            let previous = state.current_thread_id().cloned();
            let stale_previous = state
                .previous_target
                .take()
                .filter(|old| Some(old) != previous.as_ref() && old != &thread_id);
            state.previous_target = previous;
            state.thread_ui.entry(thread_id.0.clone()).or_default();
            state.prepare_conversation(&thread_id);
            state.view = View::Thread(thread_id.clone());
            state.transcript_search_active_hit = Some(hit.clone());
            let mut effects = Vec::new();
            if let Some(stale) = stale_previous {
                effects.push(Effect::StopWatchingConversation(stale));
            }
            if hit.turn_id.is_some() && hit.item_id.is_some() {
                effects.push(Effect::JumpToTranscriptHit(hit));
            } else {
                effects.push(Effect::LoadConversation(thread_id));
            }
            return effects;
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
            _ if state.workref_link_pending => {}
            InputMode::Composer => {
                if super::composer::insert(state, &character.to_string()) {
                    return vec![Effect::PersistOperatorStateDeferred];
                }
            }
            InputMode::Search
            | InputMode::TranscriptSearch
            | InputMode::Alias
            | InputMode::UserInput
            | InputMode::Snooze
            | InputMode::Note
            | InputMode::WorkrefLink => {
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
            if text.is_empty() || state.workref_link_pending {
                return vec![];
            }
            let text = normalize_pasted_input(state.input_mode, &text);
            if text.is_empty() {
                return vec![];
            }
            match state.input_mode {
                InputMode::Normal => {}
                InputMode::Composer => {
                    if super::composer::insert(state, &text) {
                        return vec![Effect::PersistOperatorStateDeferred];
                    }
                }
                InputMode::Search
                | InputMode::TranscriptSearch
                | InputMode::Alias
                | InputMode::UserInput
                | InputMode::Snooze
                | InputMode::Note
                | InputMode::WorkrefLink => {
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
            _ if state.workref_link_pending => {}
            InputMode::Composer => {
                if super::composer::backspace(state) {
                    return vec![Effect::PersistOperatorStateDeferred];
                }
            }
            InputMode::Search
            | InputMode::TranscriptSearch
            | InputMode::Alias
            | InputMode::UserInput
            | InputMode::Snooze
            | InputMode::Note
            | InputMode::WorkrefLink => {
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
        Action::ComposerCursorMove(delta) => super::composer::move_cursor(state, delta),
        Action::ComposerCursorBoundary(end) => super::composer::boundary(state, end),
        Action::ComposerViewScroll(delta) => super::composer::scroll(state, delta),
        Action::CommitInput => {
            if state.workref_link_pending {
                return vec![];
            }
            let mode = state.input_mode;
            if mode == InputMode::WorkrefLink {
                return state.commit_workref_link().into_iter().collect();
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
            if mode == InputMode::Note {
                let text = state.input_buffer.trim().to_string();
                let Some(target) = state.note_target.clone() else {
                    return vec![];
                };
                if target.kind != SourceKind::CodexThread {
                    return vec![];
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
                let Some(anchor) = state
                    .snooze_target
                    .take()
                    .filter(|target| target.kind == SourceKind::CodexThread)
                else {
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
            state.alias_target = None;
            let mut search_watch_to_release = None;
            if state.input_mode == InputMode::WorkrefLink {
                if state.workref_link_pending {
                    state.workref_link_cancel_refused();
                    return vec![];
                }
                state.cancel_workref_link();
                return vec![];
            }
            if state.input_mode == InputMode::TranscriptSearch {
                state.input_mode = InputMode::Normal;
                state.input_buffer.clear();
                return vec![];
            }
            if state.input_mode == InputMode::Snooze {
                state.snooze_target = None;
            }
            if state.input_mode == InputMode::Note {
                state.note_target = None;
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
