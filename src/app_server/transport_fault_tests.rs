//! Faults of a real local wire are different from valid RPC application errors.
use super::*;
use crate::app_server_target::ResolvedAppServerEndpoint;
use futures_util::{SinkExt, StreamExt};
use tokio::{net::{TcpListener, TcpStream}, sync::oneshot};
use tokio_tungstenite::{accept_async, WebSocketStream, tungstenite::Message};

async fn listener() -> (ResolvedAppServerTarget, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = ResolvedAppServerTarget {
        name: "transport-fault-fixture".into(),
        endpoint: ResolvedAppServerEndpoint::WebSocket {
            url: format!("ws://{}", listener.local_addr().unwrap()), auth_token: None,
        },
    };
    (target, listener)
}
async fn receive(socket: &mut WebSocketStream<TcpStream>) -> Value {
    let frame = tokio::time::timeout(Duration::from_secs(3), socket.next()).await.unwrap().unwrap().unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}
async fn emit(socket: &mut WebSocketStream<TcpStream>, value: Value) {
    socket.send(Message::Text(value.to_string().into())).await.unwrap();
}

#[tokio::test]
async fn malformed_or_closed_wire_during_rpc_cannot_recover_or_send_again() {
    for closed in [false, true] {
        let (target, listener) = listener().await;
        let task = tokio::spawn(async move {
            let mut socket = accept_async(listener.accept().await.unwrap().0).await.unwrap();
            let request = receive(&mut socket).await;
            if closed {
                socket.close(None).await.unwrap();
            } else {
                emit(&mut socket, json!({"method":"thread/name/updated","params":{"threadId":"one","threadName":"queued"}})).await;
                socket.send(Message::Text("{invalid-json}".into())).await.unwrap();
                // Even a later valid response cannot repair a corrupted wire.
                emit(&mut socket, json!({"id":request["id"],"result":{}})).await;
            }
            if let Ok(Some(Ok(frame))) = tokio::time::timeout(Duration::from_secs(3), socket.next()).await {
                assert!(!frame.is_text() && !frame.is_binary(), "post-fault bytes escaped the fence");
            }
        });
        let mut rpc = RpcSession::connect(&target).await.unwrap();
        assert!(rpc.request("fixture/read", json!({})).await.is_err());
        assert!(rpc.transport.is_quarantined());
        assert!(rpc.has_queued_messages(), "fault must take precedence over new actor commands");
        assert!(rpc.notify("fixture/must-not-send", None).await.is_err());
        assert!(matches!(rpc.read_message().now_or_never(), Some(Err(_))));
        drop(rpc);
        task.await.unwrap();
    }
}

#[tokio::test]
async fn valid_rpc_rejection_does_not_quarantine_a_healthy_connection() {
    let (target, listener) = listener().await;
    let task = tokio::spawn(async move {
        let mut socket = accept_async(listener.accept().await.unwrap().0).await.unwrap();
        let first = receive(&mut socket).await;
        emit(&mut socket, json!({"id":first["id"],"error":{"code":-32601,"message":"optional method unavailable"}})).await;
        let second = receive(&mut socket).await;
        emit(&mut socket, json!({"id":second["id"],"result":{"ok":true}})).await;
    });
    let mut rpc = RpcSession::connect(&target).await.unwrap();
    let error = rpc.request("fixture/optional", json!({})).await.unwrap_err();
    assert_eq!(error.downcast_ref::<RpcResponseError>().unwrap().code, Some(-32601));
    assert!(!rpc.transport.is_quarantined());
    assert_eq!(rpc.request("fixture/read", json!({})).await.unwrap(), json!({"ok":true}));
    task.await.unwrap();
}

#[tokio::test]
async fn cancelled_response_wait_preserves_connection_and_ignores_late_response() {
    let (target, listener) = listener().await;
    let (sent, received) = oneshot::channel();
    let task = tokio::spawn(async move {
        let mut socket = accept_async(listener.accept().await.unwrap().0).await.unwrap();
        let first = receive(&mut socket).await;
        sent.send(()).unwrap();
        let second = receive(&mut socket).await;
        assert_eq!(second["method"], "fixture/new-read");
        emit(&mut socket, json!({"id":first["id"],"result":{"late":true}})).await;
        emit(&mut socket, json!({"id":second["id"],"result":{"current":true}})).await;
    });
    let mut rpc = RpcSession::connect(&target).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::select! {
            biased;
            _ = rpc.request_without_timeout("fixture/old-read", json!({})) => panic!("peer has not answered"),
            signal = received => signal.unwrap(),
        }
    }).await.unwrap();
    assert!(!rpc.transport.is_quarantined(), "only the response wait was cancelled");
    assert_eq!(rpc.request("fixture/new-read", json!({})).await.unwrap(), json!({"current":true}));
    task.await.unwrap();
}

#[tokio::test]
async fn actor_publishes_disconnected_after_fault_inside_loaded_rpc() {
    let (target, listener) = listener().await;
    let task = tokio::spawn(async move {
        let mut socket = accept_async(listener.accept().await.unwrap().0).await.unwrap();
        let init = receive(&mut socket).await;
        emit(&mut socket, json!({"id":init["id"],"result":{}})).await;
        assert_eq!(receive(&mut socket).await["method"], "initialized");
        let list = receive(&mut socket).await;
        assert_eq!(list["method"], "thread/list");
        emit(&mut socket, json!({"id":list["id"],"result":{"data":[{"id":"one"}],"nextCursor":null}})).await;
        let loaded = receive(&mut socket).await;
        assert_eq!(loaded["method"], "thread/loaded/list");
        socket.send(Message::Text("{invalid-json}".into())).await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(3), socket.next()).await;
    });
    let mut started = start_target(target).await.unwrap();
    let last = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(snapshot) = started.handle.try_recv() {
                if !snapshot.status.connected { break snapshot; }
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }).await.unwrap();
    assert!(last.status.error.unwrap().contains("quarantined"));
    assert_eq!(last.threads.len(), 1);
    drop(started);
    task.await.unwrap();
}
