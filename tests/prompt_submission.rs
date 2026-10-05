//! User-intent regressions against the real reducer; no external account is used.
use codex_tui::{
    app::{Action, AppState, Effect, InputMode, reduce},
    backend::{CodexBackend, FakeBackend},
    conversation::ConversationPage,
    domain::ThreadId,
};

fn ready() -> (AppState, ThreadId, u64) {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    reduce(&mut app, Action::QuickPrompt);
    let id = app.current_thread_id().unwrap().clone();
    reduce(
        &mut app,
        Action::ConversationLoaded(ConversationPage {
            thread_id: id.clone(),
            title: None,
            turns: vec![],
            items: vec![],
            next_turn_cursor: None,
            next_item_cursor: None,
        }),
    );
    reduce(&mut app, Action::SetDraft("sent message".into()));
    let effects = reduce(&mut app, Action::CommitInput);
    let [Effect::SubmitPrompt { request_id, .. }] = effects.as_slice() else {
        panic!("expected a submit effect");
    };
    (app, id, *request_id)
}

#[test]
fn acknowledgement_preserves_newer_draft() {
    let (mut app, id, request_id) = ready();
    reduce(&mut app, Action::SetDraft("next message".into()));
    reduce(
        &mut app,
        Action::PromptSubmitted {
            thread_id: id.clone(),
            request_id,
        },
    );
    assert_eq!(app.thread_ui[&id.0].draft, "next message");
}

#[test]
fn edit_then_undo_is_still_new_operator_intent() {
    let (mut app, id, request_id) = ready();
    app.input_mode = InputMode::Composer;
    reduce(&mut app, Action::InputChar('!'));
    reduce(&mut app, Action::InputBackspace);
    reduce(
        &mut app,
        Action::PromptSubmitted {
            thread_id: id.clone(),
            request_id,
        },
    );
    assert_eq!(app.thread_ui[&id.0].draft, "sent message");
}

#[test]
fn a_pending_send_cannot_queue_a_second_turn() {
    let (mut app, _, _) = ready();
    app.input_mode = InputMode::Composer;
    let effects = reduce(&mut app, Action::CommitInput);
    assert!(
        effects.is_empty(),
        "duplicate send was accepted: {effects:?}"
    );
}

#[test]
fn acknowledgement_requires_authoritative_history_before_another_send() {
    let (mut app, id, request_id) = ready();
    reduce(
        &mut app,
        Action::PromptSubmitted {
            thread_id: id,
            request_id,
        },
    );
    reduce(&mut app, Action::SetDraft("next message".into()));
    app.input_mode = InputMode::Composer;
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
}

fn refresh(app: &mut AppState, id: &ThreadId) {
    reduce(
        app,
        Action::ConversationLoaded(ConversationPage {
            thread_id: id.clone(),
            title: None,
            turns: vec![],
            items: vec![],
            next_turn_cursor: None,
            next_item_cursor: None,
        }),
    );
}

fn submit_again(app: &mut AppState) -> u64 {
    app.input_mode = InputMode::Composer;
    let effects = reduce(app, Action::CommitInput);
    let [Effect::SubmitPrompt { request_id, .. }] = effects.as_slice() else {
        panic!("expected send: {effects:?}");
    };
    *request_id
}

#[test]
fn only_matching_acknowledgement_clears_an_unedited_draft_once() {
    let (mut app, id, request_id) = ready();
    assert!(
        reduce(
            &mut app,
            Action::PromptSubmitted {
                thread_id: id.clone(),
                request_id: request_id + 1,
            }
        )
        .is_empty()
    );
    assert_eq!(app.thread_ui[&id.0].draft, "sent message");
    assert_eq!(
        reduce(
            &mut app,
            Action::PromptSubmitted {
                thread_id: id.clone(),
                request_id,
            }
        ),
        vec![Effect::PersistOperatorState]
    );
    assert!(app.thread_ui[&id.0].draft.is_empty());
    reduce(&mut app, Action::SetDraft("later".into()));
    assert!(
        reduce(
            &mut app,
            Action::PromptSubmitted {
                thread_id: id.clone(),
                request_id,
            }
        )
        .is_empty()
    );
    assert_eq!(app.thread_ui[&id.0].draft, "later");
}

#[test]
fn stale_success_and_failure_cannot_resolve_a_new_submission() {
    let (mut app, id, old) = ready();
    reduce(
        &mut app,
        Action::PromptFailed {
            thread_id: id.clone(),
            request_id: old,
            error: "outcome uncertain".into(),
        },
    );
    app.input_mode = InputMode::Composer;
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(app.thread_ui[&id.0].draft, "sent message");
    refresh(&mut app, &id);
    let new = submit_again(&mut app);
    assert_ne!(old, new);
    reduce(
        &mut app,
        Action::PromptSubmitted {
            thread_id: id.clone(),
            request_id: old,
        },
    );
    reduce(
        &mut app,
        Action::PromptFailed {
            thread_id: id.clone(),
            request_id: old,
            error: "stale failure".into(),
        },
    );
    assert!(app.conversations[&id.0].error.is_none());
    assert_eq!(app.thread_ui[&id.0].draft, "sent message");
    app.input_mode = InputMode::Composer;
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    reduce(
        &mut app,
        Action::PromptSubmitted {
            thread_id: id.clone(),
            request_id: new,
        },
    );
    assert!(app.thread_ui[&id.0].draft.is_empty());
}

#[test]
fn pasted_draft_survives_background_ack_after_navigation() {
    let (mut app, id, request_id) = ready();
    app.input_mode = InputMode::Composer;
    reduce(&mut app, Action::InputText("\n你好，next step".into()));
    let expected = app.thread_ui[&id.0].draft.clone();
    reduce(&mut app, Action::Back);
    let view = app.view.clone();
    reduce(
        &mut app,
        Action::PromptSubmitted {
            thread_id: id.clone(),
            request_id,
        },
    );
    assert_eq!(app.view, view);
    assert_eq!(app.thread_ui[&id.0].draft, expected);
    assert_eq!(app.to_local_state().thread_ui[&id.0].draft, expected);
}

#[test]
fn unrelated_history_or_failure_does_not_clear_a_pending_ticket() {
    let (mut app, id, request_id) = ready();
    refresh(&mut app, &id);
    reduce(
        &mut app,
        Action::PromptFailed {
            thread_id: ThreadId::new("other-thread"),
            request_id,
            error: "other failure".into(),
        },
    );
    app.input_mode = InputMode::Composer;
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
}
