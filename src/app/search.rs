use super::{
    AppState, Effect, InputMode, View, ensure_selection_visible, filter_requires_locality,
    fuzzy_subsequence, is_locality_filter_token, refresh_git_projections,
};
use crate::domain::{CwdLocality, ThreadSummary};
use crate::planning::{
    SourceRef, WorkCardProjection, apply_saved_view, card_matches_filter,
};

impl AppState {
    pub(super) fn thread_search_extra_fields(&self, thread: &ThreadSummary) -> Vec<String> {
        let mut fields = Vec::with_capacity(20);

        if let Some(card) = self.work_card_for_thread(&thread.id) {
            fields.push(card.title.clone());
            fields.push(card.stage.label().into());
            fields.extend(card.overlay.tags.iter().cloned());
            if let Some(note) = &card.overlay.note {
                fields.push(note.clone());
            }
            if let Some(branch) = &card.branch {
                fields.push(branch.clone());
            }
            if let Some(goal) = &card.goal {
                fields.push(goal.objective.clone());
                fields.push(goal.status.wire().into());
            }
            if let Some(state) = &card.change_request_state {
                fields.push(state.clone());
            }
            fields.extend(card.links.iter().map(|link| link.source.value.clone()));
        }

        let thread_ref = SourceRef::codex_thread(&thread.id);
        fields.extend(
            self.planning_snapshot
                .notes
                .iter()
                .filter(|note| note.owner == thread_ref)
                .map(|note| note.text.clone()),
        );

        if let Some(goal) = self.goals.get(&thread.id.0) {
            fields.push(goal.objective.clone());
            fields.push(goal.status.wire().into());
        }

        if let Some(git) = self.git_context(&thread.id) {
            if let Some(branch) = &git.branch {
                fields.push(branch.clone());
            }
            if let Some(head) = &git.head {
                fields.push(head.clone());
            }
            if let Some(upstream) = &git.upstream {
                fields.push(upstream.clone());
            }
            if let Some(worktree) = &git.worktree {
                fields.push(worktree.canonical_path.clone());
                if let Some(branch) = &worktree.branch {
                    fields.push(branch.clone());
                }
            }
        }

        if let Some(forge) = self.forge_observation(&thread.id) {
            if let Some(identity) = &forge.identity {
                fields.push(identity.host.clone());
                fields.push(identity.path_with_namespace.clone());
                fields.push(identity.provider.label().into());
            }
            if let Some(branch) = self
                .git_context(&thread.id)
                .and_then(|git| git.branch.as_deref())
            {
                if let Some(change) = forge.change_request_for_branch(branch) {
                    fields.push(change.title.clone());
                    fields.push(change.state.clone());
                    fields.push(change.source_branch.clone());
                    fields.push(change.target_branch.clone());
                }
                if let Some(pipeline) = forge.pipeline_for_branch(branch) {
                    fields.push(pipeline.status.clone());
                    fields.push(pipeline.reference.clone());
                }
            }
        }

        fields
    }

    pub fn planning_cards_for_active_view(&self) -> Vec<&WorkCardProjection> {
        let view = self.active_saved_view();
        let mut cards = apply_saved_view(&self.work_cards, &view);
        if !self.planning_filter.trim().is_empty() {
            cards.retain(|card| card_matches_filter(card, &self.planning_filter));
        }
        cards
    }

    pub(super) fn begin_metadata_search(&mut self) {
        self.search_planning = matches!(self.view, View::Board | View::Scratch(_));
        if self.search_planning {
            self.search_return_view =
                matches!(self.view, View::Scratch(_)).then(|| self.view.clone());
            if matches!(self.view, View::Scratch(_)) {
                self.view = View::Board;
            }
            self.input_original.clone_from(&self.planning_filter);
            self.input_buffer.clone_from(&self.planning_filter);
            self.board_selected = 0;
        } else {
            self.search_return_view = if matches!(self.view, View::Registry) {
                None
            } else {
                Some(self.view.clone())
            };
            if self.search_return_view.is_some() {
                self.view = View::Registry;
                ensure_selection_visible(self);
            }
            self.input_original.clone_from(&self.filter);
            self.input_buffer.clone_from(&self.filter);
        }
        self.input_mode = InputMode::Search;
    }

    pub(super) fn update_search_filter_from_buffer(&mut self) {
        if self.search_planning {
            self.planning_filter.clone_from(&self.input_buffer);
            self.board_selected = 0;
        } else {
            self.filter.clone_from(&self.input_buffer);
            if filter_requires_locality(&self.filter) {
                self.reconcile_cwd_locality_cache(true);
            }
            ensure_selection_visible(self);
        }
    }

    pub(super) fn commit_metadata_search(&mut self) -> Vec<Effect> {
        let planning_search = self.search_planning;
        let origin_thread_id = (!planning_search)
            .then(|| self.search_origin_thread_id())
            .flatten();

        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.input_original.clear();
        self.search_return_view = None;
        self.search_planning = false;

        if planning_search {
            return vec![];
        }

        let mut effects = refresh_git_projections(self);
        if let Some(thread_id) = origin_thread_id {
            effects.push(Effect::StopWatchingConversation(thread_id));
        }
        effects
    }

    pub(super) fn cancel_metadata_search(&mut self) -> Vec<Effect> {
        let planning_search = self.search_planning;
        let origin_thread_id = (!planning_search)
            .then(|| self.search_origin_thread_id())
            .flatten();
        let mut search_watch_to_release = None;

        if planning_search {
            self.planning_filter.clone_from(&self.input_original);
            self.board_selected = 0;
        } else {
            self.filter.clone_from(&self.input_original);
            ensure_selection_visible(self);
        }

        if let Some(return_view) = self.search_return_view.take()
            && self.search_return_view_is_valid(&return_view)
        {
            self.view = return_view;
        } else if self.search_return_view.is_some() {
            search_watch_to_release = origin_thread_id;
        }

        self.search_planning = false;
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.input_original.clear();

        search_watch_to_release
            .map(Effect::StopWatchingConversation)
            .into_iter()
            .collect()
    }
}

pub(super) fn matches_filter_normalized_with_extra(
    thread: &ThreadSummary,
    query: &str,
    locality: Option<CwdLocality>,
    extra_fields: &[String],
) -> bool {
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

    query.split_whitespace().all(|token| {
        if is_locality_filter_token(token) {
            locality.is_some_and(|locality| token == locality.label())
        } else {
            fields.iter().any(|field| fuzzy_subsequence(token, field))
                || extra_fields
                    .iter()
                    .any(|field| fuzzy_subsequence(token, &field.to_ascii_lowercase()))
        }
    })
}
