use super::*;

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
            local_edit_revision: 0,
            pending_local_edit: None,
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
            pending_queue_confirmation: None,
            thread_queue_editor: None,
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
            command_palette_intent: None,
            context_open: false,
            context_menu: None,
            context_selected: 0,
            hot_slot_bind_pending: None,
            review_selected: 0,
            review_scroll: 0,
            review_word_diff: false,
            pending_requests: vec![],
            pending_request_threads: BTreeSet::new(),
            user_input_editor: None,
            user_responses: Default::default(),
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
            alias_target: None,
            mutation_editor: None,
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

    pub(super) fn prepare_conversation(&mut self, thread_id: &ThreadId) {
        self.conversations
            .entry(thread_id.0.clone())
            .or_insert_with(|| ConversationState::loading(thread_id.clone()))
            .loading = true;
        self.touch_conversation_cache(thread_id);
    }

    pub(super) fn touch_conversation_cache(&mut self, thread_id: &ThreadId) {
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

    pub(super) fn prepare_git_review(&mut self, thread_id: &ThreadId, cwd: String) {
        self.git_reviews.insert(
            thread_id.0.clone(),
            GitReview::pending(thread_id.clone(), cwd),
        );
        self.touch_git_review_cache(thread_id);
    }

    pub(super) fn touch_git_review_cache(&mut self, thread_id: &ThreadId) {
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

    pub(super) fn rebuild_thread_indexes(&mut self) {
        let (thread_index_by_id, thread_indices_by_cwd) = build_thread_indexes(&self.threads);
        self.thread_index_by_id = thread_index_by_id;
        self.thread_indices_by_cwd = thread_indices_by_cwd;
    }

    pub(super) fn thread_by_id(&self, thread_id: &ThreadId) -> Option<&ThreadSummary> {
        self.thread_index_by_id
            .get(&thread_id.0)
            .and_then(|index| self.threads.get(*index))
            .filter(|thread| thread.id == *thread_id)
            .or_else(|| self.threads.iter().find(|thread| thread.id == *thread_id))
    }

    pub(super) fn search_return_view_is_valid(&self, view: &View) -> bool {
        match view {
            View::Thread(thread_id)
            | View::Review(thread_id)
            | View::Workspace(thread_id)
            | View::ManagedWorktrees(thread_id) => self.thread_by_id(thread_id).is_some(),
            View::Registry | View::Board | View::Scratch(_) => true,
        }
    }

    pub(super) fn thread_id_for_view(view: &View) -> Option<ThreadId> {
        match view {
            View::Thread(thread_id)
            | View::Review(thread_id)
            | View::Workspace(thread_id)
            | View::ManagedWorktrees(thread_id) => Some(thread_id.clone()),
            View::Registry | View::Board | View::Scratch(_) => None,
        }
    }

    pub(super) fn search_origin_thread_id(&self) -> Option<ThreadId> {
        let return_view = self.search_return_view.as_ref()?;
        match return_view {
            View::Board | View::Scratch(_) => self
                .board_return_view
                .as_ref()
                .and_then(Self::thread_id_for_view),
            _ => Self::thread_id_for_view(return_view),
        }
    }

    pub(super) fn thread_ids_for_cwd(&self, cwd: &str) -> Vec<ThreadId> {
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

    pub(super) fn refresh_cwd_locality(&mut self, cwd: &str) -> CwdLocality {
        let locality = classify_cwd(cwd);
        self.cwd_localities.insert(cwd.to_string(), locality);
        locality
    }

    pub(super) fn reconcile_cwd_locality_cache(&mut self, fill_missing: bool) {
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

    pub(super) fn thread_visible_in_registry(&self, thread: &ThreadSummary) -> bool {
        let query = self.filter.trim().to_lowercase();
        self.thread_visible_in_registry_with_query(thread, &query)
    }

    pub(super) fn thread_visible_in_registry_with_query(
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

    pub(super) fn thread_matches_repo_scope(&self, thread: &ThreadSummary) -> bool {
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

    pub(super) fn freeze_visible_batch(&mut self, action: LocalBatchAction) {
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

    pub(super) fn current_forge_mutation_target(&self) -> Option<ForgeMutationTarget> {
        let thread_id = match &self.view {
            View::Review(id) | View::Workspace(id) => id,
            _ => return None,
        };
        let live = self.thread_by_id(thread_id)?;
        let context = self.git_context(thread_id)?;
        let branch = context.branch.clone()?;
        let observation = self.forge_observation(thread_id)?;
        if context.cwd != live.metadata.cwd
            || observation.cwd != live.metadata.cwd
            || context.error.is_some()
            || observation.error.is_some()
        {
            return None;
        }
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
        if let Some(menu) = self.context_menu.as_ref() {
            return menu.choices.clone();
        }
        self.build_context_choices()
    }

    pub(super) fn build_context_choices(&self) -> Vec<ContextChoice> {
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

    pub(super) fn worktree_collision_count_uncached(&self, thread_id: &ThreadId) -> usize {
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

    pub(super) fn rebuild_pending_request_threads(&mut self) {
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
