use codex_tui::{
    app::{Action, AppState, View, reduce},
    backend::{CodexBackend, FakeBackend},
    conversation::{InteractiveRequest, InteractiveResolution, parse_interactive_request},
};
use serde_json::{Value, json};

fn setup(method: &str) -> (AppState, InteractiveRequest, Value) {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let wire = json!({"id":"approval", "method":method, "params":{
        "threadId":app.threads[0].id.0, "turnId":"turn", "itemId":"item",
        "command":"echo approved", "cwd":"/repo", "permissions":{"network":{"enabled":true}},
        "questions":[{"id":"q","question":"Answer?"}]
    }});
    let request = parse_interactive_request(&wire).unwrap().unwrap();
    app.view = View::Thread(request.thread_id.clone());
    reduce(&mut app, Action::InteractiveRequested(request.clone()));
    (app, request, wire)
}

#[test]
fn approval_waits_for_explicit_action() {
    let (app, request, _) = setup("item/commandExecution/requestApproval");
    assert!(app.pending_requests.contains(&request));
}
#[test]
fn repeated_accept_is_fenced_before_backend_admission() {
    let (mut app, _, _) = setup("item/commandExecution/requestApproval");
    assert_eq!(
        reduce(
            &mut app,
            Action::ResolvePending(InteractiveResolution::Accept)
        )
        .len(),
        1
    );
    assert!(
        reduce(
            &mut app,
            Action::ResolvePending(InteractiveResolution::Accept)
        )
        .is_empty()
    );
}
#[test]
fn pending_accept_cannot_race_cancel() {
    let (mut app, _, _) = setup("item/fileChange/requestApproval");
    reduce(
        &mut app,
        Action::ResolvePending(InteractiveResolution::Accept),
    );
    assert!(
        reduce(
            &mut app,
            Action::ResolvePending(InteractiveResolution::Cancel)
        )
        .is_empty()
    );
}
#[test]
fn repeated_user_input_cancel_is_fenced() {
    let (mut app, _, _) = setup("item/tool/requestUserInput");
    assert_eq!(
        reduce(
            &mut app,
            Action::ResolvePending(InteractiveResolution::Cancel)
        )
        .len(),
        1
    );
    assert!(
        reduce(
            &mut app,
            Action::ResolvePending(InteractiveResolution::Cancel)
        )
        .is_empty()
    );
}
#[test]
fn same_id_permission_expansion_changes_request_identity() {
    let (_, original, mut wire) = setup("item/permissions/requestApproval");
    wire["params"]["permissions"]["network"]["host"] = json!("different.example");
    assert_ne!(original, parse_interactive_request(&wire).unwrap().unwrap());
}
#[test]
fn same_command_additional_permissions_change_request_identity() {
    let (_, original, mut wire) = setup("item/commandExecution/requestApproval");
    wire["params"]["additionalPermissions"] = json!({"fileSystem":{"write":["/different"]}});
    assert_ne!(original, parse_interactive_request(&wire).unwrap().unwrap());
}
#[test]
fn same_id_file_grant_root_changes_request_identity() {
    let (_, original, mut wire) = setup("item/fileChange/requestApproval");
    wire["params"]["grantRoot"] = json!("/different");
    assert_ne!(original, parse_interactive_request(&wire).unwrap().unwrap());
}
#[test]
fn independent_requests_remain_independently_actionable() {
    let (mut app, request, mut wire) = setup("item/commandExecution/requestApproval");
    wire["id"] = json!("other");
    reduce(
        &mut app,
        Action::ResolvePending(InteractiveResolution::Accept),
    );
    reduce(
        &mut app,
        Action::InteractiveResolved {
            request_id: request.request_id,
        },
    );
    let other = parse_interactive_request(&wire).unwrap().unwrap();
    reduce(&mut app, Action::InteractiveRequested(other));
    assert_eq!(
        reduce(
            &mut app,
            Action::ResolvePending(InteractiveResolution::Decline)
        )
        .len(),
        1
    );
}
