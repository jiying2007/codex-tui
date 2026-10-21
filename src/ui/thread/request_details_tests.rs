use super::*;
use crate::{
    app::{Action, View, reduce},
    conversation::{
        ConversationItem, ConversationItemKind, ConversationPage, ConversationState,
        parse_interactive_request,
    },
    domain::{ThreadId, ThreadUiState},
};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

fn app() -> AppState {
    let id = ThreadId::new("details-thread");
    let mut app = AppState::new(vec![]);
    app.view = View::Thread(id.clone());
    let mut history = ConversationState::loading(id.clone());
    history.replace_page(ConversationPage {
        thread_id: id.clone(),
        title: None,
        turns: vec![],
        items: vec![ConversationItem {
            turn_id: "turn".into(),
            item_id: "body".into(),
            kind: ConversationItemKind::Assistant,
            text: "HISTORY-STILL-READABLE".into(),
            status: None,
        }],
        next_turn_cursor: None,
        next_item_cursor: None,
    });
    app.conversations.insert(id.0.clone(), history);
    app.thread_ui.insert(id.0.clone(), ThreadUiState::default());
    let command = format!(
        "{}\n{}\nCOMMAND-END",
        (0..24)
            .map(|i| format!("COMMAND-{i:02}"))
            .collect::<Vec<_>>()
            .join("\n"),
        "wide界e\u{301}👩‍💻".repeat(20)
    );
    let request = parse_interactive_request(&json!({
        "id": 12, "method": "item/commandExecution/requestApproval", "params": {
            "threadId": id.0, "turnId": "turn", "itemId": "approval",
            "command": command,
            "cwd": format!("{}\nCWD-END", "/long/cwd/".repeat(40)),
            "reason": "reason\nREASON-END",
            "additionalPermissions": {"fileSystem": {"write": [format!("{}\nSCOPE-END", "/long/scope/".repeat(40))]}}
        }
    })).unwrap().unwrap();
    app.pending_requests.push(request);
    app
}

fn screen(app: &AppState, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| render_thread(frame, app, "details-thread", frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn small_terminals_keep_a_history_row_and_point_to_complete_request_details() {
    for height in [8, 13, 14] {
        let app = app();
        let output = screen(&app, 46, height);
        assert!(output.contains("HISTORY-STILL-READABLE"), "height {height}");
        assert!(
            output.contains("NEEDS YOU · d full details"),
            "height {height}"
        );
        assert!(output.lines().last().unwrap().starts_with("d full details"));
    }
}

#[test]
fn small_details_view_reaches_all_long_fields_and_both_endpoints() {
    for height in [3, 8] {
        let mut app = app();
        reduce(&mut app, Action::OpenRequestDetails);
        let initial = screen(&app, 38, height);
        assert!(initial.contains("NEEDS YOU"));
        let mut seen = initial;
        let mut previous = String::new();
        for _ in 0..160 {
            reduce(&mut app, Action::PageRequestDetails(1));
            let output = screen(&app, 38, height);
            seen.push_str(&output);
            if output == previous {
                break;
            }
            previous = output;
        }
        for i in 0..24 {
            assert!(
                seen.contains(&format!("COMMAND-{i:02}")),
                "height {height}, command row {i}"
            );
        }
        for tail in ["COMMAND-END", "CWD-END", "REASON-END", "SCOPE-END"] {
            assert!(seen.contains(tail), "height {height}, missing {tail}");
        }
        reduce(&mut app, Action::RequestDetailsBoundary(false));
        assert!(screen(&app, 38, height).contains("NEEDS YOU"));
        reduce(&mut app, Action::RequestDetailsBoundary(true));
        assert!(screen(&app, 38, height).contains("SCOPE-END"));
        reduce(&mut app, Action::CloseRequestDetails);
        assert!(screen(&app, 46, 14).contains("HISTORY-STILL-READABLE"));
        assert_eq!(app.pending_requests.len(), 1);
    }
}
