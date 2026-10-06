use codex_tui::{
    app::{Action, AppState, View, reduce},
    backend::{CodexBackend, FakeBackend},
    conversation::{InteractiveRequestKind, parse_interactive_request},
    ui,
};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

fn screen(wire: serde_json::Value) -> String {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let request = parse_interactive_request(&wire).unwrap().unwrap();
    app.view = View::Thread(request.thread_id.clone());
    reduce(&mut app, Action::InteractiveRequested(request));
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn base(method: &str) -> serde_json::Value {
    json!({"id":"approval","method":method,"params":{
        "threadId":"thread-1","turnId":"turn","itemId":"item"
    }})
}

#[test]
fn approval_context_exposes_known_scope_but_not_unknown_values() {
    let mut wire = base("item/permissions/requestApproval");
    wire["params"]["permissions"] = json!({
        "network":{"host":"api.example.com"},
        "fileSystem":{"read":["/safe/read"],"write":["/safe/write"]},
        "mystery":{"token":"DO_NOT_RENDER"}
    });
    wire["params"]["grantRoot"] = json!("/workspace");
    wire["params"]["additionalPermissions"] =
        json!({"fileSystem":{"write":["/extra/write"]},"camera":{"secret":"HIDDEN"}});
    let request = parse_interactive_request(&wire).unwrap().unwrap();
    let InteractiveRequestKind::PermissionsApproval { context, .. } = request.kind else {
        panic!("permissions approval")
    };
    let scope = context.visible_scope();
    assert!(scope.contains(&("network-host".into(), "api.example.com".into())));
    assert!(scope.contains(&("filesystem-read".into(), "/safe/read".into())));
    assert!(scope.contains(&("filesystem-write".into(), "/safe/write".into())));
    assert!(scope.contains(&("filesystem-write".into(), "/extra/write".into())));
    assert!(scope.contains(&("grant-root".into(), "/workspace".into())));
    assert!(scope.contains(&("permission-category".into(), "mystery".into())));
    assert!(scope.contains(&("permission-category".into(), "camera".into())));
    assert!(!format!("{scope:?}").contains("DO_NOT_RENDER"));
    assert!(!format!("{scope:?}").contains("HIDDEN"));
}

#[test]
fn approval_overlay_shows_scope_and_sanitizes_untrusted_text() {
    let mut wire = base("item/commandExecution/requestApproval");
    wire["params"]["command"] = json!("echo safe\nINJECTED");
    wire["params"]["cwd"] = json!("/repo\tchild");
    wire["params"]["reason"] = json!("because\u{0007}bell");
    wire["params"]["additionalPermissions"] = json!({
        "network":{"hosts":["one.example","two.example"]},
        "fileSystem":{"write":["/repo/out"]},
        "unknown":{"credential":"NEVER_SHOW"}
    });
    let rendered = screen(wire);
    assert!(rendered.contains("echo safe INJECTED"));
    assert!(rendered.contains("/repo child"));
    assert!(rendered.contains("because�bell"));
    assert!(rendered.contains("network host: one.example"));
    assert!(rendered.contains("network host: two.example"));
    assert!(rendered.contains("filesystem write: /repo/out"));
    assert!(rendered.contains("other permission category: unknown · details hidden"));
    assert!(!rendered.contains("NEVER_SHOW"));
    assert!(!rendered.contains('\u{0007}'));
}

#[test]
fn user_input_prompt_text_cannot_inject_layout_controls() {
    let mut wire = base("item/tool/requestUserInput");
    wire["params"]["questions"] = json!([{
        "id":"q",
        "header":"Header\nInjected",
        "question":"Question\tBody",
        "options":[{"label":"A\rB"},{"label":"C\u{0007}D"}]
    }]);
    let rendered = screen(wire);
    assert!(rendered.contains("Header Injected: Question Body"));
    assert!(rendered.contains("A B, C�D"));
    assert!(!rendered.contains('\u{0007}'));
}
