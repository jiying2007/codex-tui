//! The real actor must send an interrupt even when a Queue page never responds.
use super::*;
use crate::app_server_target::ResolvedAppServerEndpoint;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[tokio::test]
async fn stalled_queue_page_never_blocks_interrupt_dispatch() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (queue_tx, queue_rx) = tokio::sync::oneshot::channel();
    let (interrupt_tx, interrupt_rx) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        let mut queue_tx = Some(queue_tx);
        let mut interrupt_tx = Some(interrupt_tx);
        while let Some(Ok(Message::Text(frame))) = socket.next().await {
            let request: Value = serde_json::from_str(&frame).unwrap();
            let Some(method) = request.get("method").and_then(Value::as_str) else {
                continue;
            };
            let Some(id) = request.get("id") else {
                continue;
            };
            let result = match method {
                "initialize" => json!({"serverInfo":{"name":"queue-priority-fixture","version":"0.169"}}),
                "thread/list" => json!({"data":[{"id":"one","name":"test","cwd":"/tmp","status":{"type":"idle"}}],"nextCursor":null}),
                "thread/loaded/list" => json!({"data":["one"],"nextCursor":null}),
                "thread/resume" => json!({"thread":{"id":"one","name":"test","cwd":"/tmp","status":{"type":"idle"}}}),
                "thread/goal/get" => json!({"goal":null}),
                "thread/queue/list" => {
                    // The server keeps this page indefinitely; the client must
                    // cancel the read future and dispatch the control request.
                    if let Some(sender) = queue_tx.take() {
                        let _ = sender.send(());
                    }
                    continue;
                }
                "turn/interrupt" => {
                    if let Some(sender) = interrupt_tx.take() {
                        let _ = sender.send(());
                    }
                    json!({})
                }
                _ => json!({}),
            };
            socket.send(Message::Text(json!({"id":id,"result":result}).to_string().into()))
                .await.unwrap();
            if method == "turn/interrupt" {
                return;
            }
        }
    });
    let target = ResolvedAppServerTarget {
        name: "slow-queue-priority".into(),
        endpoint: ResolvedAppServerEndpoint::WebSocket {
            url: format!("ws://{address}"),
            auth_token: None,
        },
    };
    let started = tokio::time::timeout(Duration::from_secs(3), start_target(target))
        .await.unwrap().unwrap();
    let id = ThreadId::new("one");
    started.handle.refresh_thread_queue(id.clone()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), queue_rx)
        .await.expect("queue read not started").unwrap();
    started.handle.interrupt_turn(id, "turn-1".into()).unwrap();
    tokio::time::timeout(Duration::from_millis(800), interrupt_rx)
        .await.expect("interrupt stuck behind queue page").unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(1), peer).await;
}
