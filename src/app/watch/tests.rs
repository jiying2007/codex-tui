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
        reduce(
            &mut state,
            Action::ConversationLoaded(empty_page(id.clone())),
        );
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

#[test]
fn failed_prompt_propagates_nested_watch_cleanup_without_replaying_the_send() {
    let mut state = idle_app();
    let id = state.current_thread_id().unwrap().clone();
    reduce(
        &mut state,
        Action::ConversationLoaded(empty_page(id.clone())),
    );
    reduce(&mut state, Action::QuickPrompt);
    reduce(
        &mut state,
        Action::ConversationLoaded(empty_page(id.clone())),
    );
    reduce(&mut state, Action::SetDraft("retain failed draft".into()));
    let effects = reduce(&mut state, Action::CommitInput);
    let [Effect::SubmitPrompt { request_id, .. }] = effects.as_slice() else {
        panic!("submission expected");
    };
    reduce(&mut state, Action::Back);
    let effects = reduce(
        &mut state,
        Action::PromptFailed {
            thread_id: id.clone(),
            request_id: *request_id,
            error: "fixture".into(),
        },
    );
    assert_eq!(effects, vec![Effect::StopWatchingConversation(id.clone())]);
    assert!(state.watched_conversations.is_empty());
    assert_eq!(state.thread_ui[&id.0].draft, "retain failed draft");
}

#[test]
fn unregistered_history_can_resolve_but_deleted_or_failed_watches_are_released() {
    for resolves in [false, true] {
        let mut state = idle_app();
        reduce(&mut state, Action::Back);
        let id = ThreadId::new("not-yet-in-registry");
        state.view = View::Thread(id.clone());
        let effects = reconcile(&mut state, vec![Effect::LoadConversation(id.clone())]);
        assert_eq!(effects, vec![Effect::LoadConversation(id.clone())]);
        let rows = state.threads.clone();
        assert!(reduce(&mut state, Action::ReplaceThreads(rows)).is_empty());
        let effects = if resolves {
            let mut rows = state.threads.clone();
            let mut registered = rows[0].clone();
            registered.id = id.clone();
            rows.push(registered);
            assert!(reduce(&mut state, Action::ReplaceThreads(rows)).is_empty());
            assert!(state.unregistered_watches.is_empty());
            let rows = state
                .threads
                .iter()
                .filter(|row| row.id != id)
                .cloned()
                .collect();
            reduce(&mut state, Action::ReplaceThreads(rows))
        } else {
            reduce(
                &mut state,
                Action::ConversationFailed {
                    thread_id: id.clone(),
                    error: "missing".into(),
                },
            )
        };
        assert_eq!(effects, vec![Effect::StopWatchingConversation(id)]);
        assert!(state.watched_conversations.is_empty());
        assert!(state.unregistered_watches.is_empty());
    }
}

#[test]
fn cancelling_search_returns_only_to_a_still_owned_provisional_thread() {
    for retired in [false, true] {
        let mut state = idle_app();
        reduce(&mut state, Action::Back);
        let id = ThreadId::new("historical-only");
        state.view = View::Thread(id.clone());
        reconcile(&mut state, vec![Effect::LoadConversation(id.clone())]);
        reduce(
            &mut state,
            Action::ConversationLoaded(empty_page(id.clone())),
        );
        assert!(reduce(&mut state, Action::BeginSearch).is_empty());
        assert!(state.unregistered_watches.contains(&id.0));
        if retired {
            reduce(
                &mut state,
                Action::ConversationFailed {
                    thread_id: id.clone(),
                    error: "deleted".into(),
                },
            );
        }
        let effects = reduce(&mut state, Action::CancelInput);
        if retired {
            assert_eq!(state.view, View::Registry);
            assert!(!state.watched_conversations.contains(&id.0));
            assert!(!state.unregistered_watches.contains(&id.0));
            assert!(!effects.iter().any(|effect| matches!(
                effect,
                Effect::LoadConversation(_) | Effect::JumpToTranscriptHit(_)
            )));
        } else {
            assert_eq!(state.view, View::Thread(id.clone()));
            assert!(state.watched_conversations.contains(&id.0));
            assert!(effects.is_empty());
            assert!(!state.search_return_view_is_valid(&View::Review(id.clone())));
            assert!(!state.search_return_view_is_valid(&View::Workspace(id.clone())));
            assert!(!state.search_return_view_is_valid(&View::ManagedWorktrees(id.clone())));
        }
    }
}
