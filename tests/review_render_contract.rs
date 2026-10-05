//! Review rendering must consume prepared presentation, not run syntax/word diff work.
use codex_tui::{
    app::{Action, AppState, View, reduce},
    backend::{CodexBackend, FakeBackend},
    git::GitReview,
};
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn uncached_word_diff_falls_back_explicitly_without_computing_word_pairs() {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let id = app.threads[0].id.clone();
    let mut review = GitReview::pending(id.clone(), "/synthetic-review");
    review.observed_at_unix_ms = 42;
    review.unstaged_diff = "-old alpha\n+new beta\n".into();
    reduce(&mut app, Action::GitReviewLoaded(review));
    app.view = View::Review(id);
    app.review_word_diff = true;
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|frame| codex_tui::ui::render(frame, &app))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("showing plain diff"));
    assert!(text.contains("-old alpha"));
    assert!(!text.contains("[-old"));
    assert!(!text.contains("{+new"));
}

#[test]
fn input_thread_never_prewarms_review_syntax() {
    let main = include_str!("../src/main.rs");
    let drain = main
        .split("fn drain_git(")
        .nth(1)
        .unwrap()
        .split("#[cfg(test)]")
        .next()
        .unwrap();
    assert!(!drain.contains("prewarm_review_diff"));
    let git = include_str!("../src/git.rs");
    assert!(git.contains("tasks.len() < GIT_MAX_CONCURRENCY"));
    assert!(git.contains("spawn_blocking(move ||"));
}
