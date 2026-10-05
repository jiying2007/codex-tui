//! Exercise the production reducer under queue refresh while an editor is open.
use codex_tui::{
    app::{Action, AppState, Effect, InputMode, View, reduce},
    backend::{CodexBackend, FakeBackend},
    domain::ThreadId,
    thread_queue::{ThreadQueueMutation, ThreadQueueSnapshot, parse_queue_list},
};
use serde_json::json;

fn snapshot(id: ThreadId, items: &[(&str, &str)]) -> ThreadQueueSnapshot {
    parse_queue_list(
        id,
        json!({"data": items.iter().map(|(id, text)| json!({
        "id": id, "clientUserMessageId": format!("client-{id}"),
        "input": [{"type":"text", "text":text, "textElements":[]}]
    })).collect::<Vec<_>>(), "nextCursor":null}),
    )
    .unwrap()
}

fn fixture() -> (AppState, ThreadId) {
    let mut app = AppState::new(FakeBackend::scaled(2).snapshot().threads);
    let id = app.threads[0].id.clone();
    app.view = View::Thread(id.clone());
    reduce(&mut app, Action::OpenThreadQueue);
    load(&mut app, &id, &[("a", "first"), ("b", "second")]);
    app.thread_queue_selected = 1;
    (app, id)
}
fn load(app: &mut AppState, id: &ThreadId, items: &[(&str, &str)]) {
    reduce(app, Action::ThreadQueueLoaded(snapshot(id.clone(), items)));
}
fn edit(app: &mut AppState) {
    assert!(reduce(app, Action::BeginThreadQueueEdit).is_empty());
    assert_eq!(app.input_mode, InputMode::ThreadQueueEdit);
    app.input_buffer = "operator draft 中文".into();
}
fn assert_refused(app: &mut AppState) {
    let state = app.to_local_state();
    let mode = app.input_mode;
    let draft = app.input_buffer.clone();
    assert!(
        reduce(app, Action::CommitInput).is_empty(),
        "must not mutate a stale target"
    );
    assert_eq!(app.input_mode, mode);
    assert_eq!(app.input_buffer, draft);
    assert_eq!(app.to_local_state(), state);
    assert!(app.thread_queue_error.is_some());
    assert!(app.pending_queue_confirmation.is_none());
}

