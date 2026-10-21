//! Conversation navigation changes only local presentation anchors.
use super::*;

pub(super) fn scroll(state: &mut AppState, delta: i16) -> Vec<Effect> {
    if let Some(id) = state.current_thread_id().cloned() {
        let (width, height) = state.thread_viewport_size.get();
        let stride = i16::try_from(height.max(1)).unwrap_or(i16::MAX);
        let delta = delta.clamp(-stride, stride);
        let (conversation, ui) = state.displayed_conversation_and_ui(&id.0);
        let previous_ui = ui.clone();
        let at_start = conversation.map_or(ui.scroll == 0, |conversation| {
            crate::conversation_viewport::viewport(conversation, ui, width, height).at_start()
        });
        if delta.is_negative() && at_start {
            if let Some(conversation) = conversation {
                crate::conversation_viewport::scroll(conversation, ui, 0, width, height);
            }
            let older_request = conversation.and_then(|conversation| {
                (!conversation.loading_older
                    && (conversation.next_turn_cursor.is_some()
                        || conversation.next_item_cursor.is_some()))
                .then(|| {
                    (
                        conversation.next_turn_cursor.clone(),
                        conversation.next_item_cursor.clone(),
                    )
                })
            });
            if let Some((turn_cursor, item_cursor)) = older_request {
                if let Some(conversation) = state.displayed_conversation_mut(&id.0) {
                    conversation.loading_older = true;
                }
                return vec![Effect::LoadOlderConversation {
                    thread_id: id,
                    turn_cursor,
                    item_cursor,
                }];
            }
            return if state.thread_ui.get(&id.0) == Some(&previous_ui) {
                vec![]
            } else {
                vec![Effect::PersistOperatorStateDeferred]
            };
        }

        if let Some(conversation) =
            conversation.filter(|conversation| !conversation.items.is_empty())
        {
            crate::conversation_viewport::scroll(conversation, ui, delta, width, height);
        } else {
            ui.follow = false;
            ui.scroll = if delta.is_negative() {
                ui.scroll.saturating_sub(delta.unsigned_abs())
            } else {
                ui.scroll.saturating_add(delta as u16)
            };
        }
        return vec![Effect::PersistOperatorStateDeferred];
    }
    vec![]
}

pub(super) fn toggle_follow(state: &mut AppState) -> Vec<Effect> {
    if let Some(id) = state.current_thread_id().cloned() {
        let (width, height) = state.thread_viewport_size.get();
        let (conversation, ui) = state.displayed_conversation_and_ui(&id.0);
        if ui.follow {
            if let Some(conversation) = conversation {
                crate::conversation_viewport::scroll(conversation, ui, 0, width, height);
            }
            ui.follow = false;
        } else {
            ui.follow = true;
        }
        return vec![Effect::PersistOperatorState];
    }
    vec![]
}

pub(super) fn position(state: &mut AppState, follow: bool) -> Vec<Effect> {
    if let Some(id) = state.current_thread_id().cloned() {
        let ui = state.thread_ui.entry(id.0).or_default();
        ui.follow = follow;
        ui.scroll = 0;
        ui.scroll_anchor = None;
        ui.scroll_turn_id = None;
        ui.scroll_offset = 0;
        return vec![Effect::PersistOperatorStateDeferred];
    }
    vec![]
}
