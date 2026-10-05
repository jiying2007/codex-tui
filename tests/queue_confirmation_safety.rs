//! Exercise confirmations through the production reducer, not a parallel model.
use codex_tui::{
    app::{Action, AppState, Effect, View, reduce},
    backend::{CodexBackend, FakeBackend},
    domain::ThreadId,
    thread_queue::{ThreadQueueMutation, ThreadQueueSnapshot, parse_queue_list},
};
use serde_json::json;

fn snapshot(id: ThreadId) -> ThreadQueueSnapshot {
    parse_queue_list(id, json!({"data":[
        {"id":"queue-alpha","clientUserMessageId":"client-a","input":[{"type":"text","text":"original task","textElements":[]}]},
        {"id":"queue-beta","clientUserMessageId":"client-b","input":[{"type":"text","text":"other task","textElements":[]}]}
    ],"nextCursor":null})).unwrap()
}
fn fixture() -> (AppState, ThreadId) {
    let mut app = AppState::new(FakeBackend::scaled(2).snapshot().threads);
    let id = app.threads[0].id.clone();
    app.view = View::Thread(id.clone());
    reduce(&mut app, Action::OpenThreadQueue);
    reduce(&mut app, Action::ThreadQueueLoaded(snapshot(id.clone())));
    (app, id)
}
fn for_each_confirmation(change: impl Fn(&mut AppState, &ThreadId)) {
    for begin in [
        Action::BeginThreadQueueStart,
        Action::BeginThreadQueueDelete,
    ] {
        let (mut app, id) = fixture();
        assert!(reduce(&mut app, begin).is_empty());
        change(&mut app, &id);
        let before = app.to_local_state();
        assert!(
            reduce(&mut app, Action::ConfirmPendingOperation).is_empty(),
            "stale confirmation emitted an upstream mutation"
        );
        assert_eq!(app.to_local_state(), before);
        assert!(app.thread_queue_error.is_some() || app.mutation_notice.is_some());
    }
}
#[test]
fn changed_content_requires_new_confirmation() {
    for_each_confirmation(|app, id| {
        let mut fresh = snapshot(id.clone());
        fresh.submissions[0].input[0]["text"] = json!("externally replaced task");
        fresh.submissions[0].summary = "externally replaced task".into();
        fresh.submissions[0].editable_text = Some("externally replaced task".into());
        reduce(app, Action::ThreadQueueLoaded(fresh));
    });
}
#[test]
fn removed_item_cannot_be_started_or_deleted_from_old_confirmation() {
    for_each_confirmation(|app, id| {
        let mut fresh = snapshot(id.clone());
        fresh.submissions.remove(0);
        reduce(app, Action::ThreadQueueLoaded(fresh));
    });
}
#[test]
fn replacement_client_identity_requires_new_confirmation() {
    for_each_confirmation(|app, id| {
        let mut fresh = snapshot(id.clone());
        fresh.submissions[0].client_user_message_id = "replacement-client".into();
        reduce(app, Action::ThreadQueueLoaded(fresh));
    });
}
#[test]
fn structured_payload_change_requires_new_confirmation() {
    for_each_confirmation(|app, id| {
        let mut fresh = snapshot(id.clone());
        fresh.submissions[0].input[0]["textElements"] =
            json!([{"byteRange":{"start":0,"end":8},"placeholder":"linked"}]);
        reduce(app, Action::ThreadQueueLoaded(fresh));
    });
}
#[test]
fn switched_thread_cannot_use_an_old_confirmation() {
    for_each_confirmation(|app, _| {
        app.view = View::Thread(app.threads[1].id.clone());
    });
}
#[test]
fn removed_thread_cannot_use_an_old_confirmation() {
    for_each_confirmation(|app, id| {
        let mut threads = app.threads.clone();
        threads.retain(|thread| thread.id != *id);
        reduce(app, Action::ReplaceThreads(threads));
        app.view = View::Thread(id.clone());
        app.thread_queue_open = true;
    });
}
#[test]
fn failed_refresh_does_not_authorize_using_old_snapshot() {
    for_each_confirmation(|app, id| {
        reduce(
            app,
            Action::ThreadQueueFailed {
                thread_id: id.clone(),
                error: "injected refresh failure".into(),
            },
        );
    });
}
#[test]
fn in_flight_refresh_requires_completion_before_confirm() {
    for_each_confirmation(|app, _| {
        reduce(app, Action::RefreshThreadQueue);
    });
}
#[test]
fn begin_refuses_wrong_thread_snapshot() {
    for begin in [
        Action::BeginThreadQueueStart,
        Action::BeginThreadQueueDelete,
    ] {
        let (mut app, _) = fixture();
        app.thread_queue_snapshot = Some(snapshot(app.threads[1].id.clone()));
        reduce(&mut app, begin);
        assert!(reduce(&mut app, Action::ConfirmPendingOperation).is_empty());
    }
}
#[test]
fn unchanged_original_item_survives_reordering_and_navigation() {
    for begin in [
        Action::BeginThreadQueueStart,
        Action::BeginThreadQueueDelete,
    ] {
        let (mut app, id) = fixture();
        reduce(&mut app, begin);
        let mut fresh = snapshot(id.clone());
        fresh.submissions.swap(0, 1);
        reduce(&mut app, Action::ThreadQueueLoaded(fresh));
        reduce(&mut app, Action::MoveThreadQueue(1));
        let effects = reduce(&mut app, Action::ConfirmPendingOperation);
        assert!(
            matches!(effects.as_slice(), [Effect::MutateThreadQueue(ThreadQueueMutation::Start {thread_id, queued_submission_id} | ThreadQueueMutation::Delete {thread_id, queued_submission_id})] if *thread_id==id && queued_submission_id=="queue-alpha")
        );
        assert!(reduce(&mut app, Action::ConfirmPendingOperation).is_empty());
    }
}
#[test]
fn cancel_or_close_never_sends_confirmation() {
    for cancel in [Action::CancelPendingOperation, Action::CloseThreadQueue] {
        let (mut app, _) = fixture();
        reduce(&mut app, Action::BeginThreadQueueStart);
        reduce(&mut app, cancel);
        assert!(reduce(&mut app, Action::ConfirmPendingOperation).is_empty());
    }
}
#[test]
fn confirmation_ui_identifies_original_item_after_navigation() {
    use ratatui::{Terminal, backend::TestBackend};
    let (mut app, _) = fixture();
    reduce(&mut app, Action::BeginThreadQueueStart);
    reduce(&mut app, Action::MoveThreadQueue(1));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| codex_tui::ui::render(frame, &app))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(
        text.contains("queue-alpha"),
        "confirmation does not identify original queue item"
    );
}
