use super::{fixture::*, *};
use tokio_tungstenite::accept_async;

fn approval(id: i64, thread: &str) -> Value {
    json!({"id":id,"method":"item/commandExecution/requestApproval","params":{
        "threadId":thread,"turnId":"turn-1","itemId":"item-1","command":"fixture","cwd":"/fixture"
    }})
}

#[tokio::test]
async fn loaded_threads_need_this_connections_resume_and_release_only_after_last_owner() {
    let peer = Peer::start().await;
    let mut started = start_target(peer.target.clone()).await.unwrap();
    peer.wait_count("thread/loaded/list", 1).await;
    assert_eq!(started.initial.threads.len(), 2);
    assert_eq!(
        peer.count("thread/resume"),
        0,
        "Registry does not subscribe"
    );
    assert!(
        !peer
            .event(json!({"method":"item/completed","params":{"threadId":"one"}}))
            .await
    );
    let id = ThreadId::new("one");
    started.handle.load_conversation(id.clone()).unwrap();
    event(&mut started.handle, |e| {
        matches!(e, ConversationEvent::Loaded(_))
    })
    .await;
    assert_eq!(peer.subscriptions(), BTreeSet::from(["one".into()]));
    started.handle.refresh_thread_queue(id.clone()).unwrap();
    event(&mut started.handle, |e| {
        matches!(e, ConversationEvent::ThreadQueueLoaded(_))
    })
    .await;
    started
        .handle
        .submit_prompt(id.clone(), 1, "fixture prompt".into(), None)
        .unwrap();
    event(&mut started.handle, |e| {
        matches!(e, ConversationEvent::PromptSubmitted { .. })
    })
    .await;
    event(&mut started.handle, |e| {
        matches!(e, ConversationEvent::Loaded(_))
    })
    .await;
    assert_eq!(peer.count("thread/resume"), 1);
    assert_eq!(peer.count("turn/start"), 1);
    assert!(
        peer.event(json!({"method":"item/completed","params":{"threadId":"one"}}))
            .await
    );
    event(&mut started.handle, |e| {
        matches!(e, ConversationEvent::Loaded(_))
    })
    .await;
    assert!(peer.event(approval(41, "one")).await);
    event(&mut started.handle, |e| {
        matches!(e, ConversationEvent::InteractiveRequested(_))
    })
    .await;

    started
        .handle
        .stop_watching_conversation(id.clone())
        .unwrap();
    barrier(&mut started.handle).await;
    assert_eq!(
        peer.count("thread/unsubscribe"),
        0,
        "queue still owns events"
    );
    started
        .handle
        .stop_watching_thread_queue(id.clone())
        .unwrap();
    barrier(&mut started.handle).await;
    assert_eq!(
        peer.count("thread/unsubscribe"),
        0,
        "pending approval still owns events"
    );
    assert!(
        peer.event(
            json!({"method":"serverRequest/resolved","params":{"threadId":"one","requestId":41}})
        )
        .await
    );
    event(&mut started.handle, |e| {
        matches!(e, ConversationEvent::InteractiveResolved { .. })
    })
    .await;
    peer.wait_count("thread/unsubscribe", 1).await;
    assert!(peer.subscriptions().is_empty());
    assert!(
        !peer
            .event(json!({"method":"item/completed","params":{"threadId":"one"}}))
            .await
    );
    started.handle.load_conversation(id.clone()).unwrap();
    event(&mut started.handle, |e| {
        matches!(e, ConversationEvent::Loaded(_))
    })
    .await;
    assert_eq!(
        peer.count("thread/resume"),
        2,
        "released confirmation cannot be reused"
    );
    started.handle.stop_watching_conversation(id).unwrap();
    peer.wait_count("thread/unsubscribe", 2).await;
}

#[tokio::test]
async fn leaving_during_resume_ack_releases_without_reading_or_resurrecting_watch() {
    let peer = Peer::start().await;
    let mut started = start_target(peer.target.clone()).await.unwrap();
    let (observed, release) = peer.hold("thread/resume").await;
    let id = ThreadId::new("one");
    started.handle.load_conversation(id.clone()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), observed)
        .await
        .unwrap()
        .unwrap();
    started.handle.stop_watching_conversation(id).unwrap();
    release.send(()).unwrap();
    peer.wait_count("thread/unsubscribe", 1).await;
    barrier(&mut started.handle).await;
    assert_eq!(peer.count("thread/resume"), 1);
    assert_eq!(peer.count("thread/read"), 0);
    assert!(started.handle.history_watches.snapshot().is_empty());
    assert!(peer.subscriptions().is_empty());
}

