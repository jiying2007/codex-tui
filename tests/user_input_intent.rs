//! Exercise actual editor/reducer and renderer paths, not a parallel UI model.
use codex_tui::{
    app::{Action, AppState, Effect, InputMode, View, reduce},
    backend::{CodexBackend, FakeBackend},
    conversation::{
        InteractiveRequest, InteractiveRequestKind, InteractiveResolution, RpcRequestId,
        UserInputQuestion, parse_interactive_request,
    },
    ui,
};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

fn question(id: &str, secret: bool) -> UserInputQuestion {
    UserInputQuestion {
        id: id.into(),
        header: "Header".into(),
        question: format!("Question {id}?"),
        is_secret: secret,
        options: vec![],
    }
}
fn setup() -> (AppState, InteractiveRequest) {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.host_local_only = false;
    let request = InteractiveRequest {
        request_id: RpcRequestId::String("form-1".into()),
        thread_id: app.threads[0].id.clone(),
        turn_id: "turn-1".into(),
        item_id: "item-1".into(),
        kind: InteractiveRequestKind::UserInput {
            questions: vec![question("q1", true), question("q2", false)],
        },
    };
    app.view = View::Thread(request.thread_id.clone());
    reduce(&mut app, Action::InteractiveRequested(request.clone()));
    reduce(&mut app, Action::BeginUserInput);
    (app, request)
}
fn questions(request: &mut InteractiveRequest) -> &mut Vec<UserInputQuestion> {
    let InteractiveRequestKind::UserInput { questions } = &mut request.kind else {
        panic!("form");
    };
    questions
}
fn collect_first(app: &mut AppState) {
    reduce(app, Action::InputText("first-secret".into()));
    assert!(reduce(app, Action::CommitInput).is_empty());
    reduce(app, Action::InputText("second-answer".into()));
}
fn refuse_changed(change: impl FnOnce(&mut InteractiveRequest)) {
    let (mut app, mut request) = setup();
    collect_first(&mut app);
    change(&mut request);
    reduce(&mut app, Action::InteractiveRequested(request));
    assert!(
        reduce(&mut app, Action::CommitInput).is_empty(),
        "must not emit stale answers"
    );
    assert_eq!(app.input_buffer, "second-answer");
    assert_eq!(app.input_mode, InputMode::UserInput);
    assert!(app.mutation_notice.is_some());
}
#[test]
fn changed_question_text_refuses_old_answers() {
    refuse_changed(|r| questions(r)[1].question = "Different purpose?".into());
}
#[test]
fn reordered_questions_refuse_old_answers() {
    refuse_changed(|r| questions(r).reverse());
}
#[test]
fn changed_question_id_refuses_old_answers() {
    refuse_changed(|r| questions(r)[1].id = "new-id".into());
}
#[test]
fn changed_options_refuse_old_answers() {
    refuse_changed(|r| questions(r)[1].options = vec!["choice".into()]);
}
#[test]
fn changed_turn_refuses_old_answers() {
    refuse_changed(|r| r.turn_id = "turn-2".into());
}
#[test]
fn changed_item_refuses_old_answers() {
    refuse_changed(|r| r.item_id = "item-2".into());
}
#[test]
fn changed_request_thread_refuses_old_answers() {
    refuse_changed(|r| r.thread_id.0 = "another-thread".into());
}
#[test]
fn replacement_approval_preserves_unsent_answer_without_responding() {
    refuse_changed(|r| {
        r.kind = InteractiveRequestKind::FileChangeApproval {
            reason: None,
            context: serde_json::Value::Null.into(),
        }
    });
}
#[test]
fn replaced_secret_flag_cannot_unmask_existing_answer() {
    let (mut app, mut request) = setup();
    reduce(&mut app, Action::InputText("PRIVATE_VALUE_42".into()));
    questions(&mut request)[0].is_secret = false;
    reduce(&mut app, Action::InteractiveRequested(request));
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let display = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        !display.contains("PRIVATE_VALUE_42"),
        "private answer leaked into screen"
    );
    assert!(display.contains("********"));
    assert!(app.current_user_input_question().unwrap().is_secret);
}
#[test]
fn removed_thread_cannot_submit_cached_request_answers() {
    let (mut app, request) = setup();
    collect_first(&mut app);
    let threads = app
        .threads
        .iter()
        .filter(|t| t.id != request.thread_id)
        .cloned()
        .collect();
    reduce(&mut app, Action::ReplaceThreads(threads));
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(app.input_buffer, "second-answer");
}
#[test]
fn different_active_thread_cannot_submit_original_answers() {
    let (mut app, _) = setup();
    collect_first(&mut app);
    app.view = View::Thread(app.threads[1].id.clone());
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(app.input_buffer, "second-answer");
}
#[test]
fn duplicate_delivery_keeps_pending_request_order() {
    let (mut app, request) = setup();
    let mut later = request.clone();
    later.request_id = RpcRequestId::Integer(99);
    reduce(&mut app, Action::InteractiveRequested(later));
    reduce(&mut app, Action::InteractiveRequested(request.clone()));
    assert_eq!(app.current_pending_request(), Some(&request));
}
#[test]
fn duplicate_delivery_preserves_collected_answers_and_sends_once() {
    let (mut app, request) = setup();
    collect_first(&mut app);
    reduce(&mut app, Action::InteractiveRequested(request.clone()));
    let effects = reduce(&mut app, Action::CommitInput);
    assert_eq!(effects.len(), 1);
    let Effect::ResolveInteractive {
        request_id,
        resolution: InteractiveResolution::UserInput(answers),
    } = &effects[0]
    else {
        panic!("answers");
    };
    assert_eq!(request_id, &request.request_id);
    assert_eq!(answers["q1"], vec!["first-secret"]);
    assert_eq!(answers["q2"], vec!["second-answer"]);
    let submission = app
        .user_response_submission(
            request_id,
            &InteractiveResolution::UserInput(answers.clone()),
        )
        .unwrap();
    assert_eq!(app.input_mode, InputMode::UserInput);
    app.finish_user_response(
        submission.ticket,
        codex_tui::user_response::UserResponseOutcome::Written,
    );
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
}
#[test]
fn cancelling_conflicted_form_allows_explicit_fresh_edit() {
    let (mut app, mut request) = setup();
    collect_first(&mut app);
    questions(&mut request)[0].id = "replacement".into();
    reduce(&mut app, Action::InteractiveRequested(request));
    reduce(&mut app, Action::CancelInput);
    assert!(app.input_buffer.is_empty());
    assert!(app.current_user_input_question().is_none());
    reduce(&mut app, Action::BeginUserInput);
    assert_eq!(app.current_user_input_question().unwrap().id, "replacement");
}
#[test]
fn resolved_form_clears_its_secret_editor() {
    let (mut app, request) = setup();
    reduce(&mut app, Action::InputText("private".into()));
    reduce(
        &mut app,
        Action::InteractiveResolved {
            request_id: request.request_id,
        },
    );
    assert!(app.input_buffer.is_empty());
    assert!(app.current_user_input_question().is_none());
    assert_eq!(app.input_mode, InputMode::Normal);
}
fn parse_questions(qs: serde_json::Value) -> anyhow::Result<Option<InteractiveRequest>> {
    parse_interactive_request(
        &json!({"id": 7,"method":"item/tool/requestUserInput", "params":{
        "threadId":"t", "turnId":"turn", "itemId":"item", "questions": qs }}),
    )
}
#[test]
fn ambiguous_wire_question_ids_are_refused() {
    assert!(
        parse_questions(json!([{"id":"q", "question":"One?"},{"id":"q", "question":"Two?"}]))
            .is_err()
    );
}
#[test]
fn empty_wire_forms_and_ids_are_refused() {
    for value in [
        json!([]),
        json!([{"id":"", "question":"One?"}]),
        json!([{"id":"  ", "question":"One?"}]),
    ] {
        assert!(parse_questions(value).is_err());
    }
}
#[test]
fn optional_legacy_fields_and_extensions_remain_supported() {
    let request =
        parse_questions(json!([{"id":"q", "question":"One?", "future": {"anything": true}}]))
            .unwrap()
            .unwrap();
    let InteractiveRequestKind::UserInput { questions } = request.kind else {
        panic!("questions");
    };
    assert!(!questions[0].is_secret);
    assert!(questions[0].options.is_empty());
}
#[test]
fn ambiguous_typed_form_cannot_enter_editor() {
    let (mut app, mut request) = setup();
    reduce(&mut app, Action::CancelInput);
    *questions(&mut request) = vec![question("same", false), question("same", false)];
    reduce(&mut app, Action::InteractiveRequested(request));
    reduce(&mut app, Action::BeginUserInput);
    assert_ne!(app.input_mode, InputMode::UserInput);
    assert!(app.mutation_notice.is_some());
}

