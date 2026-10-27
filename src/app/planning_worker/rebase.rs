//! Rebase small source deltas; never install stale authority or operator intent.
//! Beyond 10k threads defer to a fresh worker pass instead of blocking the UI.
use super::*;
const MAX_REBASED_CARDS: usize = 64;

pub(super) struct Source {
    pub threads: HashMap<String, ThreadSummary>,
    pub git: BTreeMap<String, GitContext>,
    pub forge: BTreeMap<String, ForgeObservation>,
    pub goals: BTreeMap<String, GoalObservation>,
    pub local: PlanningSnapshot,
    pub backend: BackendStatus,
    pub intent_generation: u64,
    pub now: u64,
}

impl Output {
    pub(super) fn rebase(&mut self, state: &AppState) -> bool {
        // Remote capabilities, local overlays and operator intent are not guessed
        // from old cards. A change in these domains needs a fresh full snapshot.
        if state.threads.len() > 10_000
            || self.source.intent_generation != state.planning_intent_generation
            || self.source.git != state.git_contexts
            || self.source.forge != state.forge_observations
            || self.source.goals != state.goals
            || self.source.local != state.planning_snapshot
            || self.source.backend != state.backend_status
        {
            return false;
        }
        let removed = self
            .source
            .threads
            .keys()
            .filter(|id| !state.thread_index_by_id.contains_key(*id))
            .count();
        if removed > MAX_REBASED_CARDS {
            return false;
        }
        let active = active_worktree_counts(state);
        let mut changed = Vec::new();
        let mut collisions = BTreeMap::new();
        for thread in &state.threads {
            let collision = collision_count_from_active_worktrees(state, thread, &active);
            if self.source.threads.get(&thread.id.0) != Some(thread)
                || self.collisions.get(&thread.id.0) != Some(&collision)
            {
                if changed.len() + removed == MAX_REBASED_CARDS {
                    return false;
                }
                changed.push(thread);
            }
            collisions.insert(thread.id.0.clone(), collision);
        }
        let local = state
            .planning_snapshot
            .cards
            .iter()
            .map(|card| (&card.anchor, card))
            .collect::<BTreeMap<_, _>>();
        let notes = state
            .planning_snapshot
            .notes
            .iter()
            .map(|note| (&note.owner, note))
            .collect::<BTreeMap<_, _>>();
        for thread in changed {
            let anchor = SourceRef::codex_thread(&thread.id);
            let mut card = reconcile_thread_card_with_goal_and_forge(
                ReconcileInput {
                    thread,
                    git: state.git_context(&thread.id),
                    local: local.get(&anchor).copied(),
                    collision_count: collisions[&thread.id.0],
                    backend_observed_at_unix_ms: state.backend_status.last_refresh_unix_ms,
                    backend_error: state.backend_status.error.as_deref(),
                    now_unix_ms: self.source.now,
                },
                state.goals.get(&thread.id.0),
                state.forge_observation(&thread.id),
            );
            if let Some(note) = notes.get(&anchor) {
                card.overlay.note = Some(note.text.clone());
            }
            if let Some(index) = self.by_thread.get(&thread.id.0) {
                self.cards[*index] = card;
            } else {
                self.cards.push(card);
            }
        }
        self.cards.retain(|card| {
            card.anchor.kind != SourceKind::CodexThread
                || state.thread_index_by_id.contains_key(&card.anchor.value)
        });
        sort_planning_cards(&mut self.cards);
        self.by_thread = self
            .cards
            .iter()
            .enumerate()
            .filter(|(_, card)| card.anchor.kind == SourceKind::CodexThread)
            .map(|(index, card)| (card.anchor.value.clone(), index))
            .collect();
        self.collisions = collisions;
        self.clock = ProjectionClock::capture(&self.cards, self.source.now);
        self.generation = state.planning_generation;
        true
    }
}
