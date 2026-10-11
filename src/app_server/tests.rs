use super::*;
use crate::app_server_wire::decode_wire_line;
use crate::backend::{CodexBackend, FakeBackend};

#[test]
fn app_server_channels_are_bounded_in_production() {
    let source = include_str!("../app_server.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    assert!(!production.contains("unbounded_channel"));
    assert!(!production.contains("UnboundedSender"));
    assert!(!production.contains("UnboundedReceiver"));
}

#[test]
fn app_server_command_queue_reports_backpressure() {
    let (tx, _rx) = mpsc::channel(1);
    queue_backend_command(
        &tx,
        BackendCommand::StopWatchingConversation(ThreadId::new("first")),
    )
    .expect("first command");
    let error = queue_backend_command(
        &tx,
        BackendCommand::StopWatchingConversation(ThreadId::new("second")),
    )
    .expect_err("second command must hit bounded queue");
    assert!(error.to_string().contains("queue is full"));
}

#[tokio::test]
async fn conversation_event_queue_backpressures_without_dropping() {
    let (tx, mut rx) = mpsc::channel(1);
    send_conversation_event(&tx, ConversationEvent::GoalCleared(ThreadId::new("first"))).await;

    let second_tx = tx.clone();
    let second = tokio::spawn(async move {
        send_conversation_event(
            &second_tx,
            ConversationEvent::GoalCleared(ThreadId::new("second")),
        )
        .await;
    });
    tokio::task::yield_now().await;
    assert!(
        !second.is_finished(),
        "second event must wait while the bounded queue is full"
    );

    assert!(matches!(
        rx.recv().await,
        Some(ConversationEvent::GoalCleared(thread_id)) if thread_id.0 == "first"
    ));
    tokio::time::timeout(Duration::from_secs(1), second)
        .await
        .expect("second send must complete after capacity is released")
        .expect("second sender task");
    assert!(matches!(
        rx.recv().await,
        Some(ConversationEvent::GoalCleared(thread_id)) if thread_id.0 == "second"
    ));
}

#[tokio::test]
async fn rpc_deadline_fails_closed_without_waiting_forever() {
    let error = with_rpc_deadline::<Value>(
        "fixture/request",
        Duration::from_millis(5),
        std::future::pending(),
    )
    .await
    .expect_err("pending RPC must hit its deadline");
    assert!(
        error.to_string().contains("fixture/request timed out"),
        "timeout must retain method context: {error:#}"
    );
}

#[tokio::test]
async fn queued_approval_preempts_a_ready_command_after_rpc_yield() {
    use crate::app_server_target::ResolvedAppServerEndpoint;
    use futures_util::StreamExt;
    use tokio_tungstenite::accept_async;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let address = listener.local_addr().expect("mock address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        // A competing thread/read RPC cannot complete: the fixture never
        // replies to commands. The queued approval must win first.
        let _ = tokio::time::timeout(Duration::from_secs(3), socket.next()).await;
    });
    let target = ResolvedAppServerTarget {
        name: "queued-approval-priority".into(),
        endpoint: ResolvedAppServerEndpoint::WebSocket {
            url: format!("ws://{address}/rpc"),
            auth_token: None,
        },
    };
    let mut rpc = RpcSession::connect(&target).await.expect("connect");
    rpc.queued_messages.push_back(json!({
        "id": "pending-approval",
        "method": "item/commandExecution/requestApproval",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "item-1",
            "command": "echo fixture",
            "cwd": "/repo"
        }
    }));
    let status = BackendStatus::starting("priority-fixture");
    let (snapshot_tx, _snapshot_rx) = watch::channel(BackendSnapshot {
        generation: 0,
        threads: vec![],
        status: status.clone(),
    });
    let (event_tx, mut event_rx) = mpsc::channel(4);
    let (command_tx, command_rx) = mpsc::channel(4);
    command_tx
        .try_send(BackendCommand::LoadConversation(ThreadId::new(
            "waiting-command",
        )))
        .expect("ready competing command");

    let actor = tokio::spawn(run_registry_actor(
        rpc,
        vec![],
        status,
        snapshot_tx,
        event_tx,
        command_rx,
        (None, target),
    ));
    let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .expect("approval must be emitted without RPC timeout")
        .expect("conversation event");
    assert!(matches!(
        event,
        ConversationEvent::InteractiveRequested(request)
            if request.request_id == RpcRequestId::String("pending-approval".into())
    ));
    actor.abort();
    server.abort();
}

