//! Latest-generation planning projection. Only metadata is copied; transcripts,
//! PTY state, drafts and mutation confirmations never enter this worker.
use super::*;
mod clock;
use clock::ProjectionClock;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};

struct Input {
    generation: u64,
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
            generation: state.planning_generation,
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
        }
    }
}
struct Output {
    clock: ProjectionClock,
    generation: u64,
    cards: Vec<WorkCardProjection>,
    by_thread: BTreeMap<String, usize>,
    collisions: BTreeMap<String, usize>,
}
impl Output {
    fn apply(self, state: &mut AppState) -> bool {
        if self.generation != state.planning_generation {
            return false;
        }
        let selection = planning_selection::PlanningSelection::capture(state);
        state.planning_reconcile_count = state.planning_reconcile_count.saturating_add(1);
        state.work_cards = self.cards;
        state.work_card_by_thread = self.by_thread;
        state.worktree_collision_counts = self.collisions;
        selection.restore(state);
        ensure_selection_visible(state);
        true
    }
}

pub struct PlanningWorker {
    send: SyncSender<Input>,
    receive: Receiver<Output>,
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
            in_flight: false,
            completed: None,
            clock: None,
        })
    }
    /// One job in flight. Changes coalesce without queuing or cloning more snapshots.
    /// A result from an older generation is never installed into current state.
    pub fn advance(&mut self, state: &mut AppState, dirty: bool, now: u64) -> Result<bool, String> {
        if dirty {
            state.planning_generation = state.planning_generation.wrapping_add(1);
        }
        let mut applied = false;
        match self.receive.try_recv() {
            Ok(output) => {
                self.in_flight = false;
                let generation = output.generation;
                let clock = output.clock;
                // A delayed result may already be obsolete by time even when its
                // data generation still matches. Do not flash an expired projection.
                if !clock.expired(now) {
                    applied = output.apply(state);
                }
                if applied {
                    self.completed = Some(generation);
                    self.clock = Some(clock);
                }
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
                .try_send(Input::capture(state, now))
                .map_err(|_| {
                    "planning worker unavailable; previous projection retained".to_string()
                })?;
            self.in_flight = true;
        }
        Ok(applied)
    }
}

pub(crate) fn measure_once(state: &mut AppState, now: u64) -> [f64; 3] {
    let started = Instant::now();
    let input = Input::capture(state, now);
    let capture = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let output = input.compute();
    let compute = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    assert!(output.apply(state));
    [capture, compute, started.elapsed().as_secs_f64() * 1000.0]
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
        let selected = live.selected;
        let was_pinned = live.threads[selected].pinned;
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
