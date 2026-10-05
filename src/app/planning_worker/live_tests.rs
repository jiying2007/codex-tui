use super::*;
use crate::backend::{CodexBackend, FakeBackend};
use crate::planning::{Freshness, LocalNote, WorkCardRecord};
use std::time::Duration;

fn app() -> AppState {
    let mut state = AppState::new(FakeBackend::scaled(20).snapshot().threads);
    let mut view = builtin_saved_views().remove(0);
    view.id = "custom:all".into();
    view.layout = SavedViewLayout::List;
    state.planning_snapshot.saved_views.push(view);
    state.planning_view_index = builtin_saved_views().len();
    state.view = View::Board;
    rebuild_planning(&mut state, 100);
    state
}
fn settle(worker: &mut PlanningWorker, app: &mut AppState, now: u64) {
    let started = Instant::now();
    loop {
        if worker.advance(app, false, now).unwrap() {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "planning did not converge"
        );
        std::thread::yield_now();
    }
}
#[test]
fn local_snapshot_refresh_keeps_current_board_selection() {
    let mut live = app();
    live.board_selected = 7;
    let anchor = live.selected_planning_card().unwrap().anchor.clone();
    let mut snapshot = live.planning_snapshot.clone();
    snapshot.notes.push(LocalNote {
        owner: anchor.clone(),
        text: "saved".into(),
        updated_at_unix_ms: 1,
    });
    reduce(&mut live, Action::PlanningSnapshotLoaded(snapshot));
    assert_eq!(live.selected_planning_card().unwrap().anchor, anchor);
}
#[test]
fn snapshot_view_reorder_preserves_saved_view_identity() {
    let mut live = app();
    let selected_view = live.active_saved_view().id;
    let mut snapshot = live.planning_snapshot.clone();
    let mut inserted = builtin_saved_views().remove(0);
    inserted.id = "custom:inserted".into();
    snapshot.saved_views.insert(0, inserted);
    reduce(&mut live, Action::PlanningSnapshotLoaded(snapshot));
    assert_eq!(live.active_saved_view().id, selected_view);
}
#[test]
fn asynchronous_projection_preserves_selection_at_install_not_capture() {
    let mut live = app();
    live.board_selected = 1;
    let mut fresh = live.threads.clone();
    fresh[0].title = "ZZZ reordered".into();
    reduce(&mut live, Action::ReplaceThreads(fresh));
    let output = Input::capture(&live, 101).compute();
    live.board_selected = 7;
    let anchor = live.selected_planning_card().unwrap().anchor.clone();
    assert!(output.apply(&mut live));
    assert_eq!(live.selected_planning_card().unwrap().anchor, anchor);
}
#[test]
fn snooze_expires_without_registry_or_git_events() {
    let mut live = app();
    let id = live.threads[0].id.clone();
    let mut record = WorkCardRecord::implicit_thread(&id);
    record.overlay.snooze_until_unix_ms = Some(150);
    live.planning_snapshot.cards.push(record);
    let mut worker = PlanningWorker::start().unwrap();
    settle(&mut worker, &mut live, 100);
    assert!(live.work_card_for_thread(&id).unwrap().snoozed);
    worker.advance(&mut live, false, 150).unwrap();
    assert!(
        worker.in_flight,
        "expiry must schedule a refresh without external events"
    );
    settle(&mut worker, &mut live, 150);
    assert!(!live.work_card_for_thread(&id).unwrap().snoozed);
}
#[test]
fn cached_provenance_ages_without_external_events() {
    let mut live = app();
    live.backend_status.last_refresh_unix_ms = Some(100);
    let id = live.threads[0].id.clone();
    let mut worker = PlanningWorker::start().unwrap();
    settle(&mut worker, &mut live, 100);
    assert_eq!(
        live.work_card_for_thread(&id).unwrap().provenance[0].freshness,
        Freshness::Fresh
    );
    worker.advance(&mut live, false, 10_101).unwrap();
    assert!(worker.in_flight, "freshness expiry must schedule a refresh");
    settle(&mut worker, &mut live, 10_101);
    assert_eq!(
        live.work_card_for_thread(&id).unwrap().provenance[0].freshness,
        Freshness::Aging
    );
}

#[test]
fn idle_polls_do_not_rebuild_an_unexpired_projection() {
    let mut live = app();
    let mut worker = PlanningWorker::start().unwrap();
    settle(&mut worker, &mut live, 100);
    let before = live.planning_reconcile_count();
    for now in 101..200 {
        assert!(!worker.advance(&mut live, false, now).unwrap());
        assert!(!worker.in_flight);
    }
    assert_eq!(live.planning_reconcile_count(), before);
}

#[test]
fn clock_rollback_recomputes_expired_snooze_without_external_events() {
    let mut live = app();
    let id = live.threads[0].id.clone();
    let mut record = WorkCardRecord::implicit_thread(&id);
    record.overlay.snooze_until_unix_ms = Some(150);
    live.planning_snapshot.cards.push(record);
    let mut worker = PlanningWorker::start().unwrap();
    settle(&mut worker, &mut live, 200);
    assert!(!live.work_card_for_thread(&id).unwrap().snoozed);
    worker.advance(&mut live, false, 100).unwrap();
    assert!(worker.in_flight);
    settle(&mut worker, &mut live, 100);
    assert!(live.work_card_for_thread(&id).unwrap().snoozed);
}

#[test]
fn a_result_that_expired_in_transit_is_not_installed() {
    let mut live = app();
    let id = live.threads[0].id.clone();
    let mut record = WorkCardRecord::implicit_thread(&id);
    record.overlay.snooze_until_unix_ms = Some(150);
    live.planning_snapshot.cards.push(record);
    let output = Input::capture(&live, 100).compute();
    let (send, input) = mpsc::sync_channel(1);
    let (result, receive) = mpsc::sync_channel(1);
    result.send(output).unwrap();
    let mut worker = PlanningWorker {
        send,
        receive,
        in_flight: true,
        completed: None,
        clock: None,
    };
    assert!(!worker.advance(&mut live, false, 200).unwrap());
    assert!(worker.in_flight);
    let replacement = input.try_recv().unwrap().compute();
    result.send(replacement).unwrap();
    assert!(worker.advance(&mut live, false, 200).unwrap());
    assert!(!live.work_card_for_thread(&id).unwrap().snoozed);
}

#[test]
fn removing_selected_card_or_view_clamps_without_a_phantom_selection() {
    let mut live = app();
    live.board_selected = 19;
    let only = live.threads[0].clone();
    reduce(&mut live, Action::ReplaceThreads(vec![only]));
    assert!(Input::capture(&live, 100).compute().apply(&mut live));
    assert_eq!(live.board_selected, 0);
    assert!(live.selected_planning_card().is_some());
    reduce(
        &mut live,
        Action::PlanningSnapshotLoaded(PlanningSnapshot::default()),
    );
    assert_eq!(live.active_saved_view().id, "builtin:all");
    reduce(&mut live, Action::ReplaceThreads(vec![]));
    assert!(Input::capture(&live, 100).compute().apply(&mut live));
    assert_eq!(live.board_selected, 0);
    assert!(live.selected_planning_card().is_none());
}
