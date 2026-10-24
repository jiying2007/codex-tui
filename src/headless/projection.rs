//! Snapshot-local indexing; local metadata never becomes another runtime authority.
use crate::{
    domain::ThreadSummary,
    planning::{
        PlanningSnapshot, ReconcileInput, SourceKind, SourceRef, WorkCardProjection,
        reconcile_scratch_card_with_local, reconcile_thread_card,
    },
};
use std::collections::BTreeMap;

pub(crate) fn reconcile_work(
    threads: &[ThreadSummary],
    planning: &PlanningSnapshot,
    observed_at: Option<u64>,
    error: Option<&str>,
    now: u64,
) -> Vec<WorkCardProjection> {
    let mut local_by_anchor = BTreeMap::new();
    for card in &planning.cards {
        // Preserve the original first-match semantics even for duplicate input.
        local_by_anchor.entry(&card.anchor).or_insert(card);
    }
    let mut projections = Vec::with_capacity(threads.len() + planning.scratch.len());
    for thread in threads {
        let anchor = SourceRef::codex_thread(&thread.id);
        projections.push(reconcile_thread_card(ReconcileInput {
            thread,
            git: None,
            local: local_by_anchor.get(&anchor).copied(),
            collision_count: 0,
            backend_observed_at_unix_ms: observed_at,
            backend_error: error,
            now_unix_ms: now,
        }));
    }
    for scratch in &planning.scratch {
        let anchor = SourceRef {
            kind: SourceKind::ScratchWork,
            value: scratch.id.clone(),
        };
        projections.push(reconcile_scratch_card_with_local(
            scratch,
            local_by_anchor.get(&anchor).copied(),
            now,
        ));
    }
    projections
}
