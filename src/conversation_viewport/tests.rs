use super::*;
use crate::conversation::{ConversationItem, ConversationItemKind, ConversationPage};
use crate::domain::ThreadId;
use unicode_segmentation::UnicodeSegmentation;

fn page(id: &str, text: &str) -> ConversationPage {
    ConversationPage {
        thread_id: ThreadId::new("viewport-thread"),
        title: None,
        turns: vec![],
        items: vec![ConversationItem {
            turn_id: "turn".into(),
            item_id: id.into(),
            kind: ConversationItemKind::Assistant,
            text: text.into(),
            status: None,
        }],
        next_turn_cursor: None,
        next_item_cursor: None,
    }
}

fn state(text: &str) -> ConversationState {
    let mut state = ConversationState::loading(ThreadId::new("viewport-thread"));
    state.replace_page(page("item", text));
    state
}

fn visible(state: &ConversationState, ui: &ThreadUiState, width: u16, height: u16) -> String {
    viewport(state, ui, width, height)
        .visible()
        .iter()
        .map(|(_, _, text)| *text)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn every_line_of_one_long_message_is_reachable_in_both_directions() {
    let text = (0..200)
        .map(|n| format!("LINE-{n:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let state = state(&text);
    let mut ui = ThreadUiState {
        follow: false,
        ..Default::default()
    };
    let mut seen = String::new();
    for _ in 0..42 {
        seen.push_str(&visible(&state, &ui, 40, 8));
        scroll(&state, &mut ui, 5, 40, 8);
    }
    for n in 0..200 {
        assert!(seen.contains(&format!("LINE-{n:03}")), "missing line {n}");
    }
    for _ in 0..40 {
        scroll(&state, &mut ui, -5, 40, 8);
    }
    assert!(viewport(&state, &ui, 40, 8).at_start());
    assert!(visible(&state, &ui, 40, 8).contains("LINE-000"));
}

#[test]
fn follow_uses_new_tail_while_manual_scroll_preserves_the_reading_position() {
    let text = (0..80)
        .map(|n| format!("LINE-{n:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut state = state(&text);
    let following = ThreadUiState::default();
    assert!(visible(&state, &following, 40, 8).contains("LINE-079"));
    let mut manual = following.clone();
    scroll(&state, &mut manual, -5, 40, 8);
    let before = visible(&state, &manual, 40, 8);
    state.replace_page(page("item", &format!("{text}\nNEW-TAIL")));
    assert_eq!(visible(&state, &manual, 40, 8), before);
    assert!(visible(&state, &following, 40, 8).contains("NEW-TAIL"));
}

#[test]
fn byte_anchor_survives_resize_and_item_anchor_survives_prepend() {
    let text = "0123456789".repeat(100);
    let mut state = state(&text);
    let mut ui = ThreadUiState {
        follow: false,
        ..Default::default()
    };
    scroll(&state, &mut ui, 20, 30, 8);
    let offset = ui.scroll_offset;
    let resized = viewport(&state, &ui, 47, 8);
    let item = &resized.items[0];
    let row = item.rows.from(&item.text, resized.top).next().unwrap();
    assert!(row.start <= offset && offset < row.end);
    let before = visible(&state, &ui, 47, 8);
    state.prepend_page(page("older", "older history"));
    assert_eq!(visible(&state, &ui, 47, 8), before);
    assert_eq!(ui.scroll_anchor.as_deref(), Some("item"));
}

#[test]
fn long_history_formats_a_bounded_window_and_resolves_exact_item_anchors() {
    let mut history = page("item", "large history");
    let template = history.items[0].clone();
    history.items = (0..10_000)
        .map(|n| ConversationItem {
            item_id: format!("item-{n}"),
            ..template.clone()
        })
        .collect();
    let mut state = state("");
    state.replace_page(history);
    let ui = ThreadUiState {
        follow: false,
        scroll_anchor: Some("item-9000".into()),
        ..Default::default()
    };
    let view = viewport(&state, &ui, 80, 38);
    assert!(view.items.len() <= MAX_WINDOW_ITEMS);
    assert!(view.items.len() >= MIN_WINDOW_ITEMS);
    assert_eq!(view.visible()[0].1, "item-9000");
    let tail = viewport(&state, &ThreadUiState::default(), 80, 38);
    assert_eq!(tail.visible().last().unwrap().1, "item-9999");
}

#[test]
fn cache_invalidates_for_revised_and_reloaded_instances() {
    let mut before = state("before");
    let first = format_items(&before, 0, 1, 40);
    before.replace_page(page("item", "after"));
    let revised = format_items(&before, 0, 1, 40);
    let reloaded = state("new instance");
    let new = format_items(&reloaded, 0, 1, 40);
    assert!(first[0].text.contains("before"));
    assert!(revised[0].text.contains("after"));
    assert!(new[0].text.contains("new instance"));
    assert!(!Arc::ptr_eq(&first, &revised));
}

#[test]
fn multiline_code_and_graphemes_are_preserved_without_terminal_controls() {
    let original = "one\r\n\n\tlet x = 中e\u{301}👩‍💻;\n\u{1b}[2J\u{202e}end";
    let state = state(original);
    let items = format_items(&state, 0, 1, 14);
    let item = &items[0];
    assert!(item.text.contains("\n\n    let"));
    for character in ['\u{1b}', '\u{202e}', '\r', '\t'] {
        assert!(!item.text.contains(character));
    }
    let boundaries = item
        .text
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .chain([item.text.len()])
        .collect::<Vec<_>>();
    for row in item.rows.from(&item.text, 0) {
        assert!(boundaries.contains(&row.start));
        assert!(boundaries.contains(&row.end));
        assert!(crate::text::display_width(&item.text[row.clone()]) <= 14);
    }
    assert_eq!(state.items[0].text, original);
}

#[test]
fn very_long_single_line_and_empty_viewport_are_safe() {
    let state = state(&format!("{}TAIL", "x".repeat(200_000)));
    assert!(visible(&state, &ThreadUiState::default(), 80, 8).contains("TAIL"));
    assert!(
        viewport(&state, &ThreadUiState::default(), 0, 0)
            .visible()
            .is_empty()
    );
    let mut empty = ConversationState::loading(ThreadId::new("empty"));
    empty.replace_page(ConversationPage {
        items: vec![],
        ..page("", "")
    });
    assert!(viewport(&empty, &ThreadUiState::default(), 80, 8).at_start());
}

#[test]
fn persisted_view_state_accepts_the_previous_field_set() {
    let ui: ThreadUiState =
        serde_json::from_str(r#"{"draft":"draft","scroll":7,"follow":false}"#).unwrap();
    assert_eq!(ui.scroll, 7);
    assert_eq!(ui.scroll_anchor, None);
    assert_eq!(ui.scroll_offset, 0);
}

#[test]
fn repeated_item_ids_in_different_turns_keep_distinct_anchors() {
    let mut history = page("duplicate", "earlier turn");
    let mut later = history.items[0].clone();
    later.turn_id = "second-turn".into();
    later.text = "later turn\nrow two\nrow three".into();
    history.items.push(later);
    let mut state = state("");
    state.replace_page(history);
    let mut ui = ThreadUiState {
        follow: false,
        scroll: 1,
        ..Default::default()
    };
    scroll(&state, &mut ui, 1, 40, 2);
    assert_eq!(ui.scroll_turn_id.as_deref(), Some("second-turn"));
    assert!(visible(&state, &ui, 40, 2).starts_with("row two"));
}