#[tokio::test]
async fn interactive_server_request_yields_blocked_rpc_before_deadline() {
    use crate::app_server_target::ResolvedAppServerEndpoint;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let address = listener.local_addr().expect("mock address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("handshake");
        let first = socket.next().await.expect("request frame").expect("frame");
        let Message::Text(first) = first else {
            panic!("expected JSON-RPC text request")
        };
        let sent: Value = serde_json::from_str(first.as_str()).expect("request JSON");
        assert_eq!(sent["method"], "fixture/pending");
        // The mocked server refuses to answer the active RPC until it has
        // received a response to this separate interactive server request.
        socket
            .send(Message::Text(
                json!({
                    "id": "approval-42",
                    "method": "item/commandExecution/requestApproval",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "itemId": "item-1",
                        "command": "echo fixture",
                        "cwd": "/repo"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send server request");
        let next = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("interactive response must arrive before RPC deadline")
            .expect("response frame")
            .expect("response");
        let Message::Text(next) = next else {
            panic!("expected interactive response text frame")
        };
        let reply: Value = serde_json::from_str(next.as_str()).expect("reply JSON");
        assert_eq!(reply["id"], "approval-42");
        assert!(reply.get("error").is_some());
    });
    let target = ResolvedAppServerTarget {
        name: "interactive-yield-fixture".into(),
        endpoint: ResolvedAppServerEndpoint::WebSocket {
            url: format!("ws://{address}/rpc"),
            auth_token: None,
        },
    };
    let mut rpc = RpcSession::connect(&target).await.expect("connect");
    let error = tokio::time::timeout(
        Duration::from_secs(2),
        rpc.request("fixture/pending", json!({})),
    )
    .await
    .expect("yield without five-second RPC timeout")
    .expect_err("RPC must yield to the interactive request");
    assert!(error.to_string().contains("RPC outcome unknown"));
    let pending = rpc
        .read_message()
        .await
        .expect("read queued request")
        .expect("queued request");
    assert_eq!(pending["id"], "approval-42");
    assert!(
        parse_interactive_request(&pending)
            .expect("parse queued interactive request")
            .is_some()
    );
    rpc.reject_request(pending["id"].clone(), "test fixture rejection")
        .await
        .expect("answer queued server request");
    server.await.expect("mock server");
}

#[tokio::test]
async fn isolated_search_does_not_block_live_app_server_transport() {
    use crate::app_server_target::ResolvedAppServerEndpoint;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let addr = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        let (live, _) = listener.accept().await.expect("live connection");
        let mut live = accept_async(live).await.expect("live handshake");
        let (search, _) = listener.accept().await.expect("search connection");
        let mut search = accept_async(search).await.expect("search handshake");

        let initialize = search.next().await.expect("initialize").expect("frame");
        let Message::Text(initialize) = initialize else {
            panic!("initialize text")
        };
        let request: Value = serde_json::from_str(initialize.as_str()).expect("JSON");
        assert_eq!(request["method"], "initialize");
        search
            .send(Message::Text(
                json!({"id":request["id"],"result":{"serverInfo":{"version":"fixture"}}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("initialize reply");
        let initialized = search.next().await.expect("initialized").expect("frame");
        let Message::Text(initialized) = initialized else {
            panic!("initialized text")
        };
        assert_eq!(
            serde_json::from_str::<Value>(initialized.as_str()).unwrap()["method"],
            "initialized"
        );

        let request = search.next().await.expect("thread search").expect("frame");
        let Message::Text(request) = request else {
            panic!("search text")
        };
        let request: Value = serde_json::from_str(request.as_str()).expect("search json");
        assert_eq!(request["method"], "thread/search");

        let live_request = live.next().await.expect("live command").expect("frame");
        let Message::Text(live_request) = live_request else {
            panic!("live text")
        };
        let live_request: Value = serde_json::from_str(live_request.as_str()).expect("live json");
        assert_eq!(live_request["method"], "fixture/heartbeat");
        live.send(Message::Text(
            json!({"id":live_request["id"],"result":{"healthy":true}})
                .to_string()
                .into(),
        ))
        .await
        .expect("live reply");

        tokio::time::sleep(Duration::from_millis(75)).await;
        search
            .send(Message::Text(
                json!({"id":request["id"],"result":{"data":[],"nextCursor":null}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("search reply");
    });

    let target = ResolvedAppServerTarget {
        name: "two-connection-fixture".into(),
        endpoint: ResolvedAppServerEndpoint::WebSocket {
            url: format!("ws://{addr}/rpc"),
            auth_token: None,
        },
    };
    let mut live = AppServerTransport::connect(&target)
        .await
        .expect("live connect");
    let task_target = target.clone();
    let search =
        tokio::spawn(
            async move { isolated_transcript_search(&task_target, "needle".into()).await },
        );
    live.write_json(&json!({"id":77,"method":"fixture/heartbeat","params":{}}))
        .await
        .expect("live write");
    let live_response = tokio::time::timeout(Duration::from_secs(2), live.read_json())
        .await
        .expect("live reply must remain responsive")
        .expect("read")
        .expect("response");
    assert_eq!(live_response["result"]["healthy"], true);
    let result = search.await.expect("search task").expect("search result");
    assert!(result.complete);
    assert!(result.hits.is_empty());
    server.await.expect("mock server");
}

#[test]
fn transcript_complete_requires_all_occurrences_and_budget_headroom() {
    assert!(transcript_results_complete(None, false, 3));
    assert!(!transcript_results_complete(None, true, 3));
    assert!(!transcript_results_complete(Some("next"), false, 3));
    assert!(!transcript_results_complete(
        None,
        false,
        TRANSCRIPT_SEARCH_RESULT_LIMIT
    ));
    let (_, cursor) = crate::transcript_search::parse_search_occurrences(
        ThreadId::new("T"),
        json!({"data":[],"nextCursor":"next-occurrences"}),
    )
    .expect("fixture");
    assert!(!transcript_results_complete(None, cursor.is_some(), 1));
}

#[test]
fn rpc_eof_is_reported_as_closed_during_request() {
    let error =
        rpc_message_or_closed(None, "thread/list").expect_err("EOF must fail the active request");
    assert!(
        error
            .to_string()
            .contains("codex app-server closed while waiting for thread/list")
    );
}

#[test]
fn malformed_rpc_wire_line_is_rejected_fail_closed() {
    let error = decode_wire_line("{not-json").expect_err("malformed protocol must fail");
    assert!(
        error.to_string().contains("decode app-server JSON line"),
        "wire errors must retain protocol context: {error:#}"
    );
}

#[test]
fn rpc_wait_queue_keeps_semantic_messages_and_drops_ignored_streaming_delta() {
    assert!(!should_queue_rpc_message(&json!({
        "method": "item/agentMessage/delta",
        "params": {"threadId": "thread-1", "delta": "x"}
    })));
    assert!(should_queue_rpc_message(&json!({
        "id": 42,
        "method": "item/fileChange/requestApproval",
        "params": {"threadId": "thread-1"}
    })));
    assert!(should_queue_rpc_message(&json!({
        "method": "serverRequest/resolved",
        "params": {"requestId": 42}
    })));
    assert!(should_queue_rpc_message(&json!({
        "method": "thread/status/changed",
        "params": {"threadId": "thread-1"}
    })));
    assert!(should_queue_rpc_message(&json!({
        "method": "turn/completed",
        "params": {"threadId": "thread-1"}
    })));
}

#[test]
fn rpc_wait_queue_is_hard_bounded_without_dropping_existing_semantic_messages() {
    let mut queue = VecDeque::new();
    for index in 0..RPC_QUEUED_MESSAGE_CAPACITY {
        enqueue_rpc_message(
            &mut queue,
            json!({"method": "turn/completed", "params": {"threadId": index.to_string()}}),
        )
        .expect("queue semantic message");
    }
    let error = enqueue_rpc_message(
        &mut queue,
        json!({"method": "thread/status/changed", "params": {"threadId": "overflow"}}),
    )
    .expect_err("bounded queue must fail closed");
    assert!(error.to_string().contains("capacity exceeded"));
    assert_eq!(queue.len(), RPC_QUEUED_MESSAGE_CAPACITY);
    assert_eq!(
        queue
            .front()
            .and_then(|message| message.pointer("/params/threadId").and_then(Value::as_str)),
        Some("0")
    );
}

#[test]
fn production_rpc_decode_error_does_not_echo_unbounded_wire_payload() {
    let actor = include_str!("../app_server.rs")
        .split("#[cfg(test)]")
        .next()
        .expect("production actor source");
    let wire = include_str!("../app_server_wire.rs");
    let transport = include_str!("../app_server_transport.rs");
    assert!(actor.contains("self.transport.read_json"));
    assert!(!wire.contains("decode app-server JSON line: {line}"));
    assert!(wire.contains("decode app-server JSON line ({} bytes)"));
    assert!(!transport.contains("decode App Server stdio JSON: {line}"));
    assert!(transport.contains("decode App Server stdio JSON"));
}

#[test]
fn registry_notifications_update_only_registry_relevant_state() {
    let mut threads = BTreeMap::new();
    assert_eq!(
        apply_registry_notification(
            "thread/started",
            &json!({
                "thread": {
                    "id": "thread-new",
                    "preview": "New work",
                    "updatedAt": 7,
                    "status": {"type": "idle"},
                    "cwd": "/repo",
                    "source": "cli"
                }
            }),
            Some(8),
            &mut threads,
        )
        .expect("started"),
        Some(true)
    );
    let thread = threads.get("thread-new").expect("new thread");
    assert_eq!(thread.title, "New work");
    assert_eq!(thread.metadata.loaded, Some(true));

    assert_eq!(
        apply_registry_notification(
            "thread/status/changed",
            &json!({
                "threadId": "thread-new",
                "status": {
                    "type": "active",
                    "activeFlags": ["waitingOnApproval"]
                }
            }),
            Some(9),
            &mut threads,
        )
        .expect("status"),
        Some(true)
    );
    let thread = threads.get("thread-new").expect("updated thread");
    assert_eq!(thread.runtime, crate::domain::RuntimeStatus::WaitingHuman);
    assert_eq!(thread.metadata.updated_at, 9);

    assert_eq!(
        apply_registry_notification(
            "thread/name/updated",
            &json!({"threadId": "thread-new", "threadName": "Renamed"}),
            Some(10),
            &mut threads,
        )
        .expect("name"),
        Some(true)
    );
    assert_eq!(threads["thread-new"].title, "Renamed");

    assert_eq!(
        apply_registry_notification(
            "item/agentMessage/delta",
            &json!({"threadId": "thread-new", "delta": "ignored"}),
            Some(11),
            &mut threads,
        )
        .expect("delta"),
        None
    );

    assert_eq!(
        apply_registry_notification(
            "thread/archived",
            &json!({"threadId": "thread-new"}),
            Some(12),
            &mut threads,
        )
        .expect("archived"),
        Some(true)
    );
    assert!(!threads.contains_key("thread-new"));
}

#[test]
fn unarchive_and_cleared_metadata_schedule_upstream_reconcile() {
    for message in [
        json!({"method":"thread/unarchived","params":{"threadId":"a"}}),
        json!({"method":"thread/project/updated","params":{"threadId":"a","projectId":null}}),
        json!({"method":"thread/name/updated","params":{"threadId":"a"}}),
        json!({"method":"thread/name/updated","params":{"threadId":"a","threadName":""}}),
    ] {
        assert!(
            requires_authoritative_registry_refresh(&message),
            "{message}"
        );
    }
    for message in [
        json!({"method":"thread/archived","params":{"threadId":"a"}}),
        json!({"method":"thread/project/updated","params":{"threadId":"a","projectId":"B"}}),
        json!({"method":"thread/project/updated","params":{"threadId":"a","projectId":45}}),
        json!({"method":"thread/unarchived","params":{}}),
    ] {
        assert!(
            !requires_authoritative_registry_refresh(&message),
            "{message}"
        );
    }
}

#[test]
fn registry_full_reconcile_is_low_frequency_fallback() {
    assert!(REFRESH_INTERVAL >= Duration::from_secs(5 * 60));
}

#[test]
fn bootstrap_registry_completeness_follows_actual_pagination_exhaustion() {
    let source = include_str!("../app_server.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    assert!(production.contains("status.registry_complete = load.registry_complete;"));
    assert!(production.contains("registry_complete: true"));
    assert!(production.contains("status.registry_complete = true;"));
}

#[test]
fn startup_registry_hydration_is_pagewise_and_command_priority() {
    assert_eq!(STARTUP_REGISTRY_PAGE_LIMIT, 1);
    assert!(REGISTRY_HYDRATION_YIELD_INTERVAL <= Duration::from_millis(50));

    let source = include_str!("../app_server.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    let actor = production
        .split("async fn run_registry_actor")
        .nth(1)
        .expect("registry actor");
    let command = actor
        .find("command = command_rx.recv()")
        .expect("command branch");
    let hydration = actor
        .find("_ = hydration_tick.tick(), if registry_page_can_advance")
        .expect("hydration branch");

    assert!(
        production
            .contains("bootstrap_registry(&mut rpc, Some(STARTUP_REGISTRY_PAGE_LIMIT)).await?")
    );
    assert!(production.contains("bootstrap_registry(&mut rpc, None).await?"));
    let bootstrap = production
        .split("async fn bootstrap_registry")
        .nth(1)
        .and_then(|tail| tail.split("pub async fn start").next())
        .expect("bootstrap body");

    assert!(actor.contains("biased;"));
    assert!(command < hydration);
    assert!(actor.contains("hydration_tick.reset();"));
    assert!(!bootstrap.contains("load_all_loaded_ids"));
    assert!(actor.contains("loaded_hydration = Some(LoadedIdHydration::default())"));
    assert!(actor.contains("load_loaded_ids_page(&mut rpc, state.cursor.clone()).await"));
}

#[test]
fn startup_hydration_publish_is_batched_but_final_page_is_immediate() {
    assert_eq!(REGISTRY_HYDRATION_PUBLISH_PAGE_INTERVAL, 10);
    assert!(!hydration_publish_due(1, false));
    assert!(!hydration_publish_due(9, false));
    assert!(hydration_publish_due(10, false));
    assert!(hydration_publish_due(1, true));
}

#[test]
fn hydration_merge_preserves_newer_live_state_and_tombstones() {
    let mut current = FakeBackend::scaled(1)
        .snapshot()
        .threads
        .into_iter()
        .next()
        .expect("current");
    current.id = ThreadId::new("same");
    current.title = "live-newer".into();
    current.metadata.updated_at = 20;

    let mut stale = current.clone();
    stale.title = "stale-page".into();
    stale.metadata.updated_at = 10;

    let mut fresh = current.clone();
    fresh.id = ThreadId::new("fresh");
    fresh.title = "fresh-page".into();
    fresh.metadata.updated_at = 30;

    let mut tombstoned = current.clone();
    tombstoned.id = ThreadId::new("gone");
    tombstoned.metadata.updated_at = 30;

    let mut threads = BTreeMap::from([(current.id.0.clone(), current)]);
    let tombstones = BTreeSet::from(["gone".to_string()]);
    merge_registry_hydration_page(&mut threads, vec![stale, fresh, tombstoned], &tombstones);

    assert_eq!(threads["same"].title, "live-newer");
    assert_eq!(threads["fresh"].title, "fresh-page");
    assert!(!threads.contains_key("gone"));
}

#[test]
fn hydration_tombstones_follow_archive_and_reappearance_notifications() {
    let mut hydration = RegistryHydration {
        cursor: "next".into(),
        use_state_db_only: false,
        optimized_query: true,
        tombstones: BTreeSet::new(),
        pages_since_publish: 0,
    };

    observe_registry_hydration_message(
        &mut hydration,
        &json!({"method": "thread/archived", "params": {"threadId": "thread-1"}}),
    );
    assert!(hydration.tombstones.contains("thread-1"));

    observe_registry_hydration_message(
        &mut hydration,
        &json!({
            "method": "thread/started",
            "params": {"thread": {"id": "thread-1"}}
        }),
    );
    assert!(!hydration.tombstones.contains("thread-1"));
}

#[test]
fn loaded_metadata_hydration_overlays_live_status_on_snapshot() {
    let mut threads = by_id(FakeBackend::scaled(3).snapshot().threads);
    let ids = threads.keys().cloned().collect::<Vec<_>>();
    let first = ids[0].clone();
    let second = ids[1].clone();
    let third = ids[2].clone();

    let hydration = LoadedIdHydration {
        cursor: None,
        ids: BTreeSet::from([first.clone(), second.clone()]),
        live_overrides: BTreeMap::from([(first.clone(), false), (third.clone(), true)]),
    };
    let mut status = BackendStatus::fake();

    finish_loaded_id_hydration(hydration, &mut threads, &mut status);

    assert_eq!(threads[&first].metadata.loaded, Some(false));
    assert_eq!(threads[&second].metadata.loaded, Some(true));
    assert_eq!(threads[&third].metadata.loaded, Some(true));
    assert!(
        status
            .capabilities
            .iter()
            .any(|capability| capability == "thread/loaded/list")
    );
    assert!(
        !status
            .optional_capabilities_missing
            .iter()
            .any(|capability| capability == "thread/loaded/list")
    );
}

#[test]
fn periodic_registry_reconcile_is_pagewise() {
    let source = include_str!("../app_server.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    assert!(production.contains("load_registry_with_page_limit(&mut rpc, true, Some(1)).await"));
    assert!(production.contains("reconcile: Option<RegistryReconcile>"));
    assert!(production.contains("loaded_hydration.is_none()"));
    assert!(!production.contains("_ = refresh.tick(), if hydration.is_none() =>"));
}

#[test]
fn reconcile_finalization_preserves_live_overrides_and_removals() {
    let mut base = FakeBackend::scaled(3).snapshot().threads;
    base[0].id = ThreadId::new("live");
    base[0].title = "live-new".into();
    base[0].metadata.updated_at = 30;
    base[1].id = ThreadId::new("removed");
    base[2].id = ThreadId::new("stable");

    let live_threads = by_id(base.clone());

    let mut stale_live = base[0].clone();
    stale_live.title = "stale-candidate".into();
    stale_live.metadata.updated_at = 10;
    let candidate = BTreeMap::from([
        ("live".to_string(), stale_live),
        ("removed".to_string(), base[1].clone()),
        ("stable".to_string(), base[2].clone()),
    ]);

    let reconcile = RegistryReconcile {
        hydration: RegistryHydration {
            cursor: "done".into(),
            use_state_db_only: true,
            optimized_query: true,
            tombstones: BTreeSet::from(["removed".to_string()]),
            pages_since_publish: 0,
        },
        candidate,
        live_overrides: BTreeSet::from(["live".to_string()]),
    };

    let mut threads = live_threads;
    let mut status = BackendStatus::fake();
    finish_registry_reconcile(reconcile, &mut threads, &mut status);

    assert_eq!(threads["live"].title, "live-new");
    assert!(!threads.contains_key("removed"));
    assert!(threads.contains_key("stable"));
}

#[test]
fn eager_goal_probe_queue_is_bounded_and_prefers_recent_threads() {
    let mut threads = BTreeMap::new();
    for index in 0..150 {
        let mut thread = FakeBackend::seeded()
            .snapshot()
            .threads
            .into_iter()
            .next()
            .expect("thread");
        thread.id = ThreadId::new(format!("thread-{index:03}"));
        thread.metadata.updated_at = index;
        threads.insert(thread.id.0.clone(), thread);
    }

    let queue = eager_goal_probe_queue(&threads, &BTreeSet::new());
    assert_eq!(queue.len(), GOAL_EAGER_PROBE_LIMIT);
    assert_eq!(queue.front().map(|id| id.0.as_str()), Some("thread-149"));
    assert_eq!(queue.back().map(|id| id.0.as_str()), Some("thread-050"));

    let probed = ["thread-149".to_string(), "thread-148".to_string()]
        .into_iter()
        .collect();
    let queue = eager_goal_probe_queue(&threads, &probed);
    assert_eq!(queue.len(), GOAL_EAGER_PROBE_LIMIT);
    assert_eq!(queue.front().map(|id| id.0.as_str()), Some("thread-147"));
}

#[test]
fn registry_query_optimization_falls_back_only_for_invalid_thread_list_params() {
    let invalid = anyhow::Error::new(RpcResponseError {
        method: "thread/list".into(),
        code: Some(-32602),
        message: "unknown field useStateDbOnly".into(),
    });
    assert!(is_registry_query_optimization_unsupported(&invalid));

    let unrelated = anyhow::Error::new(RpcResponseError {
        method: "thread/list".into(),
        code: Some(-32000),
        message: "database unavailable".into(),
    });
    assert!(!is_registry_query_optimization_unsupported(&unrelated));

    let other_method = anyhow::Error::new(RpcResponseError {
        method: "thread/read".into(),
        code: Some(-32602),
        message: "unknown field".into(),
    });
    assert!(!is_registry_query_optimization_unsupported(&other_method));
}

#[test]
fn history_compatibility_matches_official_error_semantics() {
    let method_not_found = anyhow::Error::new(RpcResponseError {
        method: "thread/turns/list".into(),
        code: Some(-32601),
        message: "method not found".into(),
    });
    assert!(is_history_pagination_unsupported(&method_not_found));

    let legacy_invalid_params = anyhow::Error::new(RpcResponseError {
        method: "thread/resume".into(),
        code: Some(-32602),
        message: "unknown field excludeTurns".into(),
    });
    assert!(is_history_pagination_unsupported(&legacy_invalid_params));

    let ordinary_failure = anyhow::Error::new(RpcResponseError {
        method: "thread/items/list".into(),
        code: Some(-32000),
        message: "database unavailable".into(),
    });
    assert!(!is_history_pagination_unsupported(&ordinary_failure));
}

#[test]
fn goal_capability_degrades_only_for_unsupported_protocol_errors() {
    let method_not_found = anyhow::Error::new(RpcResponseError {
        method: "thread/goal/get".into(),
        code: Some(-32601),
        message: "method not found".into(),
    });
    assert!(is_goal_unsupported(&method_not_found));

    let old_invalid_params = anyhow::Error::new(RpcResponseError {
        method: "thread/goal/get".into(),
        code: Some(-32602),
        message: "unknown Goal request".into(),
    });
    assert!(is_goal_unsupported(&old_invalid_params));

    let ordinary_failure = anyhow::Error::new(RpcResponseError {
        method: "thread/goal/get".into(),
        code: Some(-32000),
        message: "database unavailable".into(),
    });
    assert!(!is_goal_unsupported(&ordinary_failure));
}

#[test]
fn registry_snapshot_delivery_coalesces_to_latest_value() {
    let initial = BackendSnapshot {
        generation: 0,
        threads: vec![],
        status: BackendStatus::fake(),
    };
    let (tx, mut rx) = watch::channel(initial);

    tx.send(BackendSnapshot {
        generation: 1,
        threads: vec![],
        status: BackendStatus::fake(),
    })
    .expect("send generation 1");
    tx.send(BackendSnapshot {
        generation: 2,
        threads: vec![],
        status: BackendStatus::fake(),
    })
    .expect("send generation 2");

    assert_eq!(
        try_recv_latest_snapshot(&mut rx)
            .expect("latest snapshot")
            .generation,
        2
    );
    assert!(try_recv_latest_snapshot(&mut rx).is_none());
}

#[test]
fn registry_snapshot_orders_newest_threads_first() {
    let mut threads = BTreeMap::new();
    for (id, updated_at) in [("thread-old", 10), ("thread-new", 30), ("thread-mid", 20)] {
        let mut thread = FakeBackend::seeded()
            .snapshot()
            .threads
            .into_iter()
            .next()
            .expect("thread");
        thread.id = ThreadId::new(id);
        thread.metadata.updated_at = updated_at;
        threads.insert(id.to_string(), thread);
    }

    let snapshot = snapshot(1, &threads, &BackendStatus::fake());
    assert_eq!(
        snapshot
            .threads
            .iter()
            .map(|thread| thread.id.0.as_str())
            .collect::<Vec<_>>(),
        vec!["thread-new", "thread-mid", "thread-old"]
    );
}

#[test]
fn initialize_metadata_is_capability_oriented_not_version_gated() {
    let status = status_from_initialize(&json!({
        "userAgent": "codex-cli 0.157.1",
        "codexHome": "/home/user/.codex",
        "platformFamily": "unix",
        "platformOs": "linux",
        "serverInfo": {"version": "0.157.1"}
    }));
    assert_eq!(status.version.as_deref(), Some("0.157.1"));
    assert_eq!(status.platform.as_deref(), Some("unix/linux"));
    assert!(status.capabilities.is_empty());
}
