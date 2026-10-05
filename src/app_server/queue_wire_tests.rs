//! Local WebSocket peer drives the real RpcSession and queue pagination path.
use super::*;
use crate::app_server_target::ResolvedAppServerEndpoint;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{accept_async, tungstenite::Message};

async fn replay(pages: Vec<Value>) -> (Result<ThreadQueueSnapshot>, Vec<Value>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut stream = accept_async(socket).await.unwrap();
            let mut requests = vec![];
            for page in pages {
                let Some(Ok(message)) = stream.next().await else {
                    break;
                };
                let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                assert_eq!(request["method"], "thread/queue/list");
                let response = json!({"id":request["id"],"result":page});
                requests.push(request);
                stream
                    .send(Message::Text(response.to_string().into()))
                    .await
                    .unwrap();
            }
            requests
        });
        let target = ResolvedAppServerTarget {
            name: "queue-wire-fixture".into(),
            endpoint: ResolvedAppServerEndpoint::WebSocket {
                url: format!("ws://{address}"),
                auth_token: None,
            },
        };
        let mut rpc = RpcSession::connect(&target).await.unwrap();
        let result = load_thread_queue(&mut rpc, ThreadId::new("thread")).await;
        drop(rpc);
        (result, server.await.unwrap())
    })
    .await
    .expect("bounded local queue replay")
}
fn page(id: &str, cursor: Option<&str>) -> Value {
    json!({"data":[{"id":id, "clientUserMessageId":format!("client-{id}"),
        "input":[{"type":"text","text":id,"textElements":[]}]}],"nextCursor":cursor})
}
#[tokio::test]
async fn duplicate_identity_across_queue_pages_is_rejected() {
    let (result, requests) = replay(vec![page("same", Some("next")), page("same", None)]).await;
    assert!(
        result.is_err(),
        "duplicate queue IDs escaped full pagination"
    );
    assert_eq!(requests.len(), 2);
}
#[tokio::test]
async fn cycling_cursor_is_rejected_before_requesting_another_page() {
    let (result, requests) = replay(vec![
        page("a", Some("cycle")),
        page("b", Some("cycle")),
        page("c", None),
    ])
    .await;
    assert!(
        result.is_err(),
        "cycling pagination was treated as a complete queue"
    );
    assert_eq!(requests.len(), 2);
}
#[tokio::test]
async fn unique_multipage_queue_retains_upstream_order_and_ids() {
    let (result, requests) = replay(vec![page("a", Some("next")), page("b", None)]).await;
    let snapshot = result.unwrap();
    assert_eq!(
        snapshot
            .submissions
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert!(snapshot.next_cursor.is_none());
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["params"]["cursor"], Value::Null);
    assert_eq!(requests[1]["params"]["cursor"], "next");
    assert_eq!(requests[1]["params"]["limit"], THREAD_QUEUE_PAGE_LIMIT);
}
