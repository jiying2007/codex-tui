use super::{fixture::*, *};
use futures_util::StreamExt;
use tokio_tungstenite::accept_async;

#[tokio::test]
async fn a_close_after_a_queued_resume_ack_cannot_be_undone_when_the_ack_is_drained() {
    for method in [
        "thread/closed",
        "thread/status/changed",
        "thread/archived",
        "thread/deleted",
    ] {
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
            let resume = receive(&mut socket).await;
            assert_eq!(resume["method"], "thread/resume");
            emit(
                &mut socket,
                json!({"id":73,"method":"item/commandExecution/requestApproval","params":{
                    "threadId":"existing","turnId":"turn-1","itemId":"item-1","command":"fixture","cwd":"/fixture"
                }}),
            )
            .await;
            let response = receive(&mut socket).await;
            assert_eq!(response["id"], 73);
            let read = receive(&mut socket).await;
            assert_eq!(read["method"], "fixture/read");
            emit(
                &mut socket,
                json!({"id":resume["id"],"result":{"thread":{"id":"one"}}}),
            )
            .await;
            emit(
                &mut socket,
                json!({"method":method,"params":{"threadId":"one","status":{"type":"notLoaded"}}}),
            )
            .await;
            emit(&mut socket, json!({"id":read["id"],"result":{}})).await;
            if let Ok(Some(Ok(frame))) =
                tokio::time::timeout(Duration::from_secs(3), socket.next()).await
            {
                assert!(!frame.is_text() && !frame.is_binary());
            }
        });
        let mut rpc = RpcSession::connect(&target).await.unwrap();
        rpc.ensure_subscribed(&ThreadId::new("existing"))
            .await
            .unwrap();
        assert!(rpc.ensure_subscribed(&ThreadId::new("one")).await.is_err());
        let approval = rpc.read_message().await.unwrap().unwrap();
        rpc.reject_request(approval["id"].clone(), "fixture response")
            .await
            .unwrap();
        rpc.request("fixture/read", json!({})).await.unwrap();

        let history = WatchSet::default();
        history.admit("existing", || Ok(())).unwrap();
        history.admit("one", || Ok(())).unwrap();
        let queue = WatchSet::default();
        let mut requests = BTreeMap::new();
        let mut threads = BTreeMap::new();
        let (events, mut received) = mpsc::channel(4);
        let acknowledgement = rpc.read_message().await.unwrap().unwrap();
        assert!(
            rpc.subscription_response(&acknowledgement)
                .unwrap()
                .unwrap()
                .is_none(),
            "{method}: a later close suppresses acknowledgement-triggered refresh"
        );
        assert!(!rpc.subscriptions.confirmed("one"));
        let closed = rpc.read_message().await.unwrap().unwrap();
        handle_unsolicited(
            &mut rpc,
            closed,
            &mut threads,
            &mut requests,
            &history,
            &queue,
            &events,
        )
        .await
        .unwrap();
        assert!(!rpc.subscriptions.confirmed("one"));
        let next = rpc
            .subscriptions
            .next_action(&desired(&history, &queue, &requests));
        if matches!(method, "thread/archived" | "thread/deleted") {
            assert!(next.is_none(), "removed owners cannot reacquire");
            assert!(matches!(
                received.try_recv(),
                Ok(ConversationEvent::Failed { .. })
            ));
        } else {
            assert_eq!(next, Some((ThreadId::new("one"), true)));
        }
        assert!(!rpc.is_quarantined());
        drop(rpc);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn closed_and_buffered_history_events_cannot_retry_a_rejected_subscription() {
    for method in [
        "thread/closed",
        "thread/status/changed",
        "item/completed",
        "turn/started",
        "turn/completed",
    ] {
        let (target, listener) = listener().await;
        let server = tokio::spawn(async move {
            let mut socket = accept_async(listener.accept().await.unwrap().0)
                .await
                .unwrap();
            let resume = receive(&mut socket).await;
            emit(
                &mut socket,
                json!({"id":resume["id"],"error":{"code":-32000,"message":"fixture rejection"}}),
            )
            .await;
            let release = receive(&mut socket).await;
            assert_eq!(release["method"], "thread/unsubscribe");
            emit(
                &mut socket,
                json!({"id":release["id"],"result":{"status":"unsubscribed"}}),
            )
            .await;
            emit(
                &mut socket,
                json!({"method":method,"params":{"threadId":"one","status":{"type":"notLoaded"}}}),
            )
            .await;
            let explicit_retry = receive(&mut socket).await;
            assert_eq!(explicit_retry["method"], "thread/resume");
            emit(
                &mut socket,
                json!({"id":explicit_retry["id"],"result":{"thread":{"id":"one"}}}),
            )
            .await;
        });
        let mut rpc = RpcSession::connect(&target).await.unwrap();
        let id = ThreadId::new("one");
        assert!(rpc.ensure_subscribed(&id).await.is_err());
        rpc.change_subscription(&id, false).await.unwrap();
        let closed = rpc.read_message().await.unwrap().unwrap();
        assert_eq!(closed["method"], method);
        let history = WatchSet::default();
        history.admit("one", || Ok(())).unwrap();
        let (events, mut received) = mpsc::channel(4);
        handle_unsolicited(
            &mut rpc,
            closed,
            &mut BTreeMap::new(),
            &mut BTreeMap::new(),
            &history,
            &WatchSet::default(),
            &events,
        )
        .await
        .unwrap();
        assert!(
            !rpc.subscriptions.confirmed("one"),
            "{method} cannot reacquire while draining an old event"
        );
        assert!(received.try_recv().is_err());
        assert!(
            rpc.subscriptions
                .next_action(&BTreeSet::from(["one".into()]))
                .is_none(),
            "{method} does not clear rejection state while its owner remains"
        );
        rpc.ensure_subscribed(&id).await.unwrap();
        assert!(rpc.subscriptions.confirmed("one"));
        assert!(!rpc.is_quarantined());
        server.await.unwrap();
    }
}
