//! Same-cwd fanout must not copy the detailed status payload per thread.
use codex_tui::{
    app::{Action, AppState, reduce},
    backend::{CodexBackend, FakeBackend},
    git::{GitContext, GitFileChange},
};
use std::sync::Arc;

#[test]
fn fanout_and_snapshot_clones_share_one_immutable_change_list() {
    let mut threads = FakeBackend::scaled(32).snapshot().threads;
    for thread in &mut threads { thread.metadata.cwd = "/shared/repo".into(); }
    let mut app = AppState::new(threads);
    let mut context = GitContext::pending(app.threads[0].id.clone(), "/shared/repo");
    context.is_repository = true;
    context.dirty = true;
    context.changes = (0..2000).map(|i| GitFileChange {
        path: format!("{i:04}-{}", "x".repeat(200)), original_path: None,
        index_status: None, worktree_status: None, untracked: true, conflict: false,
    }).collect();
    let payload = context.changes.clone();
    reduce(&mut app, Action::GitContextLoaded(context));
    assert_eq!(app.git_contexts.len(), 32);
    let snapshot = app.clone();
    for view in [&app, &snapshot] {
        for thread in &view.threads {
            let context = view.git_contexts.get(&thread.id.0).unwrap();
            assert_eq!(context.thread_id, thread.id);
            assert!(context.dirty);
            assert_eq!(context.changes.len(), 2000);
            assert!(Arc::ptr_eq(&payload, &context.changes));
        }
    }
}

#[test]
fn nested_cwd_payloads_share_only_equal_observations_without_rewriting_authority() {
    use codex_tui::domain::{LocalRepoIdentity, WorktreeIdentity};
    let mut threads = FakeBackend::scaled(2).snapshot().threads;
    threads[0].metadata.cwd = "/repo".into();
    threads[1].metadata.cwd = "/repo/nested".into();
    let mut app = AppState::new(threads);
    let mut contexts = Vec::new();
    for (index, thread) in app.threads.iter().enumerate() {
        let mut context = GitContext::pending(thread.id.clone(), thread.metadata.cwd.clone());
        context.is_repository = true;
        context.dirty = true;
        context.observed_at_unix_ms = index as u64 + 1;
        context.worktree = Some(WorktreeIdentity {
            repo: LocalRepoIdentity { git_common_dir: "/repo/.git".into(), primary_root: "/repo".into() },
            canonical_path: "/repo".into(), branch: None, managed_by_codex_tui: false,
        });
        context.changes = vec![GitFileChange {
            path: "changed.rs".into(), original_path: None, index_status: None,
            worktree_status: Some('M'), untracked: false, conflict: false,
        }].into();
        contexts.push(context);
    }
    let payload = contexts[0].changes.clone();
    assert!(!Arc::ptr_eq(&payload, &contexts[1].changes));
    for context in &contexts { reduce(&mut app, Action::GitContextLoaded(context.clone())); }
    for original in &contexts {
        let installed = &app.git_contexts[&original.thread_id.0];
        assert_eq!(installed.observed_at_unix_ms, original.observed_at_unix_ms);
        assert_eq!(installed.cwd, original.cwd);
        assert!(Arc::ptr_eq(&payload, &installed.changes));
    }
    let mut changed = contexts[1].clone();
    let mut fresh = changed.changes.to_vec();
    fresh[0].path = "different.rs".into();
    changed.changes = fresh.into();
    reduce(&mut app, Action::GitContextLoaded(changed.clone()));
    assert!(!Arc::ptr_eq(&payload, &app.git_contexts[&changed.thread_id.0].changes));
    assert_eq!(app.git_contexts[&contexts[0].thread_id.0].changes[0].path, "changed.rs");
}
