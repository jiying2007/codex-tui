use super::*;
use crate::backend::{CodexBackend, FakeBackend};
use crate::user_response::UserResponseSubmission;
use std::cell::Cell;
fn fixture() -> (
    BTreeMap<RpcRequestId, PendingServerRequest>,
    BTreeMap<String, ThreadSummary>,
    UserResponseSubmission,
) {
    let threads = by_id(FakeBackend::seeded().snapshot().threads);
    let thread = threads.keys().next().unwrap();
    let params = json!({"threadId": thread, "turnId": "turn", "itemId": "item", "questions": [{"id":"q", "question":"Answer?", "isSecret":true}]});
    let wire = json!({"id":"form", "method":"item/tool/requestUserInput", "params":params});
    let request = parse_interactive_request(&wire).unwrap().unwrap();
    let pending = BTreeMap::from([(
        request.request_id.clone(),
        PendingServerRequest {
            method: "item/tool/requestUserInput".into(),
            params,
        },
    )]);
    let submission = UserResponseSubmission {
        ticket: 1,
        request,
        resolution: InteractiveResolution::UserInput(BTreeMap::from([(
            "q".into(),
            vec!["synthetic-only".into()],
        )])),
    };
    (pending, threads, submission)
}
#[tokio::test]
async fn response_is_written_once_with_original_id_and_answers() {
    let (mut pending, threads, s) = fixture();
    let outcome = send_with(
        &mut pending,
        &threads,
        s.clone(),
        Duration::from_secs(1),
        |m| async move {
            assert_eq!(
                m,
                json!({"id":"form","result":{"answers":{"q":{"answers":["synthetic-only"]}}}})
            );
            Ok(())
        },
    )
    .await;
    assert_eq!(outcome, UserResponseOutcome::Written);
    assert!(pending.is_empty());
    assert!(matches!(
        send_with(
            &mut pending,
            &threads,
            s,
            Duration::from_secs(1),
            |_| async { panic!("second write") }
        )
        .await,
        UserResponseOutcome::NotSent(_)
    ));
}
#[tokio::test]
async fn changed_form_or_removed_thread_is_refused_before_io() {
    for changed_thread in [false, true] {
        let (mut pending, mut threads, s) = fixture();
        if changed_thread {
            threads.clear();
        } else {
            pending.get_mut(&s.request.request_id).unwrap().params["questions"][0]["question"] =
                json!("Different?");
        }
        let outcome = send_with(
            &mut pending,
            &threads,
            s,
            Duration::from_secs(1),
            |_| async { panic!("unexpected write") },
        )
        .await;
        assert!(matches!(outcome, UserResponseOutcome::NotSent(_)));
        assert_eq!(pending.len(), 1);
    }
}
#[tokio::test]
async fn mismatched_answer_keys_are_refused_without_consuming_request() {
    let (mut pending, threads, mut s) = fixture();
    let InteractiveResolution::UserInput(answers) = &mut s.resolution else {
        panic!("input");
    };
    answers.insert("other".into(), vec!["x".into()]);
    assert!(matches!(
        send_with(
            &mut pending,
            &threads,
            s,
            Duration::from_secs(1),
            |_| async { panic!("unexpected write") }
        )
        .await,
        UserResponseOutcome::NotSent(_)
    ));
    assert_eq!(pending.len(), 1);
}
#[tokio::test]
async fn write_failure_is_unknown_and_not_retryable() {
    let (mut pending, threads, s) = fixture();
    let calls = Cell::new(0);
    let outcome = send_with(
        &mut pending,
        &threads,
        s.clone(),
        Duration::from_secs(1),
        |_| {
            calls.set(calls.get() + 1);
            async { anyhow::bail!("injected broken pipe") }
        },
    )
    .await;
    assert!(matches!(outcome, UserResponseOutcome::Unknown(_)));
    assert!(matches!(
        send_with(&mut pending, &threads, s, Duration::from_secs(1), |_| {
            calls.set(calls.get() + 1);
            async { Ok(()) }
        })
        .await,
        UserResponseOutcome::NotSent(_)
    ));
    assert_eq!(calls.get(), 1);
}
#[tokio::test]
async fn stalled_write_is_bounded_and_not_restored_for_retry() {
    let (mut pending, threads, s) = fixture();
    let outcome = tokio::time::timeout(
        Duration::from_secs(1),
        send_with(&mut pending, &threads, s, Duration::from_millis(5), |_| {
            std::future::pending::<Result<()>>()
        }),
    )
    .await
    .unwrap();
    assert!(matches!(outcome, UserResponseOutcome::Unknown(_)));
    assert!(pending.is_empty());
}
#[test]
fn debug_does_not_expose_answers_and_actual_command_queue_is_bounded() {
    let (_, _, s) = fixture();
    assert!(!format!("{s:?}").contains("synthetic-only"));
    let (tx, _rx) = mpsc::channel(1);
    queue_backend_command(&tx, BackendCommand::SubmitUserResponse(s.clone())).unwrap();
    assert!(queue_backend_command(&tx, BackendCommand::SubmitUserResponse(s)).is_err());
}

#[test]
fn queued_replacements_resolutions_and_removed_threads_invalidate_response() {
    let (_, _, s) = fixture();
    for message in [
        json!({"id":"form", "method":"item/tool/requestUserInput", "params":{}}),
        json!({"method":"serverRequest/resolved", "params":{"requestId":"form"}}),
        json!({"method":"thread/deleted", "params":{"threadId":s.request.thread_id.0}}),
    ] {
        assert!(queued_invalidates(&VecDeque::from([message]), &s));
    }
    assert!(!queued_invalidates(
        &VecDeque::from([
            json!({"method":"serverRequest/resolved", "params":{"requestId":"other"}})
        ]),
        &s
    ));
}
