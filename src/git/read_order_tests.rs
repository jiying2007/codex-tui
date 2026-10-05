//! Test the actual handle's admission keys and result consumption without Git I/O.
use super::*;
use crate::latest_read::{ReadEnvelope, ReadFence};
fn fixture(
    capacity: usize,
) -> (
    GitHandle,
    mpsc::Receiver<ReadEnvelope<GitCommand>>,
    mpsc::Sender<ReadEnvelope<GitEvent>>,
) {
    let (command_tx, command_rx) = mpsc::channel(capacity);
    let (event_tx, event_rx) = mpsc::channel(8);
    (
        GitHandle {
            command_tx,
            event_rx,
            task: tokio::spawn(async {}),
            reads: ReadFence::default(),
        },
        command_rx,
        event_tx,
    )
}
fn reply(request: ReadEnvelope<GitCommand>, label: &str) -> ReadEnvelope<GitEvent> {
    let event = match request.value {
        GitCommand::Probe { thread_id, cwd } => {
            GitEvent::Context(GitContext::failed(thread_id, cwd, label.into()))
        }
        GitCommand::LoadReview { thread_id, cwd } => {
            GitEvent::Review(GitReview::failed(thread_id, cwd, label.into()))
        }
    };
    ReadEnvelope {
        value: event,
        ticket: request.ticket,
    }
}
#[tokio::test]
async fn shared_cwd_probes_are_fenced_across_leader_changes() {
    let (mut handle, mut commands, events) = fixture(4);
    handle.probe(ThreadId::new("a"), "/repo".into()).unwrap();
    let old = commands.try_recv().unwrap();
    handle.probe(ThreadId::new("b"), "/repo".into()).unwrap();
    events
        .try_send(reply(commands.try_recv().unwrap(), "fresh"))
        .unwrap();
    assert!(
        matches!(handle.try_recv(), Some(GitEvent::Context(context)) if context.thread_id == ThreadId::new("b"))
    );
    events.try_send(reply(old, "obsolete")).unwrap();
    assert!(handle.try_recv().is_none());
}
#[tokio::test]
async fn review_a_b_a_navigation_keeps_only_latest_intent_and_independent_probe() {
    let (mut handle, mut commands, events) = fixture(8);
    let id = ThreadId::new("thread");
    for cwd in ["/a", "/b", "/a"] {
        handle.load_review(id.clone(), cwd.into()).unwrap();
    }
    handle.probe(id.clone(), "/a".into()).unwrap();
    while let Ok(request) = commands.try_recv() {
        events.try_send(reply(request, "result")).unwrap();
    }
    assert!(matches!(handle.try_recv(), Some(GitEvent::Review(review)) if review.cwd == "/a"));
    assert!(matches!(handle.try_recv(), Some(GitEvent::Context(_))));
    assert!(handle.try_recv().is_none());
}
#[tokio::test]
async fn rejected_review_admission_does_not_invalidate_the_existing_request() {
    let (mut handle, mut commands, events) = fixture(1);
    handle
        .load_review(ThreadId::new("thread"), "/repo".into())
        .unwrap();
    assert!(
        handle
            .load_review(ThreadId::new("thread"), "/repo".into())
            .is_err()
    );
    events
        .try_send(reply(commands.try_recv().unwrap(), "accepted"))
        .unwrap();
    assert!(
        matches!(handle.try_recv(), Some(GitEvent::Review(review)) if review.error.as_deref() == Some("accepted"))
    );
}
