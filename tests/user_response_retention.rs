use codex_tui::{
    app::{Action, AppState, InputMode, View, reduce},
    backend::{CodexBackend, FakeBackend},
    conversation::{
        InteractiveRequest, InteractiveRequestKind, InteractiveResolution, RpcRequestId,
        UserInputQuestion,
    },
    ui,
};
use ratatui::{Terminal, backend::TestBackend};
fn setup() -> (AppState, InteractiveRequest) {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.host_local_only = false;
    let request = InteractiveRequest {
        request_id: RpcRequestId::String("response-1".into()),
        thread_id: app.threads[0].id.clone(),
        turn_id: "turn".into(),
        item_id: "item".into(),
        kind: InteractiveRequestKind::UserInput {
            questions: vec![UserInputQuestion {
                id: "q".into(),
                header: "Header".into(),
                question: "Private answer?".into(),
                is_secret: true,
                options: vec![],
            }],
        },
    };
    app.view = View::Thread(request.thread_id.clone());
    reduce(&mut app, Action::InteractiveRequested(request.clone()));
    reduce(&mut app, Action::BeginUserInput);
    reduce(&mut app, Action::InputText("SYNTHETIC_PRIVATE_84".into()));
    (app, request)
}
#[test]
fn final_answer_is_retained_before_any_backend_admission() {
    let (mut app, _) = setup();
    assert_eq!(reduce(&mut app, Action::CommitInput).len(), 1);
    assert_eq!(app.input_mode, InputMode::UserInput);
    assert_eq!(app.input_buffer, "SYNTHETIC_PRIVATE_84");
}
#[test]
fn pending_answer_keeps_original_secret_screen() {
    let (mut app, _) = setup();
    reduce(&mut app, Action::CommitInput);
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal.draw(|f| ui::render(f, &app)).unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(
        screen.contains("********"),
        "pending answer editor disappeared before receipt"
    );
    assert!(!screen.contains("SYNTHETIC_PRIVATE_84"));
}
#[test]
fn explicit_close_does_not_authorize_duplicate_pending_answer() {
    let (mut app, _) = setup();
    reduce(&mut app, Action::CommitInput);
    reduce(&mut app, Action::CancelInput);
    reduce(&mut app, Action::BeginUserInput);
    reduce(&mut app, Action::InputText("second".into()));
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
}
#[test]
fn pending_answer_cannot_race_a_second_cancel_response() {
    let (mut app, _) = setup();
    reduce(&mut app, Action::CommitInput);
    assert!(
        reduce(
            &mut app,
            Action::ResolvePending(InteractiveResolution::Cancel)
        )
        .is_empty()
    );
}
#[test]
fn repeated_enter_without_a_receipt_never_submits_twice() {
    let (mut app, _) = setup();
    assert_eq!(reduce(&mut app, Action::CommitInput).len(), 1);
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
}
