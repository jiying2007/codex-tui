//! Preserve current operator selection across asynchronously refreshed data.
use super::*;

pub(super) struct PlanningSelection {
    view_id: String,
    card: Option<(String, SourceRef)>,
    index: usize,
}
impl PlanningSelection {
    pub(super) fn capture(state: &AppState) -> Self {
        Self {
            view_id: state.active_saved_view().id,
            card: if matches!(state.view, View::Board) {
                state
                    .selected_planning_card()
                    .map(|card| (card.local_id.clone(), card.anchor.clone()))
            } else {
                None
            },
            index: state.board_selected,
        }
    }
    pub(super) fn restore(self, state: &mut AppState) {
        state.planning_view_index = state
            .planning_views()
            .iter()
            .position(|view| view.id == self.view_id)
            .unwrap_or(0);
        if matches!(state.view, View::Board) {
            let visible = state.visible_planning_cards();
            state.board_selected = self
                .card
                .and_then(|(id, anchor)| {
                    visible
                        .iter()
                        .position(|card| card.local_id == id && card.anchor == anchor)
                })
                .unwrap_or(self.index.min(visible.len().saturating_sub(1)));
        }
    }
}