#[test]
fn repeat_begin_keeps_partially_collected_answers() {
    let (mut app, _) = setup();
    collect_first(&mut app);
    reduce(&mut app, Action::BeginUserInput);
    assert_eq!(app.input_buffer, "second-answer");
    assert_eq!(app.current_user_input_question().unwrap().id, "q2");
    assert_eq!(reduce(&mut app, Action::CommitInput).len(), 1);
}

#[test]
fn late_form_resolution_does_not_clear_an_unrelated_note_editor() {
    let (mut app, request) = setup();
    // Exercise the resolution handler's ownership contract directly.
    app.input_mode = InputMode::Note;
    app.input_buffer = "keep my note".into();
    reduce(
        &mut app,
        Action::InteractiveResolved {
            request_id: request.request_id,
        },
    );
    assert_eq!(app.input_mode, InputMode::Note);
    assert_eq!(app.input_buffer, "keep my note");
    assert!(app.current_user_input_question().is_none());
}

#[test]
fn missing_form_metadata_defaults_to_masked_rendering() {
    let (mut app, _) = setup();
    reduce(&mut app, Action::CancelInput);
    app.input_mode = InputMode::UserInput;
    app.input_buffer = "UNOWNED_SECRET_57".into();
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let display = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(!display.contains("UNOWNED_SECRET_57"));
    assert!(display.contains("********"));
}

#[test]
fn unmasked_free_text_preserves_commas() {
    let (mut app, mut request) = setup();
    reduce(&mut app, Action::CancelInput);
    *questions(&mut request) = vec![question("free", false)];
    reduce(&mut app, Action::InteractiveRequested(request));
    reduce(&mut app, Action::BeginUserInput);
    reduce(&mut app, Action::InputText("alpha,beta".into()));
    let effects = reduce(&mut app, Action::CommitInput);
    let Effect::ResolveInteractive {
        resolution: InteractiveResolution::UserInput(answers),
        ..
    } = &effects[0]
    else {
        panic!("answer");
    };
    assert_eq!(answers["free"], vec!["alpha,beta"]);
}

#[test]
fn empty_target_identity_in_user_input_is_refused() {
    for field in ["threadId", "turnId", "itemId"] {
        let mut message = json!({"id":7,"method":"item/tool/requestUserInput", "params":{
            "threadId":"t","turnId":"turn","itemId":"item","questions":[{"id":"q","question":"one?"}]}});
        message["params"][field] = json!("");
        assert!(parse_interactive_request(&message).is_err());
    }
}
