use super::*;

impl AppState {
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

        // Bound the recent window in source order before changing its presentation.
        // Cache attention keys once rather than redoing projection lookups per comparison.
        visible.sort_by_cached_key(|&index| {
            let thread = &self.threads[index];
            (
                std::cmp::Reverse(self.thread_needs_attention(index)),
                std::cmp::Reverse(thread.pinned),
                std::cmp::Reverse(thread.metadata.updated_at),
                &thread.id.0,
            )
        });
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
}

pub(super) fn ensure_selection_visible(state: &mut AppState) {
    let visible = state.visible_indices();
    if visible.is_empty() {
        state.selected = 0;
    } else if !visible.contains(&state.selected) {
        state.selected = visible[0];
    }
}

pub(super) fn restore_selection(state: &mut AppState, selected_id: Option<ThreadId>) {
    state.selected = selected_id
        .and_then(|id| state.thread_index_by_id.get(&id.0).copied())
        .unwrap_or_else(|| state.visible_indices().first().copied().unwrap_or(0));
    ensure_selection_visible(state);
}

#[cfg(test)]
mod tests;
