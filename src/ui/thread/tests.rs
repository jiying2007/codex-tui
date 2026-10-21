use super::*;
use crate::app::{Action, Effect, View, reduce};
use crate::conversation::{
    ConversationItem, ConversationItemKind, ConversationPage, ConversationState,
};
use crate::domain::{ThreadId, ThreadUiState};
use ratatui::{Terminal, backend::TestBackend};

fn page(text: &str) -> ConversationPage {
    ConversationPage {
        thread_id: ThreadId::new("viewport-thread"),
        title: None,
        turns: vec![],
        items: vec![ConversationItem {
            turn_id: "turn".into(),
            item_id: "item".into(),
            kind: ConversationItemKind::Assistant,
            text: text.into(),
            status: None,
        }],
        next_turn_cursor: None,
        next_item_cursor: None,
    }
}

fn app(text: &str) -> AppState {
    let mut app = AppState::new(vec![]);
    app.view = View::Thread(ThreadId::new("viewport-thread"));
    let mut conversation = ConversationState::loading(ThreadId::new("viewport-thread"));
    conversation.replace_page(page(text));
    app.conversations
        .insert("viewport-thread".into(), conversation);
    app.thread_ui
        .insert("viewport-thread".into(), ThreadUiState::default());
    app
}

fn screen(app: &AppState) -> String {
    sized_screen(app, 24)
}

fn sized_screen(app: &AppState, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(70, height)).unwrap();
    terminal
        .draw(|frame| render_thread(frame, app, "viewport-thread", frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..70).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn rendered_long_message_can_scroll_to_both_ends_and_resume_following() {
    let text = (0..200)
        .map(|n| format!("VISIBLE-{n:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut app = app(&text);
    assert!(screen(&app).contains("VISIBLE-199"));
    assert!(!screen(&app).contains("VISIBLE-000"));
    reduce(&mut app, Action::ConversationStart);
    assert!(screen(&app).contains("VISIBLE-000"));
    for _ in 0..40 {
        reduce(&mut app, Action::ScrollBy(5));
    }
    assert!(screen(&app).contains("VISIBLE-199"));
    for _ in 0..40 {
        reduce(&mut app, Action::ScrollBy(-5));
    }
    assert!(screen(&app).contains("VISIBLE-000"));
    reduce(&mut app, Action::FollowNewest);
    assert!(screen(&app).contains("VISIBLE-199"));
    reduce(
        &mut app,
        Action::ConversationLoaded(page(&format!("{text}\nNEWEST-MARKER"))),
    );
    assert!(screen(&app).contains("NEWEST-MARKER"));
    reduce(&mut app, Action::ConversationStart);
    let before = screen(&app);
    reduce(
        &mut app,
        Action::ConversationLoaded(page(&format!("{text}\nNEWEST-MARKER\nANOTHER-MARKER"))),
    );
    assert_eq!(screen(&app), before);
}

#[test]
fn older_page_is_requested_only_at_the_first_visual_row() {
    let mut history = page(
        &(0..200)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    history.next_turn_cursor = Some("older".into());
    let mut app = app("");
    reduce(&mut app, Action::ConversationLoaded(history));
    screen(&app);
    let effects = reduce(&mut app, Action::ScrollBy(-5));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadOlderConversation { .. }))
    );
    reduce(&mut app, Action::ConversationStart);
    let effects = reduce(&mut app, Action::ScrollBy(-5));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadOlderConversation { .. }))
    );
    assert!(!app.thread_ui["viewport-thread"].follow);
    let mut older = page("OLDER-PAGE-MARKER");
    older.items[0].item_id = "older-item".into();
    reduce(
        &mut app,
        Action::OlderConversationLoaded {
            page: older,
            turn_cursor: Some("older".into()),
            item_cursor: None,
        },
    );
    assert!(screen(&app).contains("line 0"));
    reduce(&mut app, Action::ScrollBy(-5));
    assert!(screen(&app).contains("OLDER-PAGE-MARKER"));
}

#[test]
fn exact_search_hit_is_visible_after_a_long_preceding_item() {
    let mut app = app("");
    let mut history = page(&"long preceding paragraph\n".repeat(200));
    let mut hit = history.items[0].clone();
    hit.item_id = "hit".into();
    hit.text = "EXACT-SEARCH-HIT".into();
    history.items.push(hit);
    reduce(
        &mut app,
        Action::TranscriptSearchLoadedPage {
            page: history,
            item_id: "hit".into(),
        },
    );
    assert!(screen(&app).contains("EXACT-SEARCH-HIT"));
}

