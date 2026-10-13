//! Expensive Git detail is immutable and shared through reducer fanout and cloning.
use codex_tui::{app::{Action, AppState, reduce}, backend::{CodexBackend, FakeBackend}, git::{GitContext, GitFileChange, GitReview}};
use std::sync::Arc;

#[test]
fn shared_cwd_fanout_retains_one_detail_allocation_and_replaces_it_atomically() {
    let mut threads = FakeBackend::scaled(32).snapshot().threads;
    for thread in &mut threads { thread.metadata.cwd = "/shared/worktree".into(); }
    let leader = threads[0].id.clone();
    let mut state = AppState::new(threads);
    let mut context = GitContext::pending(leader.clone(), "/shared/worktree");
    context.is_repository = true;
    context.dirty = true;
    context.observed_at_unix_ms = 1;
    context.changes = (0..2000).map(|index| GitFileChange {
        path: format!("{index}-{}", "x".repeat(200)), original_path: None,
        index_status: None, worktree_status: None, untracked: true, conflict: false,
    }).collect();
    let first = context.changes.clone();
    reduce(&mut state, Action::GitContextLoaded(context.clone()));
    assert_eq!(state.git_contexts.len(), 32);
    for item in state.git_contexts.values() {
        assert!(Arc::ptr_eq(&item.changes, &first));
        assert!(Arc::ptr_eq(&item.clone().changes, &first));
    }
    let mut review = GitReview::pending(leader, "/shared/worktree");
    review.changes = first.clone();
    assert!(Arc::ptr_eq(&review.clone().changes, &first));
    context.observed_at_unix_ms = 2;
    context.changes = vec![first[0].clone()].into();
    let newer = context.changes.clone();
    reduce(&mut state, Action::GitContextLoaded(context));
    for item in state.git_contexts.values() { assert!(Arc::ptr_eq(&item.changes, &newer)); }
    assert_eq!(first.len(), 2000);
    assert_eq!(review.changes.len(), 2000);
}
