//! Review rendering must consume prepared presentation, not run syntax/word diff work.
use codex_tui::{
    app::{Action, AppState, Effect, View, reduce},
    backend::{CodexBackend, FakeBackend},
    domain::{LocalRepoIdentity, WorktreeIdentity},
    git::{GitContext, GitFileChange, GitReview},
};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, style::Modifier};

fn review_app(count: usize) -> AppState {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.threads[0].metadata.cwd = "/synthetic-review".into();
    let id = app.threads[0].id.clone();
    let mut review = GitReview::pending(id.clone(), "/synthetic-review");
    review.observed_at_unix_ms = 43;
    review.changes = (0..count)
        .map(|index| GitFileChange {
            path: format!("file-{index:02}.rs"),
            original_path: None,
            index_status: Some('M'),
            worktree_status: None,
            untracked: false,
            conflict: false,
        })
        .collect();
    reduce(&mut app, Action::GitReviewLoaded(review));
    let mut git = GitContext::pending(id.clone(), "/synthetic-review");
    git.is_repository = true;
    git.worktree = Some(WorktreeIdentity {
        repo: LocalRepoIdentity {
            git_common_dir: "/synthetic-review/.git".into(),
            primary_root: "/synthetic-review".into(),
        },
        canonical_path: "/synthetic-review".into(),
        branch: None,
        managed_by_codex_tui: false,
    });
    app.git_contexts.insert(id.0.clone(), git);
    app.view = View::Review(id);
    app
}

fn render_review(app: &AppState, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| codex_tui::ui::render(frame, app))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn selected_row(buffer: &Buffer, label: &str) -> u16 {
    let marker = format!("> M. {label}");
    for y in 0..buffer.area.height {
        let row: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        if row.contains(&marker) {
            assert!(buffer[(1, y)].modifier.contains(Modifier::REVERSED));
            return y;
        }
    }
    panic!(
        "selected file {label:?} missing from {}x{} buffer",
        buffer.area.width, buffer.area.height
    );
}

#[test]
fn changed_file_selection_stays_visible_when_navigating_and_resizing() {
    let mut app = review_app(40);
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
    for delta in [0, 4, 35, 1, -1, -1] {
        assert!(reduce(&mut app, Action::MoveReview(delta)).is_empty());
        let label = format!("file-{:02}.rs", app.review_selected);
        for (width, height) in [(80, 30), (160, 30), (80, 20), (160, 20)] {
            terminal.backend_mut().resize(width, height);
            terminal.resize(Rect::new(0, 0, width, height)).unwrap();
            terminal
                .draw(|frame| codex_tui::ui::render(frame, &app))
                .unwrap();
            selected_row(terminal.backend().buffer(), &label);
        }
        assert!(matches!(
            reduce(&mut app, Action::OpenReviewExternalEditor).as_slice(),
            [Effect::OpenExternalEditor { path, .. }] if path == &label
        )); // Inspect only; selection must name the same visible editor target.
    }
}

#[test]
fn changed_file_list_handles_empty_and_stale_selection_without_mutating_state() {
    for count in [0, 40] {
        let mut app = review_app(count);
        for stale in [count, usize::MAX] {
            app.review_selected = stale;
            for width in [80, 160] {
                let buffer = render_review(&app, width, 30);
                assert!(
                    buffer
                        .content
                        .iter()
                        .all(|cell| !cell.modifier.contains(Modifier::REVERSED))
                );
                assert_eq!(app.review_selected, stale);
                assert!(reduce(&mut app, Action::OpenReviewExternalEditor).is_empty());
            }
        }
    }
}

#[test]
fn long_untrusted_paths_use_one_safe_row_and_keep_the_editor_target() {
    let mut app = review_app(6);
    let id = app.threads[0].id.clone();
    let path = format!(
        "target-中文\n\r\t\u{001b}\u{202e}-{}",
        "很长路径".repeat(60)
    );
    app.git_reviews.get_mut(&id.0).unwrap().changes[0].path = path.clone();
    for width in [80, 160] {
        let buffer = render_review(&app, width, 30);
        let y = selected_row(&buffer, "target-");
        let selected: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
        assert!(selected.contains('中') && selected.contains('文') && selected.contains("��"));
        let next: String = (0..width).map(|x| buffer[(x, y + 1)].symbol()).collect();
        assert!(
            next.contains("file-01.rs"),
            "long path wrapped into the next file row"
        );
        assert!(buffer.content.iter().all(|cell| {
            !cell
                .symbol()
                .contains(['\u{001b}', '\u{202e}', '\n', '\r', '\t'])
        }));
    }
    assert_eq!(app.git_reviews[&id.0].changes[0].path, path);
    assert_eq!(
        reduce(&mut app, Action::OpenReviewExternalEditor),
        vec![Effect::OpenExternalEditor {
            thread_id: id,
            cwd: "/synthetic-review".into(),
            worktree_root: "/synthetic-review".into(),
            path,
        }]
    ); // Inspect only: never execute an external-editor effect.
}

#[test]
fn diff_scroll_does_not_move_the_visible_file_selection() {
    let mut app = review_app(40);
    reduce(&mut app, Action::MoveReview(39));
    let id = app.threads[0].id.clone();
    app.git_reviews.get_mut(&id.0).unwrap().unstaged_diff =
        (0..50).map(|i| format!("+diff-line-{i:02}\n")).collect();
    for width in [80, 160] {
        let before = render_review(&app, width, 30);
        let text: String = before.content.iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("diff-line-00"));
        reduce(&mut app, Action::ScrollReviewBy(10));
        let after = render_review(&app, width, 30);
        selected_row(&after, "file-39.rs");
        let text: String = after.content.iter().map(|cell| cell.symbol()).collect();
        assert!(!text.contains("diff-line-00"));
        assert!(text.contains("diff-line-10"));
        assert_eq!(app.review_scroll, 10);
        reduce(&mut app, Action::ScrollReviewBy(-10));
    }
    assert_eq!(app.review_selected, 39);
    reduce(&mut app, Action::ScrollReviewBy(10));
    reduce(&mut app, Action::MoveReview(-1));
    assert_eq!(app.review_scroll, 0);
    assert_eq!(app.review_selected, 38);
}

#[test]
fn uncached_word_diff_falls_back_explicitly_without_computing_word_pairs() {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.threads[0].metadata.cwd = "/synthetic-review".into();
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
