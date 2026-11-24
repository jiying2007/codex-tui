use super::*;
use crate::backend::{CodexBackend, FakeBackend};
use crate::planning::WorkCardRecord;

fn threads(count: usize) -> Vec<ThreadSummary> {
    let mut threads = FakeBackend::scaled(count).snapshot().threads;
    for thread in &mut threads {
        thread.attention.clear();
        thread.pinned = false;
        thread.runtime = RuntimeStatus::Working;
        thread.metadata.updated_at = 0;
    }
    threads
}

#[test]
fn unlocated_request_count_tracks_actual_requests_without_creating_threads() {
    let mut app = AppState::new(threads(1));
    let known = app.threads[0].id.clone();
    let unknown = ThreadId::new("not-in-current-target-registry");
    for id in [41, 42] {
        reduce(
            &mut app,
            Action::InteractiveRequested(InteractiveRequest {
                request_id: RpcRequestId::Integer(id),
                thread_id: unknown.clone(),
                turn_id: "turn".into(),
                item_id: format!("input-{id}"),
                kind: InteractiveRequestKind::UserInput { questions: vec![] },
            }),
        );
    }
    assert_eq!(app.unlocated_pending_request_count(), 2);
    assert_eq!(app.threads.len(), 1);
    reduce(
        &mut app,
        Action::InteractiveRequested(InteractiveRequest {
            request_id: RpcRequestId::Integer(43),
            thread_id: known,
            turn_id: "turn".into(),
            item_id: "known-input".into(),
            kind: InteractiveRequestKind::UserInput { questions: vec![] },
        }),
    );
    assert_eq!(app.unlocated_pending_request_count(), 2);
    reduce(
        &mut app,
        Action::InteractiveResolved {
            request_id: RpcRequestId::Integer(41),
        },
    );
    assert_eq!(app.unlocated_pending_request_count(), 1);
    let mut discovered = app.threads.clone();
    let mut actual = discovered[0].clone();
    actual.id = unknown;
    discovered.push(actual);
    reduce(&mut app, Action::ReplaceThreads(discovered));
    assert_eq!(app.unlocated_pending_request_count(), 0);
    assert_eq!(app.threads.len(), 2);
    assert!(app.thread_needs_attention(1));
}

#[test]
fn attention_then_pins_then_recency_only_reorders_the_display() {
    let mut source = threads(5);
    source[0].metadata.updated_at = 100;
    source[1].pinned = true;
    source[2].attention.push(AttentionReason::ApprovalRequired);
    source[3].id = ThreadId::new("z");
    source[4].id = ThreadId::new("a");
    let app = AppState::new(source.clone());
    assert_eq!(app.visible_indices(), [2, 1, 0, 4, 3]);
    assert_eq!(app.threads, source);
}

#[test]
fn first_snapshot_selects_priority_then_refresh_preserves_identity_or_first_row() {
    let mut source = threads(3);
    source[2].attention.push(AttentionReason::UserInputRequired);
    let mut app = AppState::new(vec![]);
    reduce(&mut app, Action::ReplaceThreads(source));
    assert_eq!(app.selected, 2);
    assert_eq!(app.selected, app.visible_indices()[0]);
    app.selected = 1;
    let selected_id = app.selected_thread_id();
    let mut next = app.threads.clone();
    next.swap(0, 1);
    next[2].attention.clear();
    next[1].attention.push(AttentionReason::SystemError);
    reduce(&mut app, Action::ReplaceThreads(next));
    assert_eq!(app.selected_thread_id(), selected_id);
    assert_eq!(app.selected, 0);
    let next = app.threads.iter().skip(1).cloned().collect();
    reduce(&mut app, Action::ReplaceThreads(next));
    assert_eq!(app.selected, app.visible_indices()[0]);
    assert_ne!(app.selected_thread_id(), selected_id);
}

#[test]
fn acknowledgment_preserves_actionable_and_pending_requests_while_snooze_keeps_pins() {
    let mut source = threads(5);
    source[0].metadata.updated_at = 100;
    source[1].attention.push(AttentionReason::ReadyForReview);
    source[2].attention.push(AttentionReason::ApprovalRequired);
    source[2].pinned = true;
    source[3].attention.push(AttentionReason::SystemError);
    let mut app = AppState::new(source);
    for index in [1, 2, 3] {
        app.selected = index;
        reduce(&mut app, Action::AcknowledgeAttention);
    }
    assert!(!app.thread_needs_attention(1));
    assert!(app.thread_needs_attention(2));
    assert!(app.thread_needs_attention(3));
    let pending_id = app.threads[4].id.clone();
    reduce(
        &mut app,
        Action::InteractiveRequested(InteractiveRequest {
            request_id: RpcRequestId::Integer(42),
            thread_id: pending_id,
            turn_id: "turn".into(),
            item_id: "input".into(),
            kind: InteractiveRequestKind::UserInput { questions: vec![] },
        }),
    );
    app.selected = 4;
    reduce(&mut app, Action::AcknowledgeAttention);
    assert!(app.thread_needs_attention(4));
    assert_eq!(app.visible_indices()[..3], [2, 3, 4]);

    let mut local = WorkCardRecord::implicit_thread(&app.threads[2].id);
    local.overlay.snooze_until_unix_ms = Some(10_000);
    app.planning_snapshot.cards.push(local);
    reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1 });
    assert!(app.thread_needs_attention(2));
    assert!(app.threads[2].pinned);
    assert_eq!(app.visible_indices()[..3], [2, 3, 4]);
}

