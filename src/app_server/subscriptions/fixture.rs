//! A connection-scoped peer: reading/listing/starting a turn never subscribes.
//! Mirrors the 0.162.1 / 0.163.0 thread_lifecycle + thread_processor contract.
use super::*;
use crate::app_server_target::ResolvedAppServerEndpoint;
use futures_util::{SinkExt, StreamExt};
use std::sync::{Arc, Mutex};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::oneshot,
};
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

pub(super) async fn receive(socket: &mut WebSocketStream<TcpStream>) -> Value {
    let frame = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .expect("peer request deadline")
        .expect("peer frame")
        .expect("peer wire");
    serde_json::from_str(frame.to_text().expect("JSON frame")).unwrap()
}

pub(super) async fn emit(socket: &mut WebSocketStream<TcpStream>, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

pub(super) async fn listener() -> (ResolvedAppServerTarget, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = ResolvedAppServerTarget {
        name: "connection-subscription-fixture".into(),
        endpoint: ResolvedAppServerEndpoint::WebSocket {
            url: format!("ws://{}", listener.local_addr().unwrap()),
            auth_token: None,
        },
    };
    (target, listener)
}

enum Control {
    Event(Value, oneshot::Sender<bool>),
    Hold(
        String,
        oneshot::Sender<()>,
        oneshot::Receiver<()>,
        oneshot::Sender<()>,
    ),
}

pub(super) struct Peer {
    pub target: ResolvedAppServerTarget,
    records: Arc<Mutex<Vec<Value>>>,
    subscribed: Arc<Mutex<BTreeSet<String>>>,
    control: mpsc::Sender<Control>,
    task: JoinHandle<()>,
}

impl Peer {
    pub async fn start() -> Self {
        let (target, listener) = listener().await;
        let records = Arc::new(Mutex::new(Vec::new()));
        let subscribed = Arc::new(Mutex::new(BTreeSet::new()));
        let (control, mut commands) = mpsc::channel(8);
        let log = records.clone();
        let subscriptions = subscribed.clone();
        let task = tokio::spawn(async move {
            let mut socket = accept_async(listener.accept().await.unwrap().0)
                .await
                .unwrap();
            let mut held = None;
            loop {
                tokio::select! {
                    biased;
                    command = commands.recv() => match command {
                        Some(Control::Event(value, done)) => {
                            if matches!(value.get("method").and_then(Value::as_str), Some("thread/archived" | "thread/deleted"))
                                && let Some(id) = value.pointer("/params/threadId").and_then(Value::as_str)
                            {
                                subscriptions.lock().unwrap().remove(id);
                            }
                            let scoped = value.get("id").is_some() || matches!(
                                value.get("method").and_then(Value::as_str),
                                Some("item/completed" | "turn/started" | "turn/completed" | "thread/queue/changed" | "serverRequest/resolved")
                            );
                            let delivered = !scoped || value.pointer("/params/threadId").and_then(Value::as_str)
                                .is_some_and(|id| subscriptions.lock().unwrap().contains(id));
                            if delivered { emit(&mut socket, value).await; }
                            let _ = done.send(delivered);
                        }
                        Some(Control::Hold(method, observed, release, ready)) => {
                            assert!(held.is_none());
                            held = Some((method, observed, release));
                            let _ = ready.send(());
                        }
                        None => return,
                    },
                    frame = socket.next() => {
                        let Some(Ok(Message::Text(frame))) = frame else { return; };
                        let request: Value = serde_json::from_str(&frame).unwrap();
                        log.lock().unwrap().push(request.clone());
                        let Some(method) = request.get("method").and_then(Value::as_str) else { continue; };
                        if request.get("id").is_none() { continue; }
                        let id = request.pointer("/params/threadId").and_then(Value::as_str).unwrap_or("");
                        let result = match method {
                            "initialize" => json!({"serverInfo":{"name":"subscription-fixture","version":"0.163.0"}}),
                            "thread/list" => json!({"data":[thread("one"),thread("other")],"nextCursor":null}),
                            "thread/loaded/list" => json!({"data":["one","other"],"nextCursor":null}),
                            "thread/resume" => {
                                assert_eq!(request["params"], json!({"threadId":id,"excludeTurns":true}));
                                subscriptions.lock().unwrap().insert(id.into());
                                json!({"thread":thread(id)})
                            }
                            "thread/unsubscribe" => {
                                let existed = subscriptions.lock().unwrap().remove(id);
                                json!({"status":if existed {"unsubscribed"} else {"notSubscribed"}})
                            }
                            "thread/read" => json!({"thread":thread(id)}),
                            "thread/turns/list" => json!({"data":[{"id":"turn-1","status":"completed"}],"nextCursor":null}),
                            "thread/items/list" => json!({"data":[{"turnId":"turn-1","item":{"id":"item-1","type":"agentMessage","text":"subscribed history"}}],"nextCursor":null}),
                            "thread/queue/list" => json!({"data":[],"nextCursor":null}),
                            "thread/goal/get" => json!({"goal":null}),
                            "turn/start" => {
                                assert!(subscriptions.lock().unwrap().contains(id), "turn/start does not grant event delivery");
                                json!({"turn":{"id":"turn-1"}})
                            }
                            _ => panic!("unexpected fixture request {request}"),
                        };
                        if held.as_ref().is_some_and(|(method, _, _)| method == request["method"].as_str().unwrap()) {
                            let (_, observed, release) = held.take().unwrap();
                            let _ = observed.send(());
                            release.await.expect("release held acknowledgement");
                        }
                        emit(&mut socket, json!({"id":request["id"],"result":result})).await;
                    }
                }
            }
        });
        Self {
            target,
            records,
            subscribed,
            control,
            task,
        }
    }

    pub fn count(&self, method: &str) -> usize {
        self.records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == method)
            .count()
    }

    pub fn subscriptions(&self) -> BTreeSet<String> {
        self.subscribed.lock().unwrap().clone()
    }

    pub async fn wait_count(&self, method: &str, expected: usize) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.count(method) < expected {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "expected {expected} {method}, records {:?}",
                self.records.lock().unwrap()
            )
        });
    }

    pub async fn event(&self, message: Value) -> bool {
        let (done, received) = oneshot::channel();
        self.control
            .send(Control::Event(message, done))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), received)
            .await
            .unwrap()
            .unwrap()
    }

    pub async fn hold(&self, method: &str) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (observed, observed_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let (ready, ready_rx) = oneshot::channel();
        self.control
            .send(Control::Hold(method.into(), observed, release_rx, ready))
            .await
            .unwrap();
        ready_rx.await.unwrap();
        (observed_rx, release)
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn thread(id: &str) -> Value {
    json!({"id":id,"name":id,"cwd":"/fixture","status":{"type":"idle"}})
}

pub(super) async fn event(
    handle: &mut RegistryHandle,
    predicate: impl Fn(&ConversationEvent) -> bool,
) -> ConversationEvent {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event = handle.conversation_rx.recv().await.expect("live actor");
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .expect("conversation event deadline")
}

pub(super) async fn barrier(handle: &mut RegistryHandle) {
    handle.refresh_goal(ThreadId::new("one")).unwrap();
    event(handle, |event| {
        matches!(event, ConversationEvent::GoalCleared(_))
    })
    .await;
}
