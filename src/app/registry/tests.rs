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
    assert!(!app.thread_needs_attention(2));
    assert!(app.threads[2].pinned);
    assert_eq!(app.visible_indices()[..3], [3, 4, 2]);
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
