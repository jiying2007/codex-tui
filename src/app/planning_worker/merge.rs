//! Merge at most 64 changed cards into an already sorted projection.
//! A full 50k-row resort in the UI event loop is not proportional to the delta.
use super::*;
use std::collections::{BTreeMap, HashMap};

pub(super) fn install(
    cards: &mut Vec<WorkCardProjection>,
    updates: BTreeMap<String, WorkCardProjection>,
    active_threads: &HashMap<String, usize>,
) {
    if updates.is_empty() && cards.iter().all(|card| {
        card.anchor.kind != SourceKind::CodexThread
            || active_threads.contains_key(&card.anchor.value)
    }) {
        return;
    }
    let mut retained = std::mem::take(cards);
    retained.retain(|card| {
        card.anchor.kind != SourceKind::CodexThread
            || (active_threads.contains_key(&card.anchor.value)
                && !updates.contains_key(&card.anchor.value))
    });
    let mut changes = updates.into_values().collect::<Vec<_>>();
    sort_planning_cards(&mut changes);
    let mut old = retained.into_iter().peekable();
    let mut new = changes.into_iter().peekable();
    let mut merged = Vec::with_capacity(old.len() + new.len());
    while let (Some(before), Some(after)) = (old.peek(), new.peek()) {
        if compare_planning_cards(before, after).is_le() {
            merged.push(old.next().expect("peeked existing card"));
        } else {
            merged.push(new.next().expect("peeked updated card"));
        }
    }
    merged.extend(old);
    merged.extend(new);
    *cards = merged;
}
