use super::*;
use crate::backend::{CodexBackend, FakeBackend};
use crate::planning::WorkCardRecord;
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

// Deterministic handoff: the test owns both worker channels and advances one job.
fn harness() -> (PlanningWorker, Receiver<Job>, SyncSender<Option<Output>>) {
    let (send, jobs) = mpsc::sync_channel(1);
    let (completed, receive) = mpsc::sync_channel(1);
    (
        PlanningWorker {
            send,
            receive,
            retired: None,
            in_flight: false,
            completed: None,
            clock: None,
        },
        jobs,
        completed,
    )
}

fn install(
    worker: &mut PlanningWorker,
    live: &mut AppState,
    jobs: &Receiver<Job>,
    completed: &SyncSender<Option<Output>>,
) {
    assert!(!worker.advance(live, false, 100).unwrap());
    let projected = jobs.try_recv().unwrap().execute();
    assert!(projected.is_some());
    completed.send(projected).unwrap();
    assert!(worker.advance(live, false, 100).unwrap());
}

#[test]
fn idle_result_is_reclaimed_without_another_projection_or_snapshot() {
    let mut live = AppState::new(FakeBackend::scaled(20).snapshot().threads);
    let (mut worker, jobs, completed) = harness();
    install(&mut worker, &mut live, &jobs, &completed);
    assert!(worker.retired.is_none());
    assert!(worker.in_flight);
    let Job::Reclaim(retired) = jobs.try_recv().unwrap() else {
        panic!("idle completion must enqueue only reclamation");
    };
    assert_eq!(retired.source.threads.len(), 20);
    let cards = live.work_cards.clone();
    let count = live.planning_reconcile_count();
    completed.send(Job::Reclaim(retired).execute()).unwrap();
    assert!(!worker.advance(&mut live, false, 100).unwrap());
    assert!(!worker.in_flight);
    assert!(worker.retired.is_none());
    assert_eq!(live.work_cards, cards);
    assert_eq!(live.planning_reconcile_count(), count);
    assert!(matches!(jobs.try_recv(), Err(TryRecvError::Empty)));
}

#[test]
fn updates_during_reclamation_coalesce_into_one_current_snapshot() {
    let mut live = AppState::new(FakeBackend::scaled(20).snapshot().threads);
    let id = live.threads[0].id.0.clone();
    let (mut worker, jobs, completed) = harness();
    install(&mut worker, &mut live, &jobs, &completed);
    let retirement = jobs.try_recv().unwrap();
    for revision in 1..=5 {
        let mut threads = live.threads.clone();
        let current = threads.iter_mut().find(|thread| thread.id.0 == id).unwrap();
        current.title = format!("current-{revision}");
        reduce(&mut live, Action::ReplaceThreads(threads));
        assert!(!worker.advance(&mut live, true, 100).unwrap());
        assert!(matches!(jobs.try_recv(), Err(TryRecvError::Empty)));
    }
    completed.send(retirement.execute()).unwrap();
    assert!(!worker.advance(&mut live, false, 100).unwrap());
    let input = jobs.try_recv().unwrap().into_projection();
    assert_eq!(input.generation, live.planning_generation);
    let current = input.threads.iter().find(|t| t.id.0 == id).unwrap();
    assert_eq!(current.title, "current-5");
    assert!(input.retired.is_none());
    assert!(matches!(jobs.try_recv(), Err(TryRecvError::Empty)));
    completed.send(Some(input.compute())).unwrap();
    assert!(worker.advance(&mut live, false, 100).unwrap());
    assert_eq!(
        live.work_cards[live.work_card_by_thread[&id]].title,
        "current-5"
    );
}

#[test]
fn reclamation_ack_does_not_hide_an_expired_snooze() {
    let mut live = AppState::new(FakeBackend::scaled(20).snapshot().threads);
    let id = live.threads[0].id.clone();
    let mut record = WorkCardRecord::implicit_thread(&id);
    record.overlay.snooze_until_unix_ms = Some(150);
    live.planning_snapshot.cards.push(record);
    let (mut worker, jobs, completed) = harness();
    install(&mut worker, &mut live, &jobs, &completed);
    assert!(live.work_card_for_thread(&id).unwrap().snoozed);
    let retirement = jobs.try_recv().unwrap();
    assert!(!worker.advance(&mut live, false, 200).unwrap());
    completed.send(retirement.execute()).unwrap();
    assert!(!worker.advance(&mut live, false, 200).unwrap());
    let input = jobs.try_recv().unwrap().into_projection();
    assert_eq!(input.now, 200);
    completed.send(Some(input.compute())).unwrap();
    assert!(worker.advance(&mut live, false, 200).unwrap());
    assert!(!live.work_card_for_thread(&id).unwrap().snoozed);
}

#[test]
fn disconnected_reclamation_keeps_the_installed_projection() {
    let mut live = AppState::new(FakeBackend::scaled(20).snapshot().threads);
    let (mut worker, jobs, completed) = harness();
    install(&mut worker, &mut live, &jobs, &completed);
    let cards = live.work_cards.clone();
    drop(jobs);
    drop(completed);
    assert!(worker.advance(&mut live, true, 100).is_err());
    assert_eq!(live.work_cards, cards);
}

#[test]
fn live_worker_finishes_idle_reclamation_without_a_timer_or_new_input() {
    let mut live = AppState::new(FakeBackend::scaled(1000).snapshot().threads);
    let mut worker = PlanningWorker::start().unwrap();
    let started = Instant::now();
    let mut installations = 0;
    loop {
        installations += usize::from(worker.advance(&mut live, false, 100).unwrap());
        if installations == 1 && !worker.in_flight {
            break;
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        std::thread::yield_now();
    }
    assert!(worker.retired.is_none());
    let count = live.planning_reconcile_count();
    for _ in 0..50 {
        assert!(!worker.advance(&mut live, false, 100).unwrap());
        assert!(!worker.in_flight);
    }
    assert_eq!(live.planning_reconcile_count(), count);
}
