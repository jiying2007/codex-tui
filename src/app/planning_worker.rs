//! Metadata-only projection; retired results travel with the next single in-flight input.
use super::*;
mod clock;
mod diagnostics;
pub(crate) use diagnostics::{measure_once, measure_rebase_once};
mod rebase;
use clock::ProjectionClock;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
struct Input {
    retired: Option<Output>,
    generation: u64,
    intent_generation: u64,
    threads: Vec<ThreadSummary>,
    git: BTreeMap<String, GitContext>,
    forge: BTreeMap<String, ForgeObservation>,
    goals: BTreeMap<String, GoalObservation>,
    local: PlanningSnapshot,
    backend: BackendStatus,
    now: u64,
}
impl Input {
    fn capture(state: &AppState, now: u64) -> Self {
        Self {
            retired: None,
            generation: state.planning_generation,
            intent_generation: state.planning_intent_generation,
            threads: state.threads.clone(),
            git: state.git_contexts.clone(),
            forge: state.forge_observations.clone(),
            goals: state.goals.clone(),
            local: state.planning_snapshot.clone(),
            backend: state.backend_status.clone(),
            now,
        }
    }
    fn compute(self) -> Output {
        drop(self.retired); // Reclaim the previous result on the planning worker.
        let mut state = AppState::new(self.threads);
        state.git_contexts = self.git;
        state.forge_observations = self.forge;
        state.goals = self.goals;
        state.planning_snapshot = self.local;
        state.backend_status = self.backend;
        rebuild_planning(&mut state, self.now);
        Output {
            clock: ProjectionClock::capture(&state.work_cards, self.now),
            generation: self.generation,
            cards: state.work_cards,
            by_thread: state.work_card_by_thread,
            collisions: state.worktree_collision_counts,
            source: rebase::Source {
                threads: state
                    .threads
                    .into_iter()
                    .map(|thread| (thread.id.0.clone(), thread))
                    .collect(),
                git: state.git_contexts,
                forge: state.forge_observations,
                goals: state.goals,
                local: state.planning_snapshot,
                backend: state.backend_status,
                intent_generation: self.intent_generation,
                now: self.now,
            },
        }
    }
}
struct Output {
    clock: ProjectionClock,
    source: rebase::Source,
    generation: u64,
    cards: Vec<WorkCardProjection>,
    by_thread: BTreeMap<String, usize>,
    collisions: BTreeMap<String, usize>,
}
impl Output {
    #[cfg(test)]
    fn apply(mut self, state: &mut AppState) -> bool {
        let now = self.source.now;
        self.apply_at(state, now).is_some()
    }
    fn apply_at(&mut self, state: &mut AppState, now: u64) -> Option<ProjectionClock> {
        if self.generation != state.planning_generation && !self.rebase(state) {
            return None;
        }
        let clock = self.clock;
        if clock.expired(now) {
            return None;
        }
        let selection = planning_selection::PlanningSelection::capture(state);
        state.planning_reconcile_count = state.planning_reconcile_count.saturating_add(1);
        std::mem::swap(&mut state.work_cards, &mut self.cards);
        std::mem::swap(&mut state.work_card_by_thread, &mut self.by_thread);
        std::mem::swap(&mut state.worktree_collision_counts, &mut self.collisions);
        selection.restore(state);
        ensure_selection_visible(state);
        Some(clock)
    }
}
pub struct PlanningWorker {
    send: SyncSender<Input>,
    receive: Receiver<Output>,
    retired: Option<Output>,
    in_flight: bool,
    completed: Option<u64>,
    clock: Option<ProjectionClock>,
}
impl PlanningWorker {
    pub fn start() -> std::io::Result<Self> {
        let (send, input) = mpsc::sync_channel::<Input>(1);
        let (output, receive) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("codex-tui-planning".into())
            .spawn(move || {
                while let Ok(input) = input.recv() {
                    if output.send(input.compute()).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            send,
            receive,
            retired: None,
            in_flight: false,
            completed: None,
            clock: None,
        })
    }
    /// One in-flight input and one retired result; source/intent/clock admission is unchanged.
    pub fn advance(&mut self, state: &mut AppState, dirty: bool, now: u64) -> Result<bool, String> {
        if dirty {
            state.planning_generation = state.planning_generation.wrapping_add(1);
        }
        let mut applied = false;
        match self.receive.try_recv() {
            Ok(mut output) => {
                self.in_flight = false;
                if let Some(clock) = output.apply_at(state, now) {
                    applied = true;
                    self.completed = Some(state.planning_generation);
                    self.clock = Some(clock);
                }
                debug_assert!(self.retired.is_none());
                self.retired = Some(output);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Err("planning worker stopped; previous projection retained".into());
            }
        }
        if !self.in_flight
            && (self.completed != Some(state.planning_generation)
                || self.clock.is_some_and(|clock| clock.expired(now)))
        {
            self.send
                .try_send(Input {
                    retired: self.retired.take(),
                    ..Input::capture(state, now)
                })
                .map_err(|_| {
                    "planning worker unavailable; previous projection retained".to_string()
                })?;
            self.in_flight = true;
        }
        Ok(applied)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};
    #[test]
    fn background_projection_matches_the_existing_authoritative_reconciler() {
        let mut live = AppState::new(FakeBackend::scaled(1000).snapshot().threads);
        let mut expected = live.clone();
        reduce(&mut expected, Action::ReconcilePlanning { now_unix_ms: 42 });
        assert!(Input::capture(&live, 42).compute().apply(&mut live));
        assert_eq!(live.work_cards, expected.work_cards);
        assert_eq!(live.work_card_by_thread, expected.work_card_by_thread);
        assert_eq!(
            live.worktree_collision_counts,
            expected.worktree_collision_counts
        );
    }
    #[test]
    fn obsolete_result_cannot_overwrite_newer_operator_or_source_state() {
        let mut live = AppState::new(FakeBackend::scaled(2).snapshot().threads);
        let old = Input::capture(&live, 1).compute();
        let (selected, was_pinned) = (live.selected, live.threads[live.selected].pinned);
        reduce(&mut live, Action::TogglePin);
        live.filter = "keep user filter".into();
        assert!(!old.apply(&mut live));
        assert!(live.work_cards.is_empty());
        assert_ne!(live.threads[selected].pinned, was_pinned);
        assert_eq!(live.filter, "keep user filter");
        assert!(Input::capture(&live, 2).compute().apply(&mut live));
        assert_eq!(live.filter, "keep user filter");
    }
}
#[cfg(test)]
#[path = "planning_worker/live_tests.rs"]
mod live_tests;
#[cfg(test)]
#[path = "planning_worker/rebase_tests.rs"]
mod rebase_tests;
