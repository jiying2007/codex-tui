//! Actual RpcSession + local WebSocket peer, no live account or remote side effect.
use super::*;
use crate::app_server_target::ResolvedAppServerEndpoint;
use crate::transcript_search::TranscriptSearchHit;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{accept_async, tungstenite::Message};

async fn peer(replies: Vec<(&'static str, Value)>) -> (RpcSession, JoinHandle<Vec<Value>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        let mut requests = vec![];
        for (method, result) in replies {
            let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .expect("bounded fixture read");
            let Some(Ok(frame)) = frame else {
                break;
            };
            if frame.is_close() {
                break;
            }
            let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            assert_eq!(request["method"], method);
            socket
                .send(Message::Text(
                    json!({"id": request["id"], "result": result})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            requests.push(request);
        }
        requests
    });
    let rpc = RpcSession::connect(&ResolvedAppServerTarget {
        name: "audit-wire".into(),
        endpoint: ResolvedAppServerEndpoint::WebSocket {
            url: format!("ws://{address}"),
            auth_token: None,
        },
    })
    .await
    .unwrap();
    (rpc, server)
}

#[tokio::test]
async fn repeating_registry_cursor_preserves_partial_rows_without_authority() {
    let (mut rpc, server) = peer(vec![
        (
            "thread/list",
            json!({"data":[{"id":"one"}],"nextCursor":"repeat"}),
        ),
        (
            "thread/list",
            json!({"data":[{"id":"one"}],"nextCursor":"repeat"}),
        ),
        ("thread/list", json!({"data":[],"nextCursor":null})),
    ])
    .await;
    let load = load_registry_with_page_limit(&mut rpc, false, None)
        .await
        .unwrap();
    assert_eq!(load.threads.len(), 1);
    assert!(!load.registry_complete);
    assert!(load.hydration.is_none());
    assert!(load.error.unwrap().contains("repeated cursor"));
    drop(rpc);
    assert_eq!(server.await.unwrap().len(), 2);
}

#[tokio::test]
async fn hydration_carries_bootstrap_cursor_history_and_loaded_scan_also_stops() {
    let (mut rpc, server) = peer(vec![
        (
            "thread/list",
            json!({"data":[{"id":"one"}],"nextCursor":"repeat"}),
        ),
        (
            "thread/list",
            json!({"data":[{"id":"two"}],"nextCursor":"repeat"}),
        ),
    ])
    .await;
    let mut load = load_registry_with_page_limit(&mut rpc, false, Some(1))
        .await
        .unwrap();
    let mut hydration = load.hydration.take().unwrap();
    assert!(
        guarded_registry_page(
            &mut rpc,
            Some(hydration.cursor),
            false,
            true,
            &mut hydration.budget
        )
        .await
        .is_err()
    );
    drop(rpc);
    assert_eq!(server.await.unwrap().len(), 2);
    let (mut rpc, server) = peer(vec![
        (
            "thread/loaded/list",
            json!({"data":["one"],"nextCursor":"repeat"}),
        ),
        (
            "thread/loaded/list",
            json!({"data":["one"],"nextCursor":"repeat"}),
        ),
    ])
    .await;
    assert!(
        load_all_loaded_ids(&mut rpc)
            .await
            .unwrap_err()
            .to_string()
            .contains("repeated cursor")
    );
    drop(rpc);
    assert_eq!(server.await.unwrap().len(), 2);
}

fn hit(id: &str) -> TranscriptSearchHit {
    TranscriptSearchHit {
        thread_id: ThreadId::new("thread"),
        turn_id: Some("turn".into()),
        item_id: Some(id.into()),
        snippet: "fixture".into(),
        turn_cursor: None,
        match_start_utf16: None,
        match_end_utf16: None,
    }
}
fn items(start: usize, end: usize, cursor: Option<&str>) -> Value {
    json!({"data": (start..end).map(|i| json!({"turnId":"turn", "item":{"id":format!("item-{i}"), "type":"agentMessage", "text":format!("item {i}")}})).collect::<Vec<_>>(), "nextCursor":cursor})
}

#[tokio::test]
async fn exact_hit_at_items_200_and_201_is_found_with_global_descending_continuation() {
    for target in [199, 200] {
        let (mut rpc, server) = peer(vec![
            (
                "thread/read",
                json!({"thread":{"id":"thread","name":"fixture"}}),
            ),
            ("thread/items/list", items(0, 200, Some("global-older-1"))),
            ("thread/items/list", items(200, 202, Some("global-older-2"))),
        ])
        .await;
        let page = load_transcript_hit(&mut rpc, &hit(&format!("item-{target}")))
            .await
            .unwrap();
        assert!(
            page.items
                .iter()
                .any(|item| item.item_id == format!("item-{target}"))
        );
        assert!(page.items.len() <= 200);
        assert_eq!(
            page.next_item_cursor.as_deref(),
            Some(if target == 199 {
                "global-older-1"
            } else {
                "global-older-2"
            })
        );
        drop(rpc);
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), if target == 199 { 2 } else { 3 });
        for request in requests.iter().skip(1) {
            assert_eq!(request["params"]["sortDirection"], "desc");
            assert!(request["params"].get("turnId").is_none());
        }
        if target == 200 {
            assert_eq!(requests[2]["params"]["cursor"], "global-older-1");
        }
    }
}

#[tokio::test]
async fn deleted_or_cyclic_search_hit_never_reports_a_successful_jump() {
    for cycle in [false, true] {
        let mut replies = vec![
            ("thread/read", json!({"thread":{"id":"thread"}})),
            ("thread/items/list", items(0, 1, cycle.then_some("repeat"))),
        ];
        if cycle {
            replies.push(("thread/items/list", items(1, 2, Some("repeat"))));
        }
        let (mut rpc, server) = peer(replies).await;
        let error = load_transcript_hit(&mut rpc, &hit("deleted-item"))
            .await
            .unwrap_err();
        assert!(error.to_string().contains(if cycle {
            "repeated cursor"
        } else {
            "not found"
        }));
        drop(rpc);
        server.await.unwrap();
    }
}
