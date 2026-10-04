use super::*;
use crate::conversation::{ConversationItem, ConversationItemKind, ConversationPage};
use crate::domain::ThreadId;

fn page(text: &str) -> ConversationPage {
    ConversationPage {
        thread_id: ThreadId::new("cache-thread"),
        title: Some("cache".into()),
        turns: vec![],
        items: vec![ConversationItem {
            turn_id: "turn-1".into(),
            item_id: "item-1".into(),
            kind: ConversationItemKind::User,
            text: text.into(),
            status: None,
        }],
        next_turn_cursor: None,
        next_item_cursor: None,
    }
}

#[test]
fn presentation_cache_invalidates_on_conversation_revision() {
    let mut conversation = ConversationState::loading(ThreadId::new("cache-thread"));
    conversation.replace_page(page("before"));
    let before = formatted_items(&conversation, 0, 1);
    assert!(before[0].text.contains("before"));

    conversation.replace_page(page("after"));
    let after = formatted_items(&conversation, 0, 1);
    assert!(after[0].text.contains("after"));
    assert!(!Arc::ptr_eq(&before, &after));
}

#[test]
fn long_history_window_is_bounded_and_tracks_item_offset() {
    let (start, end) = item_window(10_000, 9_000, 40);
    assert_eq!(start, 9_000);
    assert!(end > start);
    assert!(end - start <= MAX_WINDOW_ITEMS);
    assert!(end - start >= MIN_WINDOW_ITEMS);
}

#[test]
fn oversized_offset_clamps_to_last_item() {
    assert_eq!(item_window(5, u16::MAX, 10), (4, 5));
}

#[test]
fn reloaded_thread_cannot_reuse_an_old_instance_presentation() {
    let mut before = ConversationState::loading(ThreadId::new("cache-thread"));
    before.replace_page(page("old instance"));
    let old = formatted_items(&before, 0, 1);
    let mut after = ConversationState::loading(ThreadId::new("cache-thread"));
    after.replace_page(page("new instance"));
    let new = formatted_items(&after, 0, 1);
    assert!(old[0].text.contains("old instance"));
    assert!(new[0].text.contains("new instance"));
}

#[test]
fn cold_presentation_formats_only_the_selected_window() {
    let mut conversation = ConversationState::loading(ThreadId::new("cache-thread"));
    let mut history = page("large history");
    history.items = (0..10_000)
        .map(|index| {
            let mut item = history.items[0].clone();
            item.item_id = format!("item-{index}");
            item
        })
        .collect();
    conversation.replace_page(history);
    let (start, end) = item_window(10_000, 9000, 40);
    let items = formatted_items(&conversation, start, end);
    assert!(items.len() <= MAX_WINDOW_ITEMS);
    assert_eq!(items[0].item_id, "item-9000");
}
