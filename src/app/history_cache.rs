//! Rehydrate a valid return target without coupling ownership to the LRU.
use super::*;

impl AppState {
    pub(super) fn observe_history_activity(&mut self, id: &ThreadId) {
        let active = self.watched_conversations.contains(&id.0)
            && self
                .conversations
                .get(&id.0)
                .is_some_and(|conversation| conversation.active_turn_id().is_some());
        if active {
            self.active_history_watches.insert(id.0.clone());
        } else {
            self.active_history_watches.remove(&id.0);
        }
    }

    pub(super) fn rehydrate_evicted_thread(&mut self) -> Option<Effect> {
        let View::Thread(thread_id) = &self.view else {
            return None;
        };
        if self.conversations.contains_key(&thread_id.0)
            || !(self.thread_index_by_id.contains_key(&thread_id.0)
                || self.unregistered_watches.contains(&thread_id.0))
        {
            return None;
        }
        let id = thread_id.clone();
        let hit = self.transcript_search_active_hit.as_ref().filter(|hit| {
            hit.thread_id == id
                && hit.turn_id.is_some()
                && hit.item_id.is_some()
                && self.thread_ui.get(&id.0).is_some_and(|ui| !ui.follow)
        });
        let effect = hit
            .cloned()
            .map(Effect::JumpToTranscriptHit)
            .unwrap_or_else(|| Effect::LoadConversation(id.clone()));
        // Loading/error entries suppress duplicate reads. Admission failure is
        // handled by the normal runtime path, never retried as a write.
        self.prepare_conversation(&id);
        Some(effect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    fn page(id: ThreadId) -> ConversationPage {
        ConversationPage {
            thread_id: id,
            title: None,
            turns: vec![],
            items: vec![],
            next_turn_cursor: None,
            next_item_cursor: None,
        }
    }

    fn state(provisional: bool) -> (AppState, ThreadId) {
        let mut rows = FakeBackend::seeded().snapshot().threads;
        for row in &mut rows {
            row.runtime = RuntimeStatus::Ready;
        }
        let mut state = AppState::new(rows);
        let id = if provisional {
            ThreadId::new("provisional-cache")
        } else {
            state.threads[0].id.clone()
        };
        state.view = View::Thread(id.clone());
        state.prepare_conversation(&id);
        watch::reconcile(&mut state, vec![Effect::LoadConversation(id.clone())]);
        reduce(&mut state, Action::ConversationLoaded(page(id.clone())));
        (state, id)
    }

    fn evict(state: &mut AppState, id: &ThreadId) {
        for i in 0..CONVERSATION_CACHE_LIMIT + 1 {
            let other = ThreadId::new(format!("background-{i}"));
            reduce(state, Action::ConversationLoaded(page(other)));
        }
        assert!(!state.conversations.contains_key(&id.0));
        assert_eq!(state.conversations.len(), CONVERSATION_CACHE_LIMIT);
    }

    #[test]
    fn search_and_board_returns_reload_evicted_registered_and_provisional_history_once() {
        for provisional in [false, true] {
            for board in [false, true] {
                let (mut state, id) = state(provisional);
                if board {
                    reduce(&mut state, Action::OpenBoard);
                }
                reduce(&mut state, Action::BeginSearch);
                evict(&mut state, &id);
                let mut effects = reduce(&mut state, Action::CancelInput);
                if board {
                    assert!(effects.is_empty());
                    effects = reduce(&mut state, Action::Back);
                }
                assert_eq!(effects, vec![Effect::LoadConversation(id.clone())]);
                assert!(state.conversations[&id.0].loading);
                assert_eq!(state.conversations.len(), CONVERSATION_CACHE_LIMIT);
                assert!(state.rehydrate_evicted_thread().is_none());
                reduce(
                    &mut state,
                    Action::ConversationFailed {
                        thread_id: id.clone(),
                        error: "admission refused".into(),
                    },
                );
                assert!(state.rehydrate_evicted_thread().is_none());
                assert_eq!(
                    state.conversations[&id.0].error.as_deref(),
                    Some("admission refused")
                );
            }
        }
    }

    #[test]
    fn cached_and_retired_targets_and_repository_views_do_not_reload() {
        let (mut state, id) = state(true);
        assert!(state.rehydrate_evicted_thread().is_none());
        state.conversations.clear();
        state.conversation_cache_order.clear();
        for view in [
            View::Board,
            View::Review(id.clone()),
            View::Workspace(id.clone()),
        ] {
            state.view = view;
            assert!(state.rehydrate_evicted_thread().is_none());
        }
        state.view = View::Thread(id.clone());
        state.unregistered_watches.remove(&id.0);
        assert!(state.rehydrate_evicted_thread().is_none());
        assert!(state.conversations.is_empty());
    }

    #[test]
    fn evicted_search_hit_keeps_exact_anchor_unless_following_newest() {
        for follow in [false, true] {
            let (mut state, id) = state(true);
            let hit = TranscriptSearchHit {
                thread_id: id.clone(),
                turn_id: Some("turn-old".into()),
                item_id: Some("item-old".into()),
                snippet: "exact old occurrence".into(),
                turn_cursor: None,
                match_start_utf16: None,
                match_end_utf16: None,
            };
            state.transcript_search_active_hit = Some(hit.clone());
            state.thread_ui.entry(id.0.clone()).or_default().follow = follow;
            reduce(&mut state, Action::BeginSearch);
            evict(&mut state, &id);
            let expected = if follow {
                Effect::LoadConversation(id)
            } else {
                Effect::JumpToTranscriptHit(hit)
            };
            assert_eq!(reduce(&mut state, Action::CancelInput), vec![expected]);
        }
    }
    fn active_page(id: ThreadId) -> ConversationPage {
        let mut page = page(id);
        page.turns.push(crate::conversation::ConversationTurn {
            id: "working".into(),
            status: "inProgress".into(),
            started_at: None,
            completed_at: None,
            error: None,
        });
        page
    }

    #[test]
    fn history_activity_survives_eviction_but_completion_failure_and_quit_release_it() {
        for provisional in [false, true] {
            for completion in [true, false] {
                let (mut state, id) = state(provisional);
                reduce(
                    &mut state,
                    Action::ConversationLoaded(active_page(id.clone())),
                );
                reduce(&mut state, Action::Back);
                assert!(state.watched_conversations.contains(&id.0));
                evict(&mut state, &id);
                assert!(state.active_history_watches.contains(&id.0));
                assert!(state.watched_conversations.contains(&id.0));
                let action = if completion {
                    Action::ConversationLoaded(page(id.clone()))
                } else {
                    Action::ConversationFailed {
                        thread_id: id.clone(),
                        error: "failed read".into(),
                    }
                };
                assert_eq!(
                    reduce(&mut state, action),
                    vec![Effect::StopWatchingConversation(id.clone())]
                );
                assert!(state.active_history_watches.is_empty());
                // A late unowned history completion is presentation data, not a new watch.
                reduce(
                    &mut state,
                    Action::ConversationLoaded(active_page(id.clone())),
                );
                assert!(state.active_history_watches.is_empty());
                assert!(state.watched_conversations.is_empty());
            }
            let (mut state, id) = state(provisional);
            reduce(
                &mut state,
                Action::ConversationLoaded(active_page(id.clone())),
            );
            state.should_quit = true;
            watch::reconcile(&mut state, vec![]);
            assert!(state.active_history_watches.is_empty());
            assert!(state.watched_conversations.is_empty());
        }
    }

    #[test]
    fn oldest_registered_activity_remains_owned_when_more_than_the_cache_limit_are_active() {
        let template = FakeBackend::seeded().snapshot().threads[0].clone();
        let mut rows = Vec::new();
        for i in 0..32 {
            let mut row = template.clone();
            row.id = ThreadId::new(format!("active-{i}"));
            row.runtime = RuntimeStatus::Ready;
            rows.push(row);
        }
        let mut state = AppState::new(rows);
        for i in 0..32 {
            let id = ThreadId::new(format!("active-{i}"));
            state.view = View::Thread(id.clone());
            state.prepare_conversation(&id);
            watch::reconcile(&mut state, vec![Effect::LoadConversation(id.clone())]);
            reduce(&mut state, Action::ConversationLoaded(active_page(id)));
            reduce(&mut state, Action::Back);
        }
        assert_eq!(state.active_history_watches.len(), 32);
        assert_eq!(state.watched_conversations.len(), 32);
        assert_eq!(state.conversations.len(), CONVERSATION_CACHE_LIMIT);
        for i in 0..32 {
            let id = ThreadId::new(format!("active-{i}"));
            reduce(&mut state, Action::ConversationLoaded(page(id)));
        }
        assert!(state.active_history_watches.is_empty());
        assert!(state.watched_conversations.is_empty());
    }
}