#[tokio::test]
async fn full_command_queue_cannot_block_release_or_recreate_a_discarded_watch() {
    let peer = Peer::start().await;
    let mut started = start_target(peer.target.clone()).await.unwrap();
    let id = ThreadId::new("one");
    started.handle.load_conversation(id.clone()).unwrap();
    event(&mut started.handle, |e| {
        matches!(e, ConversationEvent::Loaded(_))
    })
    .await;
    let (observed, release) = peer.hold("thread/read").await;
    started.handle.load_conversation(id.clone()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), observed)
        .await
        .unwrap()
        .unwrap();
    for _ in 0..APP_SERVER_COMMAND_QUEUE_CAPACITY {
        started.handle.load_conversation(id.clone()).unwrap();
    }
    assert!(
        started
            .handle
            .load_conversation(id.clone())
            .unwrap_err()
            .to_string()
            .contains("queue is full")
    );
    started.handle.stop_watching_conversation(id).unwrap();
    release.send(()).unwrap();
    peer.wait_count("thread/unsubscribe", 1).await;
    barrier(&mut started.handle).await;
    assert_eq!(
        peer.count("thread/read"),
        2,
        "queued discarded reads are not delivered"
    );
    assert_eq!(
        peer.count("thread/resume"),
        1,
        "queued discarded reads do not reacquire"
    );
    assert!(peer.subscriptions().is_empty());
}

#[tokio::test]
async fn late_subscription_ack_survives_another_rpc_without_replaying_the_resume() {
    let (target, listener) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept_async(listener.accept().await.unwrap().0)
            .await
            .unwrap();
        let existing = receive(&mut socket).await;
        assert_eq!(existing["method"], "thread/resume");
        emit(
            &mut socket,
            json!({"id":existing["id"],"result":{"thread":{"id":"existing"}}}),
        )
        .await;
        let pending = receive(&mut socket).await;
        assert_eq!(pending["method"], "thread/resume");
        assert_eq!(pending["params"]["threadId"], "one");
        // An existing subscribed thread can request approval while another
        // thread's resume is pending. The actor must service it immediately.
        emit(&mut socket, approval(73, "existing")).await;
        let reply = receive(&mut socket).await;
        assert_eq!(reply["id"], 73);
        assert!(reply.get("error").is_some());
        let read = receive(&mut socket).await;
        assert_eq!(read["method"], "fixture/read");
        emit(
            &mut socket,
            json!({"id":pending["id"],"result":{"thread":{"id":"one"}}}),
        )
        .await;
        emit(&mut socket, json!({"id":read["id"],"result":{"ok":true}})).await;
        for status in ["unsubscribed", "notSubscribed"] {
            let release = receive(&mut socket).await;
            assert_eq!(release["method"], "thread/unsubscribe");
            emit(
                &mut socket,
                json!({"id":release["id"],"result":{"status":status}}),
            )
            .await;
        }
    });
    let mut rpc = RpcSession::connect(&target).await.unwrap();
    rpc.ensure_subscribed(&ThreadId::new("existing"))
        .await
        .unwrap();
    let error = rpc
        .ensure_subscribed(&ThreadId::new("one"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("outcome unknown"));
    assert!(!rpc.subscriptions.confirmed("one"));
    assert!(rpc.subscriptions.pending.is_some());
    let message = rpc.read_message().await.unwrap().unwrap();
    assert_eq!(message["id"], 73);
    rpc.reject_request(message["id"].clone(), "fixture response")
        .await
        .unwrap();
    assert_eq!(
        rpc.request("fixture/read", json!({})).await.unwrap(),
        json!({"ok":true})
    );
    assert!(!rpc.subscriptions.confirmed("one"));
    let ack = rpc.read_message().await.unwrap().unwrap();
    assert_eq!(
        rpc.subscription_response(&ack).unwrap().unwrap(),
        Some(ThreadId::new("one"))
    );
    rpc.ensure_subscribed(&ThreadId::new("one")).await.unwrap();
    rpc.change_subscription(&ThreadId::new("one"), false)
        .await
        .unwrap();
    rpc.change_subscription(&ThreadId::new("existing"), false)
        .await
        .unwrap();
    assert!(rpc.subscriptions.entries.is_empty());
    server.await.unwrap();
}

