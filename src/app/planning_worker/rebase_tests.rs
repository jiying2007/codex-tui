//! Deterministic source changes between capture and delivery: no scheduler timing.
use super::*;
use crate::backend::{CodexBackend, FakeBackend};

fn app(count: usize) -> AppState {
    AppState::new(FakeBackend::scaled(count).snapshot().threads)
}
fn assert_current(mut live: AppState, output: Output) {
    let mut expected = live.clone();
    rebuild_planning(&mut expected, 100);
    assert!(
        output.apply(&mut live),
        "small source delta must not starve"
    );
    assert_eq!(live.work_cards, expected.work_cards);
    assert_eq!(live.work_card_by_thread, expected.work_card_by_thread);
    assert_eq!(
        live.worktree_collision_counts,
        expected.worktree_collision_counts
    );
}
#[test]
fn unrelated_continuous_churn_cannot_discard_a_current_target() {
    let mut live = app(5000);
    for round in 0..8 {
        let mut threads = live.threads.clone();
        threads[0].title = format!("target-{round}");
        reduce(&mut live, Action::ReplaceThreads(threads));
        let output = Input::capture(&live, 100).compute();
        let mut noise = live.threads.clone();
        noise[1].title = format!("noise-{round}");
        reduce(&mut live, Action::ReplaceThreads(noise));
        assert_current(live.clone(), output);
    }
}
#[test]
fn same_target_newer_title_and_attention_are_recomputed_not_overwritten() {
    let mut live = app(20);
    let output = Input::capture(&live, 100).compute();
    let mut threads = live.threads.clone();
    threads[0].title = "latest title".into();
    threads[0].runtime = RuntimeStatus::WaitingHuman;
    threads[0].attention.push(AttentionReason::ApprovalRequired);
    reduce(&mut live, Action::ReplaceThreads(threads));
    assert_current(live, output);
}
#[test]
fn small_membership_change_does_not_resurrect_a_removed_thread() {
    let mut live = app(20);
    let output = Input::capture(&live, 100).compute();
    let mut threads = live.threads.clone();
    threads.remove(2);
    let mut added = threads[0].clone();
    added.id = ThreadId::new("newly-added");
    added.title = "new source".into();
    threads.push(added);
    reduce(&mut live, Action::ReplaceThreads(threads));
    assert_current(live, output);
}
#[test]
fn operator_aba_still_fences_an_old_result() {
    let mut live = app(20);
    let output = Input::capture(&live, 100).compute();
    reduce(&mut live, Action::TogglePin);
    reduce(&mut live, Action::TogglePin);
    assert!(!output.apply(&mut live));
    assert!(live.work_cards.is_empty());
}
#[test]
fn backend_authority_change_still_requires_a_fresh_snapshot() {
    let mut live = app(20);
    let output = Input::capture(&live, 100).compute();
    let mut backend = live.backend_status.clone();
    backend.connected = false;
    backend.error = Some("transport quarantined".into());
    reduce(&mut live, Action::BackendStatus(backend));
    assert!(!output.apply(&mut live));
    assert!(live.work_cards.is_empty());
}
#[test]
fn broad_churn_does_not_expand_the_bounded_ui_reconcile_budget() {
    let mut live = app(100);
    let output = Input::capture(&live, 100).compute();
    let mut threads = live.threads.clone();
    for thread in threads.iter_mut().take(65) {
        thread.title.push_str(" changed");
    }
    reduce(&mut live, Action::ReplaceThreads(threads));
    assert!(!output.apply(&mut live));
    assert!(live.work_cards.is_empty());
    assert!(Input::capture(&live, 100).compute().apply(&mut live));
}
#[test]
fn local_snapshot_change_still_fences_an_old_result() {
    let mut live = app(20);
    let output = Input::capture(&live, 100).compute();
    let snapshot = live.planning_snapshot.clone();
    reduce(&mut live, Action::PlanningSnapshotLoaded(snapshot));
    assert!(!output.apply(&mut live));
}
#[test]
fn shared_worktree_peer_is_reprojected_when_only_one_runtime_changes() {
    let mut live = app(20);
    let repo = LocalRepoIdentity {
        git_common_dir: "/repo/.git".into(),
        primary_root: "/repo".into(),
    };
    for thread in live.threads.iter_mut().take(2) {
        thread.runtime = RuntimeStatus::Working;
        thread.metadata.cwd = "/repo".into();
        let mut context = GitContext::pending(thread.id.clone(), "/repo");
        context.is_repository = true;
        context.repo = Some(repo.clone());
        context.worktree = Some(crate::domain::WorktreeIdentity {
            repo: repo.clone(),
            canonical_path: "/repo".into(),
            branch: Some("main".into()),
            managed_by_codex_tui: false,
        });
        live.git_contexts.insert(thread.id.0.clone(), context);
    }
    let output = Input::capture(&live, 100).compute();
    assert_eq!(output.collisions[&live.threads[1].id.0], 1);
    let mut threads = live.threads.clone();
    threads[0].runtime = RuntimeStatus::Inactive;
    reduce(&mut live, Action::ReplaceThreads(threads));
    assert_current(live, output);
}
include!("stale_retry_tests.rs");
