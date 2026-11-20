use super::*;
use crate::planning::PlanningAttention;
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

    pub fn thread_needs_attention(&self, index: usize) -> bool {
        let Some(thread) = self.threads.get(index) else {
            return false;
        };
        // The reconciliation index makes the full Registry pass O(T log C).
        // Live requests below remain actionable while that projection catches up.
        let card = self
            .work_card_by_thread
            .get(&thread.id.0)
            .and_then(|index| self.work_cards.get(*index));
        let snoozed = card.is_some_and(|card| card.snoozed);
        let actionable = thread.attention.iter().any(|reason| {
            matches!(
                reason,
                AttentionReason::ApprovalRequired
                    | AttentionReason::UserInputRequired
                    | AttentionReason::SystemError
            )
        });
        let interactive = self.pending_request_threads.contains(&thread.id.0);
        let derived_actionable = card.is_some_and(|card| {
            card.attention.iter().any(|reason| {
                matches!(
                    reason,
                    PlanningAttention::ConflictRisk
                        | PlanningAttention::GoalBlocked
                        | PlanningAttention::UsageLimited
                        | PlanningAttention::BudgetLimited
                        | PlanningAttention::PipelineFailed
                        | PlanningAttention::ChangeRequested
                )
            })
        });
        let unseen = !thread.attention.is_empty()
            || card.is_some_and(|card| card.attention.contains(&PlanningAttention::ReviewUnseen));
        actionable
            || interactive
            || (!snoozed
                && (derived_actionable
                    || (unseen && !self.acknowledged_attention.contains(&thread.id.0))))
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