#[tokio::test]
async fn rejected_resume_is_cleaned_up_and_not_retried_until_a_new_command() {
    let (target, listener) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept_async(listener.accept().await.unwrap().0)
            .await
            .unwrap();
        let resume = receive(&mut socket).await;
        assert_eq!(resume["method"], "thread/resume");
        // Upstream may already have attached a listener before an application
        // error, so release is still required after a definitive rejection.
        emit(
            &mut socket,
            json!({"id":resume["id"],"error":{"code":-32000,"message":"fixture rejected resume"}}),
        )
        .await;
        let release = receive(&mut socket).await;
        assert_eq!(release["method"], "thread/unsubscribe");
        emit(
            &mut socket,
            json!({"id":release["id"],"result":{"status":"notLoaded"}}),
        )
        .await;
        let retry = receive(&mut socket).await;
        assert_eq!(retry["method"], "thread/resume");
        emit(
            &mut socket,
            json!({"id":retry["id"],"result":{"thread":{"id":"one"}}}),
        )
        .await;
    });
    let mut rpc = RpcSession::connect(&target).await.unwrap();
    let id = ThreadId::new("one");
    assert!(rpc.ensure_subscribed(&id).await.is_err());
    assert!(!rpc.is_quarantined());
    let owners = BTreeSet::from(["one".into()]);
    assert_eq!(
        rpc.subscriptions.next_action(&owners),
        Some((id.clone(), false))
    );
    rpc.change_subscription(&id, false).await.unwrap();
    assert!(rpc.subscriptions.next_action(&owners).is_none());
    rpc.ensure_subscribed(&id).await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn malformed_subscription_ack_stops_the_connection_without_another_write() {
    for result in [json!({"thread":{"id":"wrong-thread"}}), json!({})] {
        let (target, listener) = listener().await;
        let server = tokio::spawn(async move {
            let mut socket = accept_async(listener.accept().await.unwrap().0)
                .await
                .unwrap();
            let request = receive(&mut socket).await;
            emit(&mut socket, json!({"id":request["id"],"result":result})).await;
            // The client drops its transport without another application frame.
            use futures_util::StreamExt;
            if let Ok(Some(Ok(frame))) =
                tokio::time::timeout(Duration::from_secs(3), socket.next()).await
            {
                assert!(!frame.is_text() && !frame.is_binary());
            }
        });
        let mut rpc = RpcSession::connect(&target).await.unwrap();
        assert!(rpc.ensure_subscribed(&ThreadId::new("one")).await.is_err());
        assert!(rpc.is_quarantined());
        assert!(
            rpc.request("turn/start", json!({"threadId":"one"}))
                .await
                .is_err()
        );
        drop(rpc);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn older_queued_not_loaded_event_cannot_invalidate_a_newer_resume_ack() {
    let (target, listener) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept_async(listener.accept().await.unwrap().0)
            .await
            .unwrap();
        let request = receive(&mut socket).await;
        emit(
            &mut socket,
            json!({"method":"thread/status/changed","params":{
                "threadId":"one","status":{"type":"notLoaded"}
            }}),
        )
        .await;
        emit(
            &mut socket,
            json!({"id":request["id"],"result":{"thread":{"id":"one"}}}),
        )
        .await;
        let release = receive(&mut socket).await;
        assert_eq!(
            release["method"], "thread/unsubscribe",
            "the old status must not trigger another resume"
        );
        emit(
            &mut socket,
            json!({"id":release["id"],"result":{"status":"unsubscribed"}}),
        )
        .await;
    });
    let mut rpc = RpcSession::connect(&target).await.unwrap();
    let id = ThreadId::new("one");
    rpc.ensure_subscribed(&id).await.unwrap();
    let old_status = rpc.read_message().await.unwrap().unwrap();
    let (tx, _rx) = mpsc::channel(4);
    handle_unsolicited(
        &mut rpc,
        old_status,
        &mut BTreeMap::new(),
        &mut BTreeMap::new(),
        &WatchSet::default(),
        &WatchSet::default(),
        &tx,
    )
    .await
    .unwrap();
    assert!(rpc.subscriptions.confirmed("one"));
    rpc.ensure_subscribed(&id).await.unwrap();
    rpc.change_subscription(&id, false).await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn authoritative_removal_retires_approval_ownership_without_resuming_deleted_threads() {
    for method in ["thread/archived", "thread/deleted"] {
        let peer = Peer::start().await;
        let mut rpc = RpcSession::connect(&peer.target).await.unwrap();
        rpc.ensure_subscribed(&ThreadId::new("one")).await.unwrap();
        let history = WatchSet::default();
        history.admit("one", || Ok(())).unwrap();
        let queue = WatchSet::default();
        let mut requests = BTreeMap::new();
        let mut threads = BTreeMap::new();
        let (tx, mut events) = mpsc::channel(8);
        assert!(peer.event(approval(41, "one")).await);
        let request = rpc.read_message().await.unwrap().unwrap();
        handle_unsolicited(
            &mut rpc,
            request,
            &mut threads,
            &mut requests,
            &history,
            &queue,
            &tx,
        )
        .await
        .unwrap();
        assert_eq!(requests.len(), 1);
        assert!(matches!(
            events.try_recv(),
            Ok(ConversationEvent::InteractiveRequested(_))
        ));

        assert!(
            peer.event(json!({"method":method,"params":{"threadId":"one"}}))
                .await
        );
        let removal = rpc.read_message().await.unwrap().unwrap();
        handle_unsolicited(
            &mut rpc,
            removal,
            &mut threads,
            &mut requests,
            &history,
            &queue,
            &tx,
        )
        .await
        .unwrap();
        assert!(
            requests.is_empty(),
            "a retired approval is no longer an owner"
        );
        assert!(desired(&history, &queue, &requests).is_empty());
        assert!(rpc.subscriptions.next_action(&BTreeSet::new()).is_none());
        let mut resolved = false;
        while let Ok(event) = events.try_recv() {
            resolved |= matches!(
                event,
                ConversationEvent::InteractiveResolved {
                    request_id: RpcRequestId::Integer(41)
                }
            );
        }
        assert!(
            resolved,
            "the pending UI request is retired with its thread"
        );
        assert_eq!(peer.count("thread/resume"), 1);
        assert!(peer.subscriptions().is_empty());
    }
}
