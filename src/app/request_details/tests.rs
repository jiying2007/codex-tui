use super::*;
use crate::{
    app::{Action, reduce},
    conversation::{
        InteractiveRequestKind, InteractiveResolution, RpcRequestId, parse_interactive_request,
    },
    domain::ThreadId,
    i18n::UiLanguage,
};
use serde_json::json;

fn request() -> InteractiveRequest {
    parse_interactive_request(&json!({
        "id": 7, "method": "item/commandExecution/requestApproval", "params": {
            "threadId": "details-thread", "turnId": "turn", "itemId": "item",
            "command": "echo first\n    second\n\nCOMMAND-END",
            "cwd": "/repo\nCWD-END", "reason": "reason\nREASON-END\u{0007}",
            "additionalPermissions": {"fileSystem": {"write": ["/safe\nSCOPE-END"]}, "future": {"secret": "DO-NOT-DISPLAY"}}
        }
    })).unwrap().unwrap()
}

fn state(request: InteractiveRequest) -> AppState {
    let mut app = AppState::new(vec![]);
    app.view = View::Thread(request.thread_id.clone());
    app.pending_requests.push(request);
    app
}

#[test]
fn details_preserve_safe_newlines_and_all_known_scope_without_revealing_opaque_values() {
    let request = request();
    let text = request_lines(&request, UiLanguage::English, true).join("\n");
    assert!(text.contains("echo first\n    second\n\nCOMMAND-END"));
    assert!(text.contains("cwd: /repo\nCWD-END"));
    assert!(text.contains("reason: reason\nREASON-END�"));
    assert!(text.contains("filesystem write: /safe\nSCOPE-END"));
    assert!(text.contains("other permission category: future · details hidden"));
    assert!(!text.contains("DO-NOT-DISPLAY"));
    assert!(!text.contains('\u{0007}'));
    let summary = request_lines(&request, UiLanguage::English, false).join("\n");
    assert!(summary.starts_with("NEEDS YOU · d full details"));
    assert!(summary.contains("echo first     second  COMMAND-END"));
}

#[test]
fn details_include_every_user_input_question_and_option() {
    let request = parse_interactive_request(&json!({
        "id": 9, "method": "item/tool/requestUserInput", "params": {
            "threadId": "details-thread", "turnId": "turn", "itemId": "input",
            "questions": [
                {"id": "q1", "header": "First", "question": "First line\nSECOND-LINE", "options": [{"label": "A"}]},
                {"id": "q2", "header": "Second", "question": "LAST-QUESTION", "isSecret": true,
                 "options": [{"label": "B\nOPTION-END"}, {"label": "FINAL-OPTION"}]}
            ]
        }
    })).unwrap().unwrap();
    let text = request_lines(&request, UiLanguage::English, true).join("\n");
    for expected in [
        "First: First line\nSECOND-LINE",
        "Second: LAST-QUESTION",
        "B\nOPTION-END",
        "FINAL-OPTION",
    ] {
        assert!(text.contains(expected), "missing {expected}");
    }
}

#[test]
fn details_browse_and_close_do_not_respond_even_through_direct_actions() {
    let request = request();
    let mut app = state(request.clone());
    assert!(reduce(&mut app, Action::OpenRequestDetails).is_empty());
    assert!(app.request_details_open());
    for action in [
        Action::ScrollRequestDetails(1),
        Action::PageRequestDetails(1),
        Action::RequestDetailsBoundary(true),
        Action::RequestDetailsBoundary(false),
        Action::ResolvePending(InteractiveResolution::Accept),
        Action::ResolvePending(InteractiveResolution::Decline),
        Action::ResolvePending(InteractiveResolution::Cancel),
        Action::BeginUserInput,
    ] {
        assert!(reduce(&mut app, action).is_empty());
        assert_eq!(app.current_pending_request(), Some(&request));
        assert!(app.request_details_open());
    }
    assert!(reduce(&mut app, Action::CloseRequestDetails).is_empty());
    assert!(!app.request_details_open());
    assert_eq!(app.current_pending_request(), Some(&request));
    assert_eq!(app.view, View::Thread(ThreadId::new("details-thread")));
}

#[test]
fn details_invalidate_when_payload_changes_request_resolves_or_thread_is_left() {
    let mut current = request();
    let mut app = state(current.clone());
    reduce(&mut app, Action::OpenRequestDetails);
    if let InteractiveRequestKind::CommandApproval { command, .. } = &mut current.kind {
        *command = "REPLACED-COMMAND".into();
    }
    reduce(&mut app, Action::InteractiveRequested(current.clone()));
    assert!(app.request_details.is_none());
    reduce(&mut app, Action::OpenRequestDetails);
    let mut next = current.clone();
    next.request_id = RpcRequestId::Integer(8);
    reduce(&mut app, Action::InteractiveRequested(next.clone()));
    assert!(app.request_details_open());
    reduce(
        &mut app,
        Action::InteractiveResolved {
            request_id: current.request_id,
        },
    );
    assert!(app.request_details.is_none());
    assert_eq!(app.current_pending_request(), Some(&next));
    reduce(&mut app, Action::OpenRequestDetails);
    reduce(&mut app, Action::Back);
    assert!(app.request_details.is_none());
    assert_eq!(app.pending_requests, vec![next]);
}

#[test]
fn details_cannot_open_over_an_editor_or_action_panel() {
    let mut app = state(request());
    app.input_mode = InputMode::Composer;
    reduce(&mut app, Action::OpenRequestDetails);
    assert!(!app.request_details_open());
    app.input_mode = InputMode::Normal;
    app.goal_actions_open = true;
    reduce(&mut app, Action::OpenRequestDetails);
    assert!(!app.request_details_open());
    app.goal_actions_open = false;
    app.view = View::Registry;
    reduce(&mut app, Action::OpenRequestDetails);
    assert!(!app.request_details_open());
}

#[test]
fn newline_heavy_details_reach_past_u16_rows_with_only_a_visible_window() {
    let mut request = request();
    if let InteractiveRequestKind::CommandApproval {
        command,
        context,
        cwd,
        reason,
    } = &mut request.kind
    {
        *command = format!("{}TAIL-AFTER-70000", "x\n".repeat(70_000));
        *context = crate::conversation::ApprovalContext::from(json!({}));
        cwd.clear();
        *reason = None;
    }
    let mut app = state(request);
    reduce(&mut app, Action::OpenRequestDetails);
    reduce(&mut app, Action::RequestDetailsBoundary(true));
    let details = app.active_request_details().unwrap();
    let (rows, top, total) = details.visible(32, 2);
    assert_eq!(rows.len(), 2);
    assert!(top > usize::from(u16::MAX));
    assert!(total > usize::from(u16::MAX));
    assert!(rows.last().unwrap().contains("TAIL-AFTER-70000"));
    reduce(&mut app, Action::RequestDetailsBoundary(false));
    let (rows, top, _) = app.active_request_details().unwrap().visible(32, 2);
    assert_eq!(top, 0);
    assert_eq!(rows[0], "NEEDS YOU");
}
