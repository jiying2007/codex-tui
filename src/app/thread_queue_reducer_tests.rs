use super::*;
use crate::backend::{CodexBackend, FakeBackend};
use serde_json::json;

fn queue_app() -> (AppState, ThreadId) {
    let threads = FakeBackend::seeded().snapshot().threads;
    let thread_id = threads[0].id.clone();
    let mut app = AppState::new(threads);
    app.view = View::Thread(thread_id.clone());
    (app, thread_id)
}

fn snapshot(thread_id: ThreadId) -> ThreadQueueSnapshot {
    ThreadQueueSnapshot {
        thread_id,
        submissions: vec![
            QueuedSubmission {
                id: "q1".into(),
                client_user_message_id: "c1".into(),
                input: vec![json!({
                    "type": "text",
                    "text": "first",
                    "textElements": []
                })],
                summary: "first".into(),
                editable_text: Some("first".into()),
            },
            QueuedSubmission {
                id: "q2".into(),
                client_user_message_id: "c2".into(),
                input: vec![json!({"type": "localImage", "path": "/tmp/a.png"})],
                summary: "[localImage]".into(),
                editable_text: None,
            },
        ],
        next_cursor: None,
    }
}

#[test]
fn queue_open_load_edit_and_close_stay_upstream_authoritative() {
    let (mut app, thread_id) = queue_app();
    assert_eq!(
        reduce(&mut app, Action::OpenThreadQueue),
        vec![Effect::RefreshThreadQueue(thread_id.clone())]
    );
    assert!(app.thread_queue_open);
    assert!(app.thread_queue_loading);

    reduce(
        &mut app,
        Action::ThreadQueueLoaded(snapshot(thread_id.clone())),
    );
    assert!(!app.thread_queue_loading);
    assert_eq!(app.selected_thread_queue_submission().unwrap().id, "q1");

    reduce(&mut app, Action::BeginThreadQueueEdit);
    assert_eq!(app.input_mode, InputMode::ThreadQueueEdit);
    assert_eq!(app.input_buffer, "first");
    app.input_buffer = "updated".into();
    let effects = reduce(&mut app, Action::CommitInput);
    assert!(matches!(
        effects.as_slice(),
        [Effect::MutateThreadQueue(ThreadQueueMutation::Update {
            queued_submission_id,
            text,
            ..
        })] if queued_submission_id == "q1" && text == "updated"
    ));

    app.input_mode = InputMode::Normal;
    app.thread_queue_selected = 1;
    reduce(&mut app, Action::BeginThreadQueueEdit);
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.mutation_notice.is_some());

    let effects = reduce(&mut app, Action::CloseThreadQueue);
    assert_eq!(effects, vec![Effect::StopWatchingThreadQueue(thread_id)]);
    assert!(!app.thread_queue_open);
}

#[test]
fn queue_reorder_and_destructive_actions_follow_confirmation_contract() {
    let (mut app, thread_id) = queue_app();
    app.thread_queue_open = true;
    app.thread_queue_snapshot = Some(snapshot(thread_id.clone()));

    let effects = reduce(&mut app, Action::ReorderThreadQueue(1));
    assert_eq!(app.thread_queue_selected, 0);
    assert_eq!(app.selected_thread_queue_submission().unwrap().id, "q1");
    assert!(matches!(
        effects.as_slice(),
        [Effect::MutateThreadQueue(ThreadQueueMutation::Reorder {
            queued_submission_ids,
            ..
        })] if queued_submission_ids == &vec!["q2".to_string(), "q1".to_string()]
    ));

    reduce(
        &mut app,
        Action::ThreadQueueLoaded(snapshot(thread_id.clone())),
    );
    app.thread_queue_selected = 0;
    reduce(&mut app, Action::BeginThreadQueueStart);
    assert_eq!(
        app.pending_queue_confirmation.as_ref().unwrap().label(),
        "start"
    );
    let effects = reduce(&mut app, Action::ConfirmPendingOperation);
    assert!(matches!(
        effects.as_slice(),
        [Effect::MutateThreadQueue(ThreadQueueMutation::Start { .. })]
    ));

    reduce(&mut app, Action::ThreadQueueLoaded(snapshot(thread_id)));
    reduce(&mut app, Action::BeginThreadQueueDelete);
    assert_eq!(
        app.pending_queue_confirmation.as_ref().unwrap().label(),
        "delete"
    );
    reduce(&mut app, Action::CancelPendingOperation);
    assert!(app.pending_queue_confirmation.is_none());
}