#[test]
fn renderer_retains_code_indentation_and_blank_paragraphs() {
    let app = app("paragraph\n\n    let answer = 42;\nnext line");
    let output = screen(&app);
    let rows = output.lines().collect::<Vec<_>>();
    let paragraph = rows
        .iter()
        .position(|line| line.contains("paragraph"))
        .unwrap();
    let code = rows
        .iter()
        .position(|line| line.contains("let answer = 42;"))
        .unwrap();
    assert_eq!(code, paragraph + 2);
    assert!(rows[code].contains("│    let answer"));
}

#[test]
fn small_body_page_keys_do_not_skip_any_content() {
    for height in [13, 14] {
        let text = (0..30)
            .map(|n| format!("SMALL-{n:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut app = app(&text);
        reduce(&mut app, Action::ConversationStart);
        let mut seen = String::new();
        for _ in 0..32 {
            seen.push_str(&sized_screen(&app, height));
            reduce(&mut app, Action::ScrollBy(5));
        }
        for n in 0..30 {
            assert!(
                seen.contains(&format!("SMALL-{n:02}")),
                "height {height} skipped {n}"
            );
        }
    }
}

#[test]
fn pending_approval_keeps_history_navigation_and_follow_available() {
    use crate::conversation::{
        ApprovalContext, InteractiveRequest, InteractiveRequestKind, RpcRequestId,
    };
    let text = (0..40)
        .map(|n| format!("PENDING-HISTORY-{n:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut app = app(&text);
    app.pending_requests.push(InteractiveRequest {
        request_id: RpcRequestId::Integer(3),
        thread_id: ThreadId::new("viewport-thread"),
        turn_id: "turn".into(),
        item_id: "approval".into(),
        kind: InteractiveRequestKind::CommandApproval {
            context: ApprovalContext::from(serde_json::json!({})),
            command: "cargo check".into(),
            cwd: "/repo".into(),
            reason: None,
        },
    });
    let initial = screen(&app);
    assert!(initial.contains("NEEDS YOU"));
    assert!(initial.contains("PENDING-HISTORY-39"));
    reduce(&mut app, Action::ConversationStart);
    assert!(screen(&app).contains("PENDING-HISTORY-00"));
    let mut seen = String::new();
    for _ in 0..42 {
        let output = sized_screen(&app, 14);
        assert!(output.contains("NEEDS YOU"));
        seen.push_str(&output);
        reduce(&mut app, Action::ScrollBy(5));
    }
    for n in 0..40 {
        assert!(seen.contains(&format!("PENDING-HISTORY-{n:02}")));
    }
    reduce(&mut app, Action::FollowNewest);
    assert!(screen(&app).contains("PENDING-HISTORY-39"));
}

#[test]
fn paged_old_body_stays_visible_when_recent_refresh_drops_its_anchor_until_end() {
    let mut app = app("RECENT-BODY");
    let mut recent = page("RECENT-BODY");
    recent.next_item_cursor = Some("older".into());
    reduce(&mut app, Action::ConversationLoaded(recent));
    screen(&app);
    reduce(&mut app, Action::ConversationStart);
    reduce(&mut app, Action::ScrollBy(-5));
    let mut older = page(
        &(0..40)
            .map(|n| format!("OLD-BODY-{n:02}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    older.items[0].item_id = "old-item".into();
    older.next_item_cursor = Some("ancient".into());
    reduce(
        &mut app,
        Action::OlderConversationLoaded {
            page: older,
            turn_cursor: None,
            item_cursor: Some("older".into()),
        },
    );
    reduce(&mut app, Action::ConversationStart);
    reduce(&mut app, Action::ScrollBy(5));
    let before = screen(&app)
        .lines()
        .filter(|line| line.contains("OLD-BODY-"))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    assert!(!before.is_empty());
    let mut latest = page("NEWEST-BODY");
    latest.items[0].item_id = "new-item".into();
    latest.next_item_cursor = Some("recent-gap".into());
    reduce(&mut app, Action::ConversationLoaded(latest));
    assert!(app.is_reading_snapshot("viewport-thread"));
    let paused = screen(&app);
    assert_eq!(
        paused
            .lines()
            .filter(|line| line.contains("OLD-BODY-"))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>(),
        before
    );
    assert!(!paused.contains("NEWEST-BODY"));
    reduce(&mut app, Action::FollowNewest);
    assert!(screen(&app).contains("NEWEST-BODY"));
    assert!(!app.is_reading_snapshot("viewport-thread"));
}
