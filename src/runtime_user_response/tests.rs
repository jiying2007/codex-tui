use super::*;
use codex_tui::{
    app::{Effect, InputMode, View},
    backend::{CodexBackend, FakeBackend},
    conversation::{
        InteractiveRequest, InteractiveRequestKind, InteractiveResolution, UserInputQuestion,
    },
    user_response::UserResponseAnswers,
};
fn ready() -> (AppState, RpcRequestId, UserResponseAnswers) {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let request = InteractiveRequest {
        request_id: RpcRequestId::String("form".into()),
        thread_id: app.threads[0].id.clone(),
        turn_id: "t".into(),
        item_id: "i".into(),
        kind: InteractiveRequestKind::UserInput {
            questions: vec![UserInputQuestion {
                id: "q".into(),
                header: "H".into(),
                question: "Q?".into(),
                is_secret: true,
                options: vec![],
            }],
        },
    };
    app.view = View::Thread(request.thread_id.clone());
    reduce(&mut app, Action::InteractiveRequested(request));
    reduce(&mut app, Action::BeginUserInput);
    reduce(&mut app, Action::InputText("answer".into()));
    let effect = reduce(&mut app, Action::CommitInput).pop().unwrap();
    let Effect::ResolveInteractive {
        request_id,
        resolution: InteractiveResolution::UserInput(answers),
    } = effect
    else {
        panic!("response")
    };
    (app, request_id, answers)
}
#[test]
fn missing_backend_preserves_answer_and_allows_explicit_retry() {
    let (mut app, id, answers) = ready();
    submit(
        &mut app,
        None,
        &id,
        &InteractiveResolution::UserInput(answers.clone()),
    );
    assert_eq!(app.input_mode, InputMode::UserInput);
    assert_eq!(app.input_buffer, "answer");
    assert!(
        app.user_response_submission(&id, &InteractiveResolution::UserInput(answers.clone()))
            .is_none()
    );
    assert_eq!(reduce(&mut app, Action::CommitInput).len(), 1);
}
#[test]
fn actual_bounded_queue_refusal_is_not_a_write_receipt() {
    let (mut app, id, answers) = ready();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    tx.try_send(
        app.user_response_submission(&id, &InteractiveResolution::UserInput(answers.clone()))
            .unwrap(),
    )
    .unwrap();
    submit_with(
        &mut app,
        &id,
        &InteractiveResolution::UserInput(answers.clone()),
        |s| tx.try_send(s).map_err(|e| e.to_string()),
    );
    assert_eq!(app.input_buffer, "answer");
    assert!(
        app.user_response_submission(&id, &InteractiveResolution::UserInput(answers.clone()))
            .is_none()
    );
    rx.try_recv().unwrap();
    reduce(&mut app, Action::CommitInput);
    submit_with(
        &mut app,
        &id,
        &InteractiveResolution::UserInput(answers.clone()),
        |s| tx.try_send(s).map_err(|e| e.to_string()),
    );
    assert!(
        app.user_response_submission(&id, &InteractiveResolution::UserInput(answers.clone()))
            .is_some()
    );
    assert_eq!(app.input_mode, InputMode::UserInput);
}
#[test]
fn old_and_unrelated_receipts_cannot_clear_answer() {
    let (mut app, id, answers) = ready();
    let first = app
        .user_response_submission(&id, &InteractiveResolution::UserInput(answers.clone()))
        .unwrap();
    app.finish_user_response(first.ticket, UserResponseOutcome::NotSent("full".into()));
    reduce(&mut app, Action::CommitInput);
    let second = app
        .user_response_submission(&id, &InteractiveResolution::UserInput(answers.clone()))
        .unwrap();
    assert_ne!(first.ticket, second.ticket);
    app.finish_user_response(first.ticket, UserResponseOutcome::Written);
    assert_eq!(app.input_buffer, "answer");
    app.finish_user_response(second.ticket, UserResponseOutcome::Written);
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.input_buffer.is_empty());
}
#[test]
fn later_edit_and_edit_undo_survive_success() {
    for undo in [false, true] {
        let (mut app, id, answers) = ready();
        let ticket = app
            .user_response_submission(&id, &InteractiveResolution::UserInput(answers.clone()))
            .unwrap()
            .ticket;
        reduce(&mut app, Action::InputChar('!'));
        if undo {
            reduce(&mut app, Action::InputBackspace);
        }
        app.finish_user_response(ticket, UserResponseOutcome::Written);
        assert_eq!(app.input_mode, InputMode::UserInput);
        assert_eq!(app.input_buffer, if undo { "answer" } else { "answer!" });
    }
}
#[test]
fn uncertain_response_blocks_retry_but_keeps_answer_and_explicit_close() {
    let (mut app, id, answers) = ready();
    let ticket = app
        .user_response_submission(&id, &InteractiveResolution::UserInput(answers.clone()))
        .unwrap()
        .ticket;
    app.finish_user_response(ticket, UserResponseOutcome::Unknown("broken pipe".into()));
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(app.input_buffer, "answer");
    reduce(&mut app, Action::CancelInput);
    assert_eq!(app.input_mode, InputMode::Normal);
    reduce(&mut app, Action::BeginUserInput);
    assert_eq!(app.input_mode, InputMode::Normal);
}
#[test]
fn disconnect_is_reported_once_and_never_retried() {
    let (mut app, _, _) = ready();
    assert!(app.user_response_actor_stopped());
    assert!(!app.user_response_actor_stopped());
    assert_eq!(app.input_buffer, "answer");
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
}
#[test]
fn replaced_form_is_not_removed_by_old_success() {
    let (mut app, id, answers) = ready();
    let original = app
        .user_response_submission(&id, &InteractiveResolution::UserInput(answers.clone()))
        .unwrap();
    let mut replacement = original.request.clone();
    replacement.item_id = "new-item".into();
    reduce(&mut app, Action::InteractiveRequested(replacement.clone()));
    app.finish_user_response(original.ticket, UserResponseOutcome::Written);
    assert!(app.pending_requests.contains(&replacement));
}
#[test]
fn explicit_close_is_not_resurrected_by_failure() {
    let (mut app, id, answers) = ready();
    let ticket = app
        .user_response_submission(&id, &InteractiveResolution::UserInput(answers.clone()))
        .unwrap()
        .ticket;
    reduce(&mut app, Action::CancelInput);
    app.finish_user_response(ticket, UserResponseOutcome::NotSent("full".into()));
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.input_buffer.is_empty());
}

#[test]
fn duplicate_refusal_cannot_clear_an_uncertain_attempt() {
    let (mut app, id, answers) = ready();
    let ticket = app
        .user_response_submission(&id, &InteractiveResolution::UserInput(answers.clone()))
        .unwrap()
        .ticket;
    app.finish_user_response(ticket, UserResponseOutcome::Unknown("partial write".into()));
    app.finish_user_response(
        ticket,
        UserResponseOutcome::NotSent("duplicate command".into()),
    );
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(app.input_buffer, "answer");
}
