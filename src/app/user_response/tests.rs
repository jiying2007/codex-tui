use super::*;
use crate::{
    app::View,
    backend::{CodexBackend, FakeBackend},
    conversation::{InteractiveRequest, InteractiveRequestKind, UserInputQuestion},
};
fn editor() -> AppState {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let request = InteractiveRequest {
        request_id: RpcRequestId::String("r".into()),
        thread_id: app.threads[0].id.clone(),
        turn_id: "t".into(),
        item_id: "i".into(),
        kind: InteractiveRequestKind::UserInput {
            questions: vec![UserInputQuestion {
                id: "q".into(),
                header: "H".into(),
                question: "Q".into(),
                is_secret: false,
                options: vec![],
            }],
        },
    };
    app.view = View::Thread(request.thread_id.clone());
    crate::app::reduce(&mut app, Action::InteractiveRequested(request));
    crate::app::reduce(&mut app, Action::BeginUserInput);
    crate::app::reduce(&mut app, Action::InputText("retained".into()));
    app
}
#[test]
fn ticket_exhaustion_does_not_wrap_or_drop_answer() {
    let mut app = editor();
    app.user_responses.next_ticket = u64::MAX;
    assert!(crate::app::reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(app.input_buffer, "retained");
    assert!(app.user_responses.attempts.is_empty());
}
#[test]
fn ledger_capacity_never_evicts_uncertain_entries() {
    let mut app = editor();
    crate::app::reduce(&mut app, Action::CommitInput);
    let mut attempt = app.user_responses.attempts.values().next().unwrap().clone();
    attempt.unknown = true;
    app.user_responses.attempts.clear();
    for i in 0..LIMIT {
        app.user_responses.attempts.insert(
            RpcRequestId::String(format!("uncertain-{i}")),
            attempt.clone(),
        );
    }
    assert!(crate::app::reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(app.user_responses.attempts.len(), LIMIT);
    assert_eq!(app.input_buffer, "retained");
}
