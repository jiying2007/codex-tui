use super::*;
use crate::backend::{CodexBackend, FakeBackend};

fn idle_app() -> AppState {
    let mut threads = FakeBackend::seeded().snapshot().threads;
    for thread in &mut threads {
        thread.runtime = RuntimeStatus::Ready;
    }
    let mut state = AppState::new(threads);
    reduce(&mut state, Action::OpenSelected);
    state
}

fn empty_page(id: ThreadId) -> ConversationPage {
    ConversationPage {
        thread_id: id,
        title: None,
        turns: vec![],
        items: vec![],
        next_turn_cursor: None,
        next_item_cursor: None,
    }
}

#[test]
fn cancelled_search_preserves_watch_but_committed_search_releases_it() {
    let mut state = idle_app();
    let id = state.current_thread_id().unwrap().clone();
    assert!(reduce(&mut state, Action::BeginSearch).is_empty());
    assert!(state.watched_conversations.contains(&id.0));
    assert!(reduce(&mut state, Action::CancelInput).is_empty());
    assert!(state.watched_conversations.contains(&id.0));
    reduce(&mut state, Action::BeginSearch);
    let effects = reduce(&mut state, Action::CommitInput);
    assert!(effects.contains(&Effect::StopWatchingConversation(id)));
    assert!(state.watched_conversations.is_empty());
}

#[test]
fn reachable_nested_returns_retain_watch_but_stale_returns_do_not() {
    let mut state = idle_app();
    let id = state.current_thread_id().unwrap().clone();
    reduce(&mut state, Action::OpenBoard);
    reduce(&mut state, Action::BeginSearch);
    assert!(state.watched_conversations.contains(&id.0));
    reduce(&mut state, Action::CancelInput);
    assert_eq!(state.view, View::Board);
    reduce(&mut state, Action::Back);
    assert_eq!(state.view, View::Thread(id.clone()));
    assert!(state.watched_conversations.contains(&id.0));
    state.review_return_view = Some(View::Thread(id.clone()));
    reduce(&mut state, Action::Back);
    assert!(state.watched_conversations.is_empty());
    // An unreachable old return slot must not reacquire or retain an owner.
    assert!(reachable_threads(&state).is_empty());
}

#[test]
fn cyclic_return_slots_are_bounded_and_deleted_current_thread_is_not_an_owner() {
    let mut state = idle_app();
    let id = state.current_thread_id().unwrap().clone();
    state.view = View::Review(id.clone());
    state.review_return_view = Some(View::Workspace(id.clone()));
    state.workspace_return_view = Some(View::Review(id.clone()));
    assert_eq!(reachable_threads(&state), BTreeSet::from([id.0.clone()]));
    let rows = state
        .threads
        .iter()
        .filter(|row| row.id != id)
        .cloned()
        .collect();
    let effects = reduce(&mut state, Action::ReplaceThreads(rows));
    assert!(effects.contains(&Effect::StopWatchingConversation(id)));
    assert!(state.watched_conversations.is_empty());
}

#[test]
fn acknowledged_prompt_keeps_watch_until_authoritative_history_or_failure() {
    for fails in [false, true] {
        let mut state = idle_app();
        let id = state.current_thread_id().unwrap().clone();
        reduce(
            &mut state,
            Action::ConversationLoaded(empty_page(id.clone())),
        );
        reduce(&mut state, Action::QuickPrompt);
        reduce(&mut state, Action::SetDraft("pending prompt".into()));
        let effects = reduce(&mut state, Action::CommitInput);
        let [Effect::SubmitPrompt { request_id, .. }] = effects.as_slice() else {
            panic!("submission expected");
        };
        reduce(&mut state, Action::Back);
        let ack = reduce(
            &mut state,
            Action::PromptSubmitted {
                thread_id: id.clone(),
                request_id: *request_id,
            },
        );
        assert!(!ack.contains(&Effect::StopWatchingConversation(id.clone())));
        assert!(state.watched_conversations.contains(&id.0));
        let final_action = if fails {
            Action::ConversationFailed {
                thread_id: id.clone(),
                error: "fixture".into(),
            }
        } else {
            Action::ConversationLoaded(empty_page(id.clone()))
        };
        let effects = reduce(&mut state, final_action);
        assert_eq!(effects, vec![Effect::StopWatchingConversation(id)]);
        assert!(state.watched_conversations.is_empty());
        assert_eq!(state.prompt_submissions.pending_thread_ids().count(), 0);
    }
}