#[test]
fn refresh_preserves_the_selected_submission_identity() {
    let (mut app, id) = fixture();
    load(&mut app, &id, &[("b", "second"), ("a", "first")]);
    assert_eq!(app.selected_thread_queue_submission().unwrap().id, "b");
}
#[test]
fn edit_keeps_its_original_item_even_after_selection_moves() {
    let (mut app, id) = fixture();
    edit(&mut app);
    reduce(&mut app, Action::MoveThreadQueue(1));
    assert_eq!(
        reduce(&mut app, Action::CommitInput),
        vec![Effect::MutateThreadQueue(ThreadQueueMutation::Update {
            thread_id: id,
            queued_submission_id: "b".into(),
            text: "operator draft 中文".into()
        })]
    );
}
#[test]
fn disappeared_item_retains_draft_without_updating_a_neighbor() {
    let (mut app, id) = fixture();
    edit(&mut app);
    load(&mut app, &id, &[("a", "first")]);
    assert_refused(&mut app);
}
#[test]
fn externally_changed_item_cannot_be_overwritten_by_the_old_editor() {
    let (mut app, id) = fixture();
    edit(&mut app);
    load(&mut app, &id, &[("a", "first"), ("b", "external edit")]);
    assert_refused(&mut app);
}
#[test]
fn multimodal_replacement_cannot_be_destroyed_by_text_editing() {
    let (mut app, id) = fixture();
    edit(&mut app);
    let mut fresh = snapshot(id, &[("a", "first"), ("b", "second")]);
    fresh.submissions[1]
        .input
        .push(json!({"type":"localImage", "path":"/synthetic.png"}));
    fresh.submissions[1].editable_text = None;
    reduce(&mut app, Action::ThreadQueueLoaded(fresh));
    assert_refused(&mut app);
}
#[test]
fn add_editor_cannot_be_retargeted_to_another_thread() {
    let (mut app, _) = fixture();
    reduce(&mut app, Action::BeginThreadQueueAdd);
    app.input_buffer = "operator draft 中文".into();
    app.view = View::Thread(app.threads[1].id.clone());
    assert_refused(&mut app);
}
#[test]
fn deleted_thread_snapshot_is_not_reintroduced() {
    let (mut app, id) = fixture();
    let fresh = snapshot(id.clone(), &[("ghost", "ghost")]);
    let mut threads = app.threads.clone();
    threads.retain(|thread| thread.id != id);
    reduce(&mut app, Action::ReplaceThreads(threads));
    // Preserve an in-flight screen identity to exercise the liveness guard itself.
    app.view = View::Thread(id);
    app.thread_queue_open = true;
    let before = app.thread_queue_snapshot.clone();
    reduce(&mut app, Action::ThreadQueueLoaded(fresh));
    assert_eq!(app.thread_queue_snapshot, before);
}
#[test]
fn reorder_waits_for_authoritative_order_without_retargeting_selection() {
    let (mut app, id) = fixture();
    let before = app.thread_queue_snapshot.clone();
    assert!(matches!(
        reduce(&mut app, Action::ReorderThreadQueue(-1)).as_slice(),
        [Effect::MutateThreadQueue(
            ThreadQueueMutation::Reorder { .. }
        )]
    ));
    assert_eq!(app.thread_queue_snapshot, before);
    assert_eq!(app.selected_thread_queue_submission().unwrap().id, "b");
    load(&mut app, &id, &[("b", "second"), ("a", "first")]);
    assert_eq!(app.selected_thread_queue_submission().unwrap().id, "b");
    assert_eq!(app.thread_queue_selected, 0);
}
#[test]
fn unchanged_item_commits_once_and_clears_only_the_editor() {
    let (mut app, id) = fixture();
    let local = app.to_local_state();
    edit(&mut app);
    load(&mut app, &id, &[("a", "first"), ("b", "second")]);
    assert_eq!(app.to_local_state(), local);
    assert!(matches!(reduce(&mut app, Action::CommitInput).as_slice(),
        [Effect::MutateThreadQueue(ThreadQueueMutation::Update { queued_submission_id, .. })] if queued_submission_id == "b"));
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.input_buffer.is_empty());
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
}
#[test]
fn unchanged_add_keeps_upstream_authority() {
    let (mut app, id) = fixture();
    let before = app.thread_queue_snapshot.clone();
    reduce(&mut app, Action::BeginThreadQueueAdd);
    app.input_buffer = "new item".into();
    assert!(matches!(reduce(&mut app, Action::CommitInput).as_slice(),
        [Effect::MutateThreadQueue(ThreadQueueMutation::Add { thread_id, text, .. })] if *thread_id == id && text == "new item"));
    assert_eq!(app.thread_queue_snapshot, before);
}
#[test]
fn cancel_and_close_do_not_leak_editor_targets() {
    let (mut app, id) = fixture();
    edit(&mut app);
    reduce(&mut app, Action::CancelInput);
    assert_eq!(app.input_mode, InputMode::Normal);
    reduce(&mut app, Action::BeginThreadQueueAdd);
    app.input_buffer = "new".into();
    assert!(matches!(
        reduce(&mut app, Action::CommitInput).as_slice(),
        [Effect::MutateThreadQueue(ThreadQueueMutation::Add { .. })]
    ));
    load(&mut app, &id, &[("a", "first"), ("b", "second")]);
    edit(&mut app);
    assert_eq!(
        reduce(&mut app, Action::CloseThreadQueue),
        vec![Effect::StopWatchingThreadQueue(id)]
    );
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.input_buffer.is_empty());
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
}
