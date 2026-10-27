//! Pure reducer navigation only: never execute a selected command or backend effect.
use codex_tui::{
    app::{Action, AppState, reduce},
    backend::{CodexBackend, FakeBackend},
    planning::WorkflowStage,
};

fn fixture() -> AppState {
    AppState::new(FakeBackend::seeded().snapshot().threads)
}

fn navigate(app: &mut AppState, action: Action) {
    assert!(
        reduce(app, action).is_empty(),
        "navigation must not start a backend or mutation effect"
    );
}

fn expected(selected: usize, delta: i32, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let count = count as i128;
    ((selected as i128 + i128::from(delta)) % count + count) as usize % count as usize
}

const EXTREMES: [(usize, i32); 5] = [
    (i32::MAX as usize, 1),
    (usize::MAX, 1),
    (0, i32::MIN),
    (2, i32::MAX),
    (usize::MAX, -1),
];

#[test]
fn command_palette_never_truncates_a_stale_index_to_i32() {
    let mut app = fixture();
    navigate(&mut app, Action::OpenCommandPalette);
    let count = app.command_palette_choices().len();
    assert!(count > 1);
    for (selected, delta) in EXTREMES {
        app.command_palette_selected = selected;
        navigate(&mut app, Action::MoveCommandPalette(delta));
        assert_eq!(
            app.command_palette_selected,
            expected(selected, delta, count)
        );
        assert!(app.command_palette_choice().is_some());
    }
}

#[test]
fn board_columns_saved_views_and_card_cursor_wrap_at_extreme_indices() {
    let mut app = fixture();
    navigate(&mut app, Action::ReconcilePlanning { now_unix_ms: 10 });
    navigate(&mut app, Action::OpenBoard);
    let count = WorkflowStage::ALL.len();
    for (selected, delta) in EXTREMES {
        app.board_stage_index = selected;
        navigate(&mut app, Action::MoveBoardColumn(delta));
        assert_eq!(app.board_stage_index, expected(selected, delta, count));
        assert_eq!(app.board_selected, 0);
    }

    let count = app.planning_views().len();
    assert!(count > 0);
    for (selected, delta) in EXTREMES {
        app.planning_view_index = selected;
        navigate(&mut app, Action::CycleSavedView(delta));
        assert_eq!(app.planning_view_index, expected(selected, delta, count));
    }

    app.planning_view_index = 0;
    for stage in 0..WorkflowStage::ALL.len() {
        app.board_stage_index = stage;
        if !app.visible_planning_cards().is_empty() {
            break;
        }
    }
    let count = app.visible_planning_cards().len();
    assert!(count > 0, "fixture must have at least one Board card");
    for (selected, delta) in EXTREMES {
        app.board_selected = selected;
        navigate(&mut app, Action::MovePlanningSelection(delta));
        assert_eq!(app.board_selected, expected(selected, delta, count));
    }
}

#[test]
fn frozen_context_actions_wrap_without_executing_any_choice() {
    let mut app = fixture();
    navigate(&mut app, Action::ReconcilePlanning { now_unix_ms: 10 });
    navigate(&mut app, Action::OpenBoard);
    for stage in 0..WorkflowStage::ALL.len() {
        app.board_stage_index = stage;
        if !app.visible_planning_cards().is_empty() {
            break;
        }
    }
    navigate(&mut app, Action::OpenContext);
    assert!(app.context_open);
    let choices = app.context_choices();
    assert!(choices.len() > 10);
    for (selected, delta) in EXTREMES {
        app.context_selected = selected;
        navigate(&mut app, Action::MoveContext(delta));
        assert_eq!(
            app.context_selected,
            expected(selected, delta, choices.len())
        );
        assert_eq!(app.context_choices(), choices);
        assert!(app.context_choice().is_some());
    }
}

#[test]
fn empty_optional_lists_reset_invalid_cursors_without_effects() {
    let mut app = fixture();
    app.thread_queue_selected = usize::MAX;
    app.managed_selected = usize::MAX;
    app.launch_selected = usize::MAX;
    app.transcript_search_selected = usize::MAX;

    navigate(&mut app, Action::MoveThreadQueue(i32::MIN));
    navigate(&mut app, Action::MoveManagedWorktree(i32::MAX));
    navigate(&mut app, Action::MoveLaunchPreset(-1));
    navigate(&mut app, Action::MoveTranscriptSearch(1));

    assert_eq!(app.thread_queue_selected, 0);
    assert_eq!(app.managed_selected, 0);
    assert_eq!(app.launch_selected, 0);
    assert_eq!(app.transcript_search_selected, 0);
}
