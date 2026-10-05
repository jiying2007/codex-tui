use codex_tui::{
    app::{Action, AppState, View, git_review_cache_limit, reduce},
    backend::{CodexBackend, FakeBackend},
    domain::ThreadId,
    git::{GitFileChange, GitReview},
};

fn fixture() -> AppState {
    let mut state = AppState::new(FakeBackend::scaled(8).snapshot().threads);
    state.view = View::Review(state.threads[0].id.clone());
    state
}

fn review(state: &AppState, index: usize, paths: &[&str]) -> GitReview {
    let thread = &state.threads[index];
    let mut review = GitReview::pending(thread.id.clone(), thread.metadata.cwd.clone());
    review.observed_at_unix_ms = 10;
    review.changes = paths
        .iter()
        .map(|path| GitFileChange {
            path: (*path).into(),
            original_path: None,
            index_status: None,
            worktree_status: Some('M'),
            untracked: false,
            conflict: false,
        })
        .collect();
    review
}

fn install(state: &mut AppState, review: GitReview) {
    assert!(reduce(state, Action::GitReviewLoaded(review)).is_empty());
}

#[test]
fn background_review_cannot_retarget_active_file() {
    let mut state = fixture();
    let current = review(&state, 0, &["a.rs", "b.rs", "c.rs"]);
    install(&mut state, current);
    state.review_selected = 2;
    state.review_scroll = 19;
    let other = review(&state, 1, &["other.rs"]);
    install(&mut state, other.clone());
    assert_eq!(state.review_selected, 2);
    assert_eq!(state.review_scroll, 19);
    assert_eq!(state.git_reviews.get(&other.thread_id.0), Some(&other));
}

#[test]
fn reordered_active_review_preserves_file_identity() {
    let mut state = fixture();
    let initial = review(&state, 0, &["a.rs", "b.rs", "c.rs"]);
    install(&mut state, initial);
    state.review_selected = 1;
    state.review_scroll = 19;
    let fresh = review(&state, 0, &["c.rs", "a.rs", "b.rs"]);
    install(&mut state, fresh);
    assert_eq!(state.review_selected, 2);
    assert_eq!(state.review_scroll, 19);
}

#[test]
fn old_cwd_review_cannot_replace_current_snapshot() {
    let mut state = fixture();
    let old = review(&state, 0, &["old.rs"]);
    let mut threads = state.threads.clone();
    threads[0].metadata.cwd.push_str("/new-worktree");
    reduce(&mut state, Action::ReplaceThreads(threads));
    let current = review(&state, 0, &["new.rs", "second.rs"]);
    install(&mut state, current.clone());
    state.review_selected = 1;
    state.review_scroll = 19;
    install(&mut state, old);
    assert_eq!(state.git_reviews.get(&current.thread_id.0), Some(&current));
    assert_eq!((state.review_selected, state.review_scroll), (1, 19));
}

#[test]
fn removed_thread_review_is_not_reintroduced() {
    let mut state = fixture();
    let mut orphan = review(&state, 0, &["orphan.rs"]);
    orphan.thread_id = ThreadId::new("removed-thread");
    install(&mut state, orphan);
    assert!(!state.git_reviews.contains_key("removed-thread"));
}

#[test]
fn removed_file_resets_scroll_and_clamps_selection() {
    let mut state = fixture();
    let initial = review(&state, 0, &["a.rs", "removed.rs", "c.rs"]);
    install(&mut state, initial);
    state.review_selected = 1;
    state.review_scroll = 19;
    let fresh = review(&state, 0, &["a.rs", "c.rs"]);
    install(&mut state, fresh);
    assert_eq!((state.review_selected, state.review_scroll), (1, 0));
}

#[test]
fn empty_review_resets_scroll() {
    let mut state = fixture();
    let initial = review(&state, 0, &["a.rs"]);
    install(&mut state, initial);
    state.review_scroll = 19;
    let empty = review(&state, 0, &[]);
    install(&mut state, empty);
    assert_eq!((state.review_selected, state.review_scroll), (0, 0));
}

#[test]
fn same_file_refresh_keeps_selection_and_scroll() {
    let mut state = fixture();
    let initial = review(&state, 0, &["a.rs", "b.rs"]);
    install(&mut state, initial.clone());
    state.review_selected = 1;
    state.review_scroll = 19;
    install(&mut state, initial);
    assert_eq!((state.review_selected, state.review_scroll), (1, 19));
}

#[test]
fn background_arrival_keeps_active_review_cached() {
    let mut state = fixture();
    let active = review(&state, 0, &["active.rs"]);
    install(&mut state, active.clone());
    for index in 1..state.threads.len() {
        let other = review(&state, index, &["other.rs"]);
        install(&mut state, other);
        assert!(state.git_reviews.len() <= git_review_cache_limit());
        assert_eq!(state.git_reviews.get(&active.thread_id.0), Some(&active));
    }
}
