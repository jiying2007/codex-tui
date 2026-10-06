use super::*;
use codex_tui::{
    app::{InputMode, View},
    backend::{CodexBackend, FakeBackend},
    conversation::{InteractiveRequest, parse_interactive_request},
};
use serde_json::json;
fn ready(method: &str, resolution: InteractiveResolution) -> (AppState, InteractiveRequest) {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let request = parse_interactive_request(&json!({"id":"approval","method":method,"params":{
        "threadId":app.threads[0].id.0,"turnId":"t","itemId":"i","command":"echo test",
        "questions":[{"id":"q","question":"Q?"}]}}))
    .unwrap()
    .unwrap();
    app.view = View::Thread(request.thread_id.clone());
    reduce(&mut app, Action::InteractiveRequested(request.clone()));
    if method == "item/tool/requestUserInput" {
        reduce(&mut app, Action::BeginUserInput);
        reduce(&mut app, Action::InputText("kept".into()));
    }
    assert_eq!(
        reduce(&mut app, Action::ResolvePending(resolution)).len(),
        1
    );
    (app, request)
}
#[test]
fn absent_backend_reports_refusal_and_allows_explicit_decision_retry() {
    let (mut app, r) = ready(
        "item/commandExecution/requestApproval",
        InteractiveResolution::Accept,
    );
    submit(
        &mut app,
        None,
        &r.request_id,
        &InteractiveResolution::Accept,
    );
    assert!(app.pending_requests.contains(&r));
    assert!(app.mutation_notice.as_ref().unwrap().contains("not sent"));
    assert_eq!(
        reduce(
            &mut app,
            Action::ResolvePending(InteractiveResolution::Accept)
        )
        .len(),
        1
    );
}
#[test]
fn full_and_closed_admission_preserve_the_owned_input_editor() {
    for closed in [false, true] {
        let (mut app, r) = ready("item/tool/requestUserInput", InteractiveResolution::Cancel);
        let s = app
            .user_response_submission(&r.request_id, &InteractiveResolution::Cancel)
            .unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tx.try_send(s).unwrap();
        let _rx = if closed {
            drop(rx);
            None
        } else {
            Some(rx)
        };
        submit_with(
            &mut app,
            &r.request_id,
            &InteractiveResolution::Cancel,
            |s| tx.try_send(s).map_err(|e| e.to_string()),
        );
        assert_eq!(app.input_mode, InputMode::UserInput);
        assert_eq!(app.input_buffer, "kept");
        assert_eq!(
            reduce(
                &mut app,
                Action::ResolvePending(InteractiveResolution::Cancel)
            )
            .len(),
            1
        );
    }
}
#[test]
fn uncertain_approval_blocks_all_competing_decisions_and_late_refusal() {
    let (mut app, r) = ready(
        "item/commandExecution/requestApproval",
        InteractiveResolution::Accept,
    );
    let ticket = app
        .user_response_submission(&r.request_id, &InteractiveResolution::Accept)
        .unwrap()
        .ticket;
    app.finish_user_response(ticket, UserResponseOutcome::Unknown("partial".into()));
    app.finish_user_response(ticket, UserResponseOutcome::NotSent("late".into()));
    for decision in [
        InteractiveResolution::Accept,
        InteractiveResolution::Decline,
        InteractiveResolution::Cancel,
    ] {
        assert!(reduce(&mut app, Action::ResolvePending(decision)).is_empty());
    }
    assert!(app.pending_requests.contains(&r));
}
#[test]
fn old_approval_receipt_does_not_retire_a_replacement_request() {
    let (mut app, mut r) = ready(
        "item/commandExecution/requestApproval",
        InteractiveResolution::Accept,
    );
    let ticket = app
        .user_response_submission(&r.request_id, &InteractiveResolution::Accept)
        .unwrap()
        .ticket;
    r.item_id = "replacement".into();
    reduce(&mut app, Action::InteractiveRequested(r.clone()));
    app.finish_user_response(ticket, UserResponseOutcome::Written);
    assert!(app.pending_requests.contains(&r));
}
#[test]
fn cancellation_receipt_clears_only_unchanged_owned_editor() {
    for changed in [false, true] {
        let (mut app, r) = ready("item/tool/requestUserInput", InteractiveResolution::Cancel);
        let ticket = app
            .user_response_submission(&r.request_id, &InteractiveResolution::Cancel)
            .unwrap()
            .ticket;
        if changed {
            reduce(&mut app, Action::InputChar('!'));
            reduce(&mut app, Action::InputBackspace);
        }
        app.finish_user_response(ticket, UserResponseOutcome::Written);
        assert!(!app.pending_requests.contains(&r));
        assert_eq!(
            app.input_mode,
            if changed {
                InputMode::UserInput
            } else {
                InputMode::Normal
            }
        );
        assert_eq!(app.input_buffer, if changed { "kept" } else { "" });
    }
}
#[test]
fn actor_stop_fences_approval_once_without_fabricating_resolution() {
    let (mut app, r) = ready(
        "item/commandExecution/requestApproval",
        InteractiveResolution::Decline,
    );
    assert!(app.user_response_actor_stopped());
    assert!(!app.user_response_actor_stopped());
    assert!(app.pending_requests.contains(&r));
    assert!(
        reduce(
            &mut app,
            Action::ResolvePending(InteractiveResolution::Decline)
        )
        .is_empty()
    );
}
