//! Single ordering contract shared by full planning and incremental rebases.
use crate::planning::WorkCardProjection;
use std::cmp::Ordering;

pub(super) fn compare_planning_cards(
    left: &WorkCardProjection,
    right: &WorkCardProjection,
) -> Ordering {
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
}

pub(super) fn sort_planning_cards(cards: &mut [WorkCardProjection]) {
    cards.sort_by(compare_planning_cards);
}
