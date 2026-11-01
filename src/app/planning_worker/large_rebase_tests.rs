//! A bounded 20k watch may change while the planning worker is running.
//! Recompute only verified small source deltas; fail closed on broad drift.
use super::*;
use crate::backend::{CodexBackend, FakeBackend};

fn fixture(rows: usize) -> AppState {
    AppState::new(FakeBackend::scaled(rows).snapshot().threads)
}

#[test]
fn large_sparse_watch_drift_installs_current_target_and_matches_fresh_projection() {
    let mut live = fixture(20_000);
    let old = Input::capture(&live, 100).compute();
    let mut threads = live.threads.clone();
    let target = threads[0].id.clone();
    threads[0].title = "LATEST_TARGET".into();
    for (index, thread) in threads.iter_mut().enumerate().skip(1).take(30) {
        thread.title = format!("noise-{index}");
    }
    reduce(&mut live, Action::ReplaceThreads(threads));
    let mut expected = live.clone();
    rebuild_planning(&mut expected, 100);

    assert!(
        old.apply(&mut live),
        "bounded 20k drift must be reconcilable"
    );
    assert_eq!(live.work_cards, expected.work_cards);
    assert_eq!(live.work_card_by_thread, expected.work_card_by_thread);
    assert_eq!(
        live.worktree_collision_counts,
        expected.worktree_collision_counts
    );
    assert_eq!(
        live.work_cards[live.work_card_by_thread[&target.0]].title,
        "LATEST_TARGET"
    );
}

#[test]
fn large_broad_watch_drift_remains_rejected_without_partial_install() {
    let mut live = fixture(20_000);
    let old = Input::capture(&live, 100).compute();
    let mut threads = live.threads.clone();
    for thread in threads.iter_mut().take(65) {
        thread.title.push_str(" changed");
    }
    reduce(&mut live, Action::ReplaceThreads(threads));

    assert!(
        !old.apply(&mut live),
        "65 changes exceed the bounded delta budget"
    );
    assert!(
        live.work_cards.is_empty(),
        "no stale or partial result installed"
    );
}

#[test]
fn large_operator_aba_cannot_be_hidden_by_source_rebase() {
    let mut live = fixture(12_000);
    let old = Input::capture(&live, 100).compute();
    reduce(&mut live, Action::TogglePin);
    reduce(&mut live, Action::TogglePin);

    assert!(
        !old.apply(&mut live),
        "even reverted operator edits fence stale work"
    );
    assert!(live.work_cards.is_empty());
}

#[test]
fn large_backend_authority_change_rejects_stale_projection() {
    let mut live = fixture(12_000);
    let old = Input::capture(&live, 100).compute();
    let mut status = live.backend_status.clone();
    status.connected = false;
    status.error = Some("quarantined".into());
    reduce(&mut live, Action::BackendStatus(status));

    assert!(!old.apply(&mut live));
    assert!(live.work_cards.is_empty());
}
