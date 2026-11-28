use super::*;
use crate::domain::classify_cwd;

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
            planning_intent_generation: 0,
            local_edit_revision: 0,
            pending_local_edit: None,
            planning_reconcile_count: 0,
            threads,
            thread_index_by_id,
            thread_indices_by_cwd,
            selected: 0,
            view: View::Registry,
            previous_target: None,
            quick_prompt_return_view: None,
            hot_slot_return_selected: None,
            watched_conversations: BTreeSet::new(),
            unregistered_watches: BTreeSet::new(),
            active_history_watches: BTreeSet::new(),
            thread_ui: BTreeMap::new(),
            thread_viewport_size: crate::conversation_viewport::Size::new((78, 12)),
            reading_snapshot: None,
            prompt_submissions: Default::default(),
            local_pins,
            local_aliases,
            local_marked_unread,
            operator_local: Default::default(),
            workref_link_intent: None,
            workref_link_pending: false,
            workref_ledger: Arc::new(WorkRefLedger::default()),
            workref_unavailable: false,
            conversations: BTreeMap::new(),
            conversation_cache_order: VecDeque::new(),
            git_contexts: BTreeMap::new(),
            cwd_localities: BTreeMap::new(),
            forge_observations: BTreeMap::new(),
            git_reviews: BTreeMap::new(),
            git_review_cache_order: VecDeque::new(),
            planning_snapshot: PlanningSnapshot::default(),
            work_cards: vec![],
            session_attention_policy: Default::default(),
            work_card_by_thread: BTreeMap::new(),
            worktree_collision_counts: BTreeMap::new(),
            goals: BTreeMap::new(),
            goal_checked: BTreeSet::new(),
            goal_actions_open: false,
            pending_local_batch: None,
            terminal_drawer_open: false,
            terminal_focused: false,
            terminal_snapshot: None,
            mutation_notice: None,
            recent_notification: None,
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
            target_lookup: None,
            target_lookup_generation: 0,
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
            request_details: None,
            pending_request_threads: BTreeSet::new(),
            user_input_editor: None,
            user_responses: Default::default(),
            show_help: false,
            help_top: 0,
            help_max_top: help_scroll::Limit::new(HELP_ENTRY_COUNT - 1),
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
            composer_cursor: 0,
            composer_scroll_back: 0,
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

    /// Indexed membership check for read-only notification routing.
    /// Validate the indexed item instead of trusting a stale thread-ID mapping.
    /// A recent alert is a navigation hint only when the main work surface
    /// owns the keyboard. Share this policy with rendering and the reducer so
    /// transient modals cannot hide a view change or consume an alert.
    pub fn recent_alert_controls_available(&self) -> bool {
        self.input_mode == InputMode::Normal
            && !self.should_quit
            && !self.terminal_focused
            && !self.request_details_open()
            && !self.show_help
            && !self.command_palette_open
            && self.target_lookup.is_none()
            && !self.context_open
            && !self.transcript_search_open
            && !self.goal_actions_open
            && self.saved_view_editor.is_none()
            && self.pending_local_batch.is_none()
            && self.hot_slot_bind_pending.is_none()
    }

    pub fn contains_thread_id(&self, thread_id: &ThreadId) -> bool {
        self.thread_index_by_id
            .get(&thread_id.0)
            .and_then(|index| self.threads.get(*index))
            .is_some_and(|thread| thread.id == *thread_id)
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
            View::Thread(thread_id) => {
                self.thread_by_id(thread_id).is_some()
                    || self.unregistered_watches.contains(&thread_id.0)
            }
            View::Review(thread_id) | View::Workspace(thread_id) => {
                self.thread_by_id(thread_id).is_some()
            }
            View::Registry | View::Board | View::Scratch(_) => true,
        }
    }

    pub(super) fn thread_id_for_view(view: &View) -> Option<ThreadId> {
        match view {
            View::Thread(thread_id) | View::Review(thread_id) | View::Workspace(thread_id) => {
                Some(thread_id.clone())
            }
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
            View::Board => ViewKind::Board,
            View::Scratch(_) => ViewKind::Scratch,
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
            View::Thread(id) | View::Review(id) | View::Workspace(id) => Some(id),
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
            View::Thread(id) | View::Review(id) | View::Workspace(id) => {
                Some(SourceRef::codex_thread(id))
            }
            View::Scratch(id) => Some(SourceRef {
                kind: SourceKind::ScratchWork,
                value: id.clone(),
            }),
        }
    }

    pub fn terminal_target_cwd(&self) -> Result<String, String> {
        if self.app_server_target_is_remote() {
            return Err(local_text(
                self.language,
                "terminal unavailable: this App Server target has no verified local host identity",
                "终端不可用：当前 App Server 目标没有经过验证的本地主机身份",
            )
            .into());
        }
        let thread_id = match &self.view {
            View::Registry => self.selected_thread_id(),
            View::Thread(id) | View::Review(id) | View::Workspace(id) => Some(id.clone()),
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
        let cwd = thread.metadata.cwd.as_str();
        match self.cwd_locality(cwd) {
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
            if target.kind == SourceKind::CodexThread && self.workref_link_snapshot().is_some() {
                choices.push(ContextChoice::LinkWorkItem);
            }
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
        if context.error.is_some()
            || self
                .thread_by_id(thread_id)
                .is_none_or(|thread| thread.metadata.cwd != context.cwd)
        {
            return 0;
        }
        let Some(worktree) = context.worktree.as_ref() else {
            return 0;
        };
        self.git_contexts
            .values()
            .filter(|other| {
                other.thread_id != *thread_id
                    && other.error.is_none()
                    && self.thread_by_id(&other.thread_id).is_some_and(|thread| {
                        thread.metadata.cwd == other.cwd
                            && matches!(
                                thread.runtime,
                                RuntimeStatus::Working | RuntimeStatus::WaitingHuman
                            )
                    })
                    && other.worktree.as_ref().is_some_and(|other| {
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

    pub(crate) fn planning_reconcile_count(&self) -> usize {
        self.planning_reconcile_count
    }
}
