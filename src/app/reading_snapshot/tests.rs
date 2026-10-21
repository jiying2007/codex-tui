use super::*;
use crate::backend::{CodexBackend, FakeBackend};
use crate::conversation::{ConversationItem, ConversationItemKind, ConversationTurn};

fn page(id: &ThreadId, item: &str, cursor: &str, active: &str) -> ConversationPage {
    ConversationPage {
        thread_id: id.clone(),
        title: None,
        turns: vec![ConversationTurn {
            id: active.into(),
            status: "inProgress".into(),
            started_at: None,
            completed_at: None,
            error: None,
        }],
        items: vec![ConversationItem {
            turn_id: "body-turn".into(),
            item_id: item.into(),
            kind: ConversationItemKind::Assistant,
            text: (0..30)
                .map(|n| format!("{item}-{n:02}"))
                .collect::<Vec<_>>()
                .join("\n"),
            status: None,
        }],
        next_turn_cursor: Some(format!("turn-{cursor}")),
        next_item_cursor: Some(format!("item-{cursor}")),
    }
}

fn visible(state: &AppState, id: &ThreadId) -> String {
    let (width, height) = state.thread_viewport_size.get();
    crate::conversation_viewport::viewport(
        state.displayed_conversation(&id.0).unwrap(),
        &state.thread_ui[&id.0],
        width,
        height,
    )
    .visible()
    .iter()
    .map(|(_, _, text)| *text)
    .collect::<Vec<_>>()
    .join("\n")
}

fn load_older(state: &mut AppState, page: ConversationPage, cursor: &str) {
    reduce(
        state,
        Action::OlderConversationLoaded {
            page,
            turn_cursor: Some(format!("turn-{cursor}")),
            item_cursor: Some(format!("item-{cursor}")),
        },
    );
}

fn older_request(state: &mut AppState, id: &ThreadId, cursor: &str) {
    reduce(state, Action::ConversationStart);
    assert_eq!(
        reduce(state, Action::ScrollBy(-5)),
        vec![Effect::LoadOlderConversation {
            thread_id: id.clone(),
            turn_cursor: Some(format!("turn-{cursor}")),
            item_cursor: Some(format!("item-{cursor}")),
        }]
    );
}

fn reading_old_page() -> (AppState, ThreadId) {
    let mut state = AppState::new(FakeBackend::seeded().snapshot().threads);
    state.threads[0].runtime = RuntimeStatus::Ready;
    let id = state.threads[0].id.clone();
    state.selected = 0;
    reduce(&mut state, Action::OpenSelected);
    state.thread_viewport_size.set((40, 6));
    reduce(
        &mut state,
        Action::ConversationLoaded(page(&id, "recent", "older", "active-before")),
    );
    older_request(&mut state, &id, "older");
    load_older(&mut state, page(&id, "old", "ancient", "old-turn"), "older");
    reduce(&mut state, Action::ConversationStart);
    reduce(&mut state, Action::ScrollBy(5));
    assert_eq!(state.thread_ui[&id.0].scroll_anchor.as_deref(), Some("old"));
    assert!(state.thread_ui[&id.0].scroll_offset > 0);
    (state, id)
}

fn paused() -> (AppState, ThreadId) {
    let (mut state, id) = reading_old_page();
    reduce(
        &mut state,
        Action::ConversationLoaded(page(&id, "newest", "gap", "active-now")),
    );
    assert!(state.is_reading_snapshot(&id.0));
    (state, id)
}

#[test]
fn recent_refresh_preserves_old_anchor_and_offset_but_updates_authoritative_turn() {
    let (mut state, id) = reading_old_page();
    let before = visible(&state, &id);
    let ui = state.thread_ui[&id.0].clone();
    let original_text = state.conversations[&id.0].items[0].text.as_ptr();
    reduce(
        &mut state,
        Action::ConversationLoaded(page(&id, "newest", "gap", "active-now")),
    );
    assert_eq!(visible(&state, &id), before);
    assert_eq!(state.thread_ui[&id.0], ui);
    assert!(state.is_reading_snapshot(&id.0));
    assert_eq!(
        state.displayed_conversation(&id.0).unwrap().items[0]
            .text
            .as_ptr(),
        original_text
    );
    assert_eq!(
        state.current_conversation().unwrap().active_turn_id(),
        Some("active-now")
    );
    assert!(state.active_history_watches.contains(&id.0));
    assert_eq!(
        reduce(&mut state, Action::InterruptCurrent),
        vec![Effect::InterruptTurn {
            thread_id: id.clone(),
            turn_id: "active-now".into(),
        }]
    );
    reduce(
        &mut state,
        Action::ConversationLoaded(page(&id, "latest", "new-gap", "active-latest")),
    );
    assert_eq!(visible(&state, &id), before);
    assert_eq!(
        state.displayed_conversation(&id.0).unwrap().items[0]
            .text
            .as_ptr(),
        original_text
    );
    assert_eq!(state.displayed_conversation(&id.0).unwrap().items.len(), 2);
    assert_eq!(state.conversations[&id.0].items.len(), 1);
    reduce(&mut state, Action::FollowNewest);
    assert!(state.reading_snapshot.is_none());
    assert!(visible(&state, &id).contains("latest-29"));
    assert_eq!(
        state.conversations[&id.0].next_item_cursor.as_deref(),
        Some("item-new-gap")
    );
}

