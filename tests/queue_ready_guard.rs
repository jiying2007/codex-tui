//! A known failed/pending observation cannot authorize list-dependent writes.
use codex_tui::{
    app::{Action, AppState, Effect, View, reduce},
    backend::{CodexBackend, FakeBackend},
    thread_queue::{ThreadQueueMutation, parse_queue_list},
};
use serde_json::json;
fn fixture() -> AppState {
    let mut app = AppState::new(FakeBackend::scaled(2).snapshot().threads);
    let id = app.threads[0].id.clone();
    app.view = View::Thread(id.clone());
    reduce(&mut app, Action::OpenThreadQueue);
    let page=parse_queue_list(id,json!({"data":[
        {"id":"a","clientUserMessageId":"ca","input":[{"type":"text","text":"first","textElements":[]}]},
        {"id":"b","clientUserMessageId":"cb","input":[{"type":"text","text":"second","textElements":[]}]}
    ],"nextCursor":null})).unwrap();
    reduce(&mut app, Action::ThreadQueueLoaded(page));
    app
}
fn refresh_failed(app: &mut AppState) {
    reduce(
        app,
        Action::ThreadQueueFailed {
            thread_id: app.threads[0].id.clone(),
            error: "read failed".into(),
        },
    );
}
#[test]
fn failed_refresh_blocks_stale_edit_without_losing_draft() {
    let mut app = fixture();
    reduce(&mut app, Action::BeginThreadQueueEdit);
    app.input_buffer = "draft".into();
    refresh_failed(&mut app);
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(app.input_buffer, "draft");
}
#[test]
fn pending_refresh_blocks_stale_edit_without_losing_draft() {
    let mut app = fixture();
    reduce(&mut app, Action::BeginThreadQueueEdit);
    app.input_buffer = "draft".into();
    reduce(&mut app, Action::RefreshThreadQueue);
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(app.input_buffer, "draft");
}
#[test]
fn failed_refresh_blocks_reorder_from_cached_list() {
    let mut app = fixture();
    refresh_failed(&mut app);
    assert!(reduce(&mut app, Action::ReorderThreadQueue(1)).is_empty());
}
#[test]
fn mismatched_thread_blocks_reorder_from_cached_list() {
    let mut app = fixture();
    app.view = View::Thread(app.threads[1].id.clone());
    assert!(reduce(&mut app, Action::ReorderThreadQueue(1)).is_empty());
}
#[test]
fn ordinary_fresh_edits_and_reorders_still_work() {
    let mut app = fixture();
    reduce(&mut app, Action::BeginThreadQueueEdit);
    app.input_buffer = "draft".into();
    assert!(matches!(
        reduce(&mut app, Action::CommitInput).as_slice(),
        [Effect::MutateThreadQueue(
            ThreadQueueMutation::Update { .. }
        )]
    ));
    let mut app = fixture();
    assert!(matches!(
        reduce(&mut app, Action::ReorderThreadQueue(1)).as_slice(),
        [Effect::MutateThreadQueue(
            ThreadQueueMutation::Reorder { .. }
        )]
    ));
}
#[test]
fn adding_to_live_thread_does_not_require_a_loaded_list() {
    let mut app = fixture();
    app.thread_queue_snapshot = None;
    reduce(&mut app, Action::BeginThreadQueueAdd);
    app.input_buffer = "independent add".into();
    assert!(matches!(
        reduce(&mut app, Action::CommitInput).as_slice(),
        [Effect::MutateThreadQueue(ThreadQueueMutation::Add { .. })]
    ));
}