#[test]
fn large_registry_retains_source_window_and_out_of_window_exceptions() {
    let mut source = threads(5_000);
    source[3_000].metadata.updated_at = 999_999;
    source[4_000].pinned = true;
    source[4_999]
        .attention
        .push(AttentionReason::ApprovalRequired);
    let mut app = AppState::new(source.clone());
    let (visible, matched) = app.visible_indices_with_match_count();
    assert_eq!(matched, 5_000);
    assert_eq!(visible.len(), REGISTRY_RECENT_LIMIT + 2);
    assert_eq!(visible[..2], [4_999, 4_000]);
    assert!(!visible.contains(&3_000));
    assert!((0..REGISTRY_RECENT_LIMIT).all(|index| visible.contains(&index)));
    assert_eq!(app.threads, source);
    app.filter = source[3_000].title.clone();
    assert_eq!(app.visible_indices(), [3_000]);
    app.filter.clear();
    app.show_all_history = true;
    assert_eq!(app.visible_indices().len(), 5_000);
}

fn observe_goal(app: &mut AppState, index: usize, status: GoalStatus) {
    reduce(
        app,
        Action::GoalObserved(GoalObservation {
            thread_id: app.threads[index].id.clone(),
            objective: "fixture goal".into(),
            status,
            token_budget: None,
            tokens_used: 0,
            time_used_seconds: 0,
            created_at: 1,
            updated_at: 1,
            observed_at_unix_ms: 1,
        }),
    );
}

#[test]
fn observed_goal_attention_survives_recent_window_and_acknowledgment() {
    use ratatui::{Terminal, backend::TestBackend};
    let mut app = AppState::new(threads(150));
    for status in [
        GoalStatus::Blocked,
        GoalStatus::UsageLimited,
        GoalStatus::BudgetLimited,
    ] {
        observe_goal(&mut app, 149, status);
        assert!(app.thread_needs_attention(149), "live goal before Board projection");
        reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1 });
        assert!(app.thread_needs_attention(149), "{status:?}");
        assert_eq!(app.visible_indices()[0], 149);
        assert_eq!(app.visible_indices().len(), REGISTRY_RECENT_LIMIT + 1);
        app.selected = 0;
        reduce(&mut app, Action::NextAttention);
        assert_eq!(app.selected, 149);
        reduce(&mut app, Action::AcknowledgeAttention);
        assert!(app.thread_needs_attention(149));
    }
    let mut terminal = Terminal::new(TestBackend::new(180, 40)).unwrap();
    terminal
        .draw(|frame| crate::ui::render(frame, &app))
        .unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(screen.contains("1 need attention"), "{screen}");
    assert!(
        screen.contains("budget-"),
        "derived reason is missing: {screen}"
    );

    let mut local = WorkCardRecord::implicit_thread(&app.threads[149].id);
    local.overlay.snooze_until_unix_ms = Some(100);
    app.planning_snapshot.cards.push(local);
    reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1 });
    assert!(!app.thread_needs_attention(149));
    assert!(app.visible_indices().contains(&149)); // current selection is retained
    reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 100 });
    assert!(app.thread_needs_attention(149));
}

#[test]
fn informational_goal_completion_respects_acknowledgment() {
    let mut app = AppState::new(threads(1));
    observe_goal(&mut app, 0, GoalStatus::Complete);
    reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1 });
    assert!(app.thread_needs_attention(0));
    reduce(&mut app, Action::AcknowledgeAttention);
    assert!(!app.thread_needs_attention(0));
    // A new live request is actionable before another Planning reconciliation.
    app.threads[0]
        .attention
        .push(AttentionReason::UserInputRequired);
    assert!(app.thread_needs_attention(0));
}

#[test]
fn observed_failures_do_not_depend_on_work_cards_or_stale_projections() {
    use crate::forge::{ForgeObservation, PipelineSummary};
    use crate::planning::PlanningAttention;
    let mut app = AppState::new(threads(1));
    app.threads[0].metadata.cwd = "/fixture/repo".into();
    let id = app.threads[0].id.clone();
    let mut git = GitContext::pending(id.clone(), "/fixture/repo");
    git.is_repository = true;
    git.branch = Some("main".into());
    app.git_contexts.insert(id.0.clone(), git);
    let mut forge = ForgeObservation::pending(id.clone(), "/fixture/repo".into());
    forge.pipelines.push(PipelineSummary {
        id: 1,
        status: "failed".into(),
        reference: "main".into(),
        web_url: "https://example.invalid/pipeline/1".into(),
        updated_at: None,
    });
    app.forge_observations.insert(id.0.clone(), forge);
    reduce(&mut app, Action::AcknowledgeAttention);
    assert!(app.work_cards.is_empty());
    assert!(app.thread_needs_attention(0));
    assert!(
        app.session_attention_reasons(&app.threads[0])
            .contains(&PlanningAttention::PipelineFailed)
    );
    reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1 });
    app.forge_observations.clear();
    assert!(!app.thread_needs_attention(0), "stale Board failure is not authority");

    observe_goal(&mut app, 0, GoalStatus::BudgetLimited);
    assert!(app.thread_needs_attention(0));
    reduce(&mut app, Action::GoalCleared(id));
    assert!(!app.thread_needs_attention(0));
}