#[test]
fn paused_pagination_keeps_its_contiguous_cursor_separate_from_recent_history() {
    let (mut state, id) = paused();
    older_request(&mut state, &id, "ancient");
    reduce(
        &mut state,
        Action::ConversationLoaded(page(&id, "latest", "new-gap", "active-latest")),
    );
    load_older(
        &mut state,
        page(&id, "ancient", "oldest", "ancient-turn"),
        "ancient",
    );
    let displayed = state.displayed_conversation(&id.0).unwrap();
    assert_eq!(
        displayed
            .items
            .iter()
            .map(|item| item.item_id.as_str())
            .collect::<Vec<_>>(),
        ["ancient", "old", "recent"]
    );
    assert_eq!(displayed.next_item_cursor.as_deref(), Some("item-oldest"));
    assert_eq!(
        state.conversations[&id.0].next_item_cursor.as_deref(),
        Some("item-new-gap")
    );
    assert_eq!(
        state.current_conversation().unwrap().active_turn_id(),
        Some("active-latest")
    );
    older_request(&mut state, &id, "oldest");
}

#[test]
fn end_rejects_late_old_page_and_failure_without_swallowing_the_recent_gap() {
    let (mut state, id) = paused();
    older_request(&mut state, &id, "ancient");
    reduce(&mut state, Action::FollowNewest);
    older_request(&mut state, &id, "gap");
    load_older(
        &mut state,
        page(&id, "ancient", "oldest", "ancient-turn"),
        "ancient",
    );
    reduce(
        &mut state,
        Action::OlderConversationFailed {
            thread_id: id.clone(),
            turn_cursor: Some("turn-ancient".into()),
            item_cursor: Some("item-ancient".into()),
            error: "late old failure".into(),
        },
    );
    let recent = &state.conversations[&id.0];
    assert_eq!(recent.items.len(), 1);
    assert_eq!(recent.next_item_cursor.as_deref(), Some("item-gap"));
    assert!(recent.loading_older);
    assert!(recent.error.is_none());
    assert!(state.mutation_notice.is_none());
    assert!(state.active_history_watches.contains(&id.0));
    load_older(
        &mut state,
        page(&id, "middle", "older", "middle-turn"),
        "gap",
    );
    assert_eq!(
        state.conversations[&id.0]
            .items
            .iter()
            .map(|item| item.item_id.as_str())
            .collect::<Vec<_>>(),
        ["middle", "newest"]
    );
    older_request(&mut state, &id, "older");
}

#[test]
fn retained_anchor_refreshes_normally_and_same_item_id_in_another_turn_is_distinct() {
    let (mut state, id) = reading_old_page();
    let ui = state.thread_ui[&id.0].clone();
    let mut updated = page(&id, "old", "ancient", "active-now");
    updated
        .items
        .push(page(&id, "tail", "unused", "unused").items.remove(0));
    reduce(&mut state, Action::ConversationLoaded(updated));
    assert!(state.reading_snapshot.is_none());
    assert_eq!(state.thread_ui[&id.0], ui);
    assert_eq!(state.conversations[&id.0].items.len(), 2);
    let mut reused = page(&id, "old", "gap", "active-next");
    reused.items[0].turn_id = "different-turn".into();
    reduce(&mut state, Action::ConversationLoaded(reused));
    assert!(state.is_reading_snapshot(&id.0));
    assert_eq!(state.thread_ui[&id.0], ui);
}

#[test]
fn snapshot_is_released_on_follow_leaving_removal_and_quit() {
    for action in [
        Action::FollowNewest,
        Action::ToggleFollow,
        Action::Back,
        Action::OpenBoard,
        Action::ReplaceThreads(vec![]),
        Action::Quit,
    ] {
        let (mut state, _) = paused();
        reduce(&mut state, action);
        assert!(state.reading_snapshot.is_none());
    }
}

#[test]
fn older_failure_keeps_readable_snapshot_and_authoritative_activity_for_retry() {
    let (mut state, id) = paused();
    older_request(&mut state, &id, "ancient");
    let before = visible(&state, &id);
    reduce(
        &mut state,
        Action::OlderConversationFailed {
            thread_id: id.clone(),
            turn_cursor: Some("turn-ancient".into()),
            item_cursor: Some("item-ancient".into()),
            error: "older read refused".into(),
        },
    );
    assert_eq!(visible(&state, &id), before);
    assert!(!state.displayed_conversation(&id.0).unwrap().loading_older);
    assert!(state.conversations[&id.0].error.is_none());
    assert_eq!(
        state.current_conversation().unwrap().active_turn_id(),
        Some("active-now")
    );
    assert!(state.active_history_watches.contains(&id.0));
    assert!(state.watched_conversations.contains(&id.0));
    assert!(
        state
            .mutation_notice
            .as_deref()
            .unwrap()
            .contains("older read refused")
    );
    older_request(&mut state, &id, "ancient");
}
