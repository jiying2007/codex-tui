//! Real planning worker, synthetic metadata only; no account, process or RPC.
use codex_tui::{
    app::{Action, AppState, planning_worker::PlanningWorker, reduce},
    backend::{CodexBackend, FakeBackend},
};
use std::time::{Duration, Instant};

fn settle(worker: &mut PlanningWorker, app: &mut AppState) {
    let started = Instant::now();
    loop {
        if worker.advance(app, false, 100).unwrap() {
            return;
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn retired_results_do_not_replace_current_authority_or_trigger_idle_work() {
    for rows in [20, 12_000] {
        let mut app = AppState::new(FakeBackend::scaled(rows).snapshot().threads);
        let id = app.threads[0].id.clone();
        let mut worker = PlanningWorker::start().unwrap();
        settle(&mut worker, &mut app);
        for round in 0..4 {
            let mut threads = app.threads.clone();
            threads[0].title = format!("in-flight-{round}");
            reduce(&mut app, Action::ReplaceThreads(threads));
            worker.advance(&mut app, false, 100).unwrap();
            let mut current = app.threads.clone();
            let title = format!("current-{round}");
            current[0].title = title.clone();
            reduce(&mut app, Action::ReplaceThreads(current));
            settle(&mut worker, &mut app);
            assert_eq!(app.work_card_for_thread(&id).unwrap().title, title);
            assert_eq!(app.work_cards.len(), rows);
            let reconciles = app.planning_reconcile_count();
            for _ in 0..32 {
                assert!(!worker.advance(&mut app, false, 100).unwrap());
            }
            assert_eq!(app.planning_reconcile_count(), reconciles);
        }
    }
}

#[test]
fn removing_all_threads_does_not_resurrect_retired_cards() {
    let mut app = AppState::new(FakeBackend::scaled(100).snapshot().threads);
    let mut worker = PlanningWorker::start().unwrap();
    settle(&mut worker, &mut app);
    assert_eq!(app.work_cards.len(), 100);
    reduce(&mut app, Action::ReplaceThreads(vec![]));
    settle(&mut worker, &mut app);
    assert!(app.work_cards.is_empty());
    for _ in 0..32 {
        assert!(!worker.advance(&mut app, false, 100).unwrap());
        assert!(app.work_cards.is_empty());
    }
}
