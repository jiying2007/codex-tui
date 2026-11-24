use super::*;
mod groups;

impl AppState {
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

    /// Live requests lacking an authoritative thread are visible but not navigable.
    pub fn unlocated_pending_request_count(&self) -> usize {
        self.pending_requests
            .iter()
            .filter(|request| !self.thread_index_by_id.contains_key(&request.thread_id.0))
            .count()
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
