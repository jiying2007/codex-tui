//! Retain a displaced manual reading position without merging recent and old cursors.
use super::*;
use crate::conversation::ConversationPage;

#[derive(Clone, Debug)]
pub(crate) struct ReadingSnapshot {
    conversation: ConversationState,
    was_registered: bool,
}

impl AppState {
    pub(crate) fn is_reading_snapshot(&self, thread_id: &str) -> bool {
        matches!(&self.view, View::Thread(id) if id.0 == thread_id)
            && self.thread_ui.get(thread_id).is_some_and(|ui| !ui.follow)
            && self
                .reading_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.conversation.thread_id.0 == thread_id)
    }

    /// Presentation only. Active-turn decisions continue to use `conversations`.
    pub(crate) fn displayed_conversation(&self, thread_id: &str) -> Option<&ConversationState> {
        if self.is_reading_snapshot(thread_id) {
            return self
                .reading_snapshot
                .as_ref()
                .map(|snapshot| &snapshot.conversation);
        }
        self.conversations.get(thread_id)
    }

    pub(super) fn displayed_conversation_and_ui(
        &mut self,
        thread_id: &str,
    ) -> (Option<&ConversationState>, &mut ThreadUiState) {
        let paused = self.is_reading_snapshot(thread_id);
        let conversation = if paused {
            self.reading_snapshot
                .as_ref()
                .map(|snapshot| &snapshot.conversation)
        } else {
            self.conversations.get(thread_id)
        };
        let ui = self.thread_ui.entry(thread_id.to_string()).or_default();
        (conversation, ui)
    }

    pub(super) fn displayed_conversation_mut(
        &mut self,
        thread_id: &str,
    ) -> Option<&mut ConversationState> {
        if self.is_reading_snapshot(thread_id) {
            return self
                .reading_snapshot
                .as_mut()
                .map(|snapshot| &mut snapshot.conversation);
        }
        self.conversations.get_mut(thread_id)
    }

    pub(super) fn invalidate_reading_snapshot(&mut self) {
        let Some(snapshot) = self.reading_snapshot.as_ref() else {
            return;
        };
        let id = &snapshot.conversation.thread_id.0;
        let registered = self.thread_index_by_id.contains_key(id);
        if self.should_quit
            || !self.is_reading_snapshot(id)
            || !self.conversations.contains_key(id)
            || (snapshot.was_registered && !registered)
        {
            self.reading_snapshot = None;
        } else if registered {
            self.reading_snapshot.as_mut().unwrap().was_registered = true;
        }
    }

    pub(super) fn discard_reading_snapshot(&mut self, thread_id: &ThreadId) {
        if self
            .reading_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.conversation.thread_id == *thread_id)
        {
            self.reading_snapshot = None;
        }
    }

    pub(super) fn install_latest_conversation(&mut self, page: ConversationPage) {
        let id = page.thread_id.clone();
        if self.latest_page_displaces_anchor(&page)
            && let Some(mut conversation) = self.conversations.remove(&id.0)
        {
            conversation.loading = false;
            // Move the already loaded history. Later refreshes replace only the
            // bounded recent page; they never clone or extend this snapshot.
            self.reading_snapshot = Some(ReadingSnapshot {
                conversation,
                was_registered: self.thread_index_by_id.contains_key(&id.0),
            });
        }
        self.conversations
            .entry(id.0.clone())
            .or_insert_with(|| ConversationState::loading(id))
            .replace_page(page);
    }

    fn latest_page_displaces_anchor(&mut self, page: &ConversationPage) -> bool {
        let id = &page.thread_id.0;
        if !matches!(&self.view, View::Thread(current) if current == &page.thread_id)
            || self.is_reading_snapshot(id)
            || self.thread_ui.get(id).is_none_or(|ui| ui.follow)
        {
            return false;
        }
        let Some(conversation) = self
            .conversations
            .get(id)
            .filter(|history| !history.items.is_empty() && history.error.is_none())
        else {
            return false;
        };
        let ui = self.thread_ui.get_mut(id).unwrap();
        if !conversation.items.iter().any(|item| {
            ui.scroll_anchor.as_deref() == Some(item.item_id.as_str())
                && ui
                    .scroll_turn_id
                    .as_deref()
                    .is_none_or(|turn| turn == item.turn_id)
        }) {
            let (width, height) = self.thread_viewport_size.get();
            crate::conversation_viewport::scroll(conversation, ui, 0, width, height);
        }
        !page.items.iter().any(|item| {
            ui.scroll_anchor.as_deref() == Some(item.item_id.as_str())
                && ui
                    .scroll_turn_id
                    .as_deref()
                    .is_none_or(|turn| turn == item.turn_id)
        })
    }

    fn older_cursor_matches(
        &self,
        thread_id: &ThreadId,
        turn_cursor: &Option<String>,
        item_cursor: &Option<String>,
    ) -> bool {
        self.displayed_conversation(&thread_id.0)
            .is_some_and(|history| {
                &history.next_turn_cursor == turn_cursor && &history.next_item_cursor == item_cursor
            })
    }

    pub(super) fn install_older_conversation(
        &mut self,
        page: ConversationPage,
        turn_cursor: Option<String>,
        item_cursor: Option<String>,
    ) -> bool {
        let id = page.thread_id.clone();
        if !self.older_cursor_matches(&id, &turn_cursor, &item_cursor) {
            return false;
        }
        self.displayed_conversation_mut(&id.0)
            .unwrap()
            .prepend_page(page);
        true
    }

    pub(super) fn older_conversation_failed(
        &mut self,
        thread_id: ThreadId,
        turn_cursor: Option<String>,
        item_cursor: Option<String>,
        error: String,
    ) {
        if !self.older_cursor_matches(&thread_id, &turn_cursor, &item_cursor) {
            return;
        }
        self.displayed_conversation_mut(&thread_id.0)
            .unwrap()
            .loading_older = false;
        // A failed older read is not evidence about the current turn or watch.
        self.mutation_notice = Some(format!(
            "{}: {error}",
            local_text(
                self.language,
                "older history read failed",
                "读取较早历史失败"
            )
        ));
    }
}

#[cfg(test)]
mod tests;
