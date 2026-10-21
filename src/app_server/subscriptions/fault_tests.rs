use super::{fixture::*, *};
use futures_util::StreamExt;
use tokio_tungstenite::accept_async;

fn approval() -> Value {
    json!({"id":99,"method":"item/commandExecution/requestApproval","params":{
        "threadId":"one","turnId":"turn-1","itemId":"item-1","command":"fixture","cwd":"/fixture"
    }})
}

async fn wait_with_clock_paused(phase: &str, mut complete: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !complete() {
        assert!(
            std::time::Instant::now() < deadline,
            "fixture stalled: {phase}"
        );
        // Remain runnable while real socket I/O completes so Tokio cannot
        // automatically advance the paused clock to an unrelated deadline.
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn pending_unsubscribe_does_not_authorize_new_work_before_its_ack() {
    let (target, listener) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept_async(listener.accept().await.unwrap().0)
            .await
            .unwrap();
        let acquire = receive(&mut socket).await;
        emit(
            &mut socket,
            json!({"id":acquire["id"],"result":{"thread":{"id":"one"}}}),
        )
        .await;
        let release = receive(&mut socket).await;
        assert_eq!(release["method"], "thread/unsubscribe");
        // An already-sent approval can cross the release request on the wire.
        emit(&mut socket, approval()).await;
        let response = receive(&mut socket).await;
        assert_eq!(response["id"], 99);
        emit(
            &mut socket,
            json!({"id":release["id"],"result":{"status":"unsubscribed"}}),
        )
        .await;
        let next = receive(&mut socket).await;
        assert_eq!(next["method"], "thread/resume");
        emit(
            &mut socket,
            json!({"id":next["id"],"result":{"thread":{"id":"one"}}}),
        )
        .await;
    });
    let mut rpc = RpcSession::connect(&target).await.unwrap();
    let id = ThreadId::new("one");
    rpc.ensure_subscribed(&id).await.unwrap();
    assert!(rpc.change_subscription(&id, false).await.is_err());
    assert!(!rpc.subscriptions.confirmed("one"));
    assert!(
        rpc.ensure_subscribed(&id)
            .await
            .unwrap_err()
            .to_string()
            .contains("acknowledgement is pending")
    );
    let request = rpc.read_message().await.unwrap().unwrap();
    rpc.reject_request(request["id"].clone(), "fixture response")
        .await
        .unwrap();
    let ack = rpc.read_message().await.unwrap().unwrap();
    assert!(rpc.subscription_response(&ack).unwrap().unwrap().is_none());
    rpc.ensure_subscribed(&id).await.unwrap();
    server.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn subscription_ack_deadline_stops_actor_and_does_not_replay_an_accepted_prompt() {
    // Start on the timer's exact initial tick and keep a runnable task outside
    // the socket fixture, preventing automatic clock advances during real I/O.
    let fixture = tokio::spawn(subscription_ack_deadline_fixture());
    wait_with_clock_paused("subscription timeout fixture", || fixture.is_finished()).await;
    fixture.await.unwrap();
}

async fn subscription_ack_deadline_fixture() {
    let (target, listener) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept_async(listener.accept().await.unwrap().0)
            .await
            .unwrap();
        let existing = receive(&mut socket).await;
        assert_eq!(existing["params"]["threadId"], "existing");
        emit(
            &mut socket,
            json!({"id":existing["id"],"result":{"thread":{"id":"existing"}}}),
        )
        .await;
        let request = receive(&mut socket).await;
        assert_eq!(request["method"], "thread/resume");
        let mut pending_approval = approval();
        pending_approval["params"]["threadId"] = json!("existing");
        emit(&mut socket, pending_approval).await;
        let mut requests = vec![existing, request];
        // Never acknowledge the resume. Only registry reads may follow; neither
        // the subscription request nor the accepted prompt may be replayed.
        while let Some(Ok(frame)) = socket.next().await {
            if !frame.is_text() {
                break;
            }
            let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            assert_eq!(request["method"], "thread/loaded/list");
            emit(
                &mut socket,
                json!({"id":request["id"],"result":{"data":["one"],"nextCursor":null}}),
            )
            .await;
            requests.push(request);
        }
        requests
    });
    let mut rpc = RpcSession::connect(&target).await.unwrap();
    rpc.ensure_subscribed(&ThreadId::new("existing"))
        .await
        .unwrap();
    let id = ThreadId::new("one");
    assert!(rpc.ensure_subscribed(&id).await.is_err());
    // Keep the production deadline, with the clock stopped until the actor has
    // handled the accepted prompt and completed its initial registry read.
    let ack_deadline = rpc.subscriptions.deadline().unwrap();
    assert!(ack_deadline > Instant::now());
    let watches = WatchSet::default();
    watches.admit("one", || Ok(())).unwrap();
    let queue = WatchSet::default();
    let mut status = BackendStatus::starting("subscription-timeout");
    status.connected = true;
    let (snapshots, mut snapshot_rx) = watch::channel(BackendSnapshot {
        generation: 0,
        threads: vec![],
        status: status.clone(),
    });
    let (events, mut event_rx) = mpsc::channel(8);
    let (commands, command_rx) = mpsc::channel(8);
    commands
        .try_send(BackendCommand::SubmitPrompt {
            thread_id: id,
            request_id: 1,
            text: "accepted fixture prompt".into(),
            active_turn_id: None,
        })
        .unwrap();
    let actor = tokio::spawn(run_registry_actor(
        rpc,
        vec![],
        status,
        snapshots,
        events,
        command_rx,
        (None, target, watches.clone(), queue.clone()),
    ));
    let mut failures = 0;
    wait_with_clock_paused("accepted prompt and registry reply", || {
        while let Ok(event) = event_rx.try_recv() {
            if let ConversationEvent::PromptFailed { error, .. } = event {
                assert!(error.contains("subscription acknowledgement is pending"));
                failures += 1;
            }
        }
        failures > 0
            && snapshot_rx
                .borrow()
                .status
                .capabilities
                .iter()
                .any(|value| value == "thread/loaded/list")
    })
    .await;
    assert_eq!(failures, 1);
    assert!(!actor.is_finished());
    // Cross Tokio's millisecond timer boundary without changing the deadline.
    tokio::time::advance(ack_deadline.duration_since(Instant::now()) + Duration::from_millis(1))
        .await;
    wait_with_clock_paused("subscription deadline quarantine", || actor.is_finished()).await;
    actor.await.unwrap();
    assert!(watches.snapshot().is_empty());
    assert!(queue.snapshot().is_empty());
    assert!(
        watches
            .admit("new", || panic!("closed actor admission"))
            .is_err()
    );
    let last = snapshot_rx.borrow_and_update().clone();
    assert!(!last.status.connected);
    assert!(
        last.status
            .error
            .unwrap()
            .contains("subscription acknowledgement timed out")
    );
    while let Ok(event) = event_rx.try_recv() {
        if let ConversationEvent::PromptFailed { error, .. } = event {
            assert!(error.contains("subscription acknowledgement is pending"));
            failures += 1;
        }
    }
    assert_eq!(failures, 1);
    wait_with_clock_paused("quarantined peer disconnect", || server.is_finished()).await;
    let requests = server.await.unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r["method"] == "thread/resume" && r["params"]["threadId"] == "one")
            .count(),
        1
    );
}

#[tokio::test]
async fn a_new_connection_must_acquire_its_own_acknowledgement() {
    for _ in 0..2 {
        let peer = Peer::start().await;
        let mut rpc = RpcSession::connect(&peer.target).await.unwrap();
        let loaded = rpc.request("thread/loaded/list", json!({})).await.unwrap();
        assert_eq!(loaded["data"][0], "one");
        assert!(!rpc.subscriptions.confirmed("one"));
        rpc.ensure_subscribed(&ThreadId::new("one")).await.unwrap();
        assert_eq!(peer.count("thread/resume"), 1);
        assert!(rpc.subscriptions.confirmed("one"));
    }
}
