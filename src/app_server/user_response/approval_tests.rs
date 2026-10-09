use super::*;
use crate::backend::{CodexBackend, FakeBackend};

fn fixture(
    method: &str,
    resolution: InteractiveResolution,
) -> (
    BTreeMap<RpcRequestId, PendingServerRequest>,
    BTreeMap<String, ThreadSummary>,
    UserResponseSubmission,
) {
    let threads = by_id(FakeBackend::seeded().snapshot().threads);
    let params = json!({"threadId":threads.keys().next().unwrap(),"turnId":"t","itemId":"i",
        "command":"echo test","permissions":{"network":{"enabled":true}},
        "questions":[{"id":"q","question":"Q?"}]});
    let request = parse_interactive_request(&json!({"id":17,"method":method,"params":params}))
        .unwrap()
        .unwrap();
    let pending = BTreeMap::from([(
        request.request_id.clone(),
        PendingServerRequest {
            method: method.into(),
            params,
        },
    )]);
    (
        pending,
        threads,
        UserResponseSubmission {
            ticket: 8,
            request,
            resolution,
        },
    )
}
const METHODS: [&str; 4] = [
    "item/commandExecution/requestApproval",
    "item/fileChange/requestApproval",
    "item/permissions/requestApproval",
    "item/tool/requestUserInput",
];
#[tokio::test]
async fn all_decisions_keep_existing_wire_shape_and_write_once() {
    for method in METHODS {
        for resolution in [
            InteractiveResolution::Accept,
            InteractiveResolution::Decline,
            InteractiveResolution::Cancel,
        ] {
            if method == METHODS[3] && resolution == InteractiveResolution::Accept {
                continue;
            }
            let (mut pending, threads, s) = fixture(method, resolution.clone());
            let expected = match (method, resolution) {
                (m, _) if m == METHODS[3] => {
                    json!({"id":17,"error":{"code":-32000,"message":"user input cancelled by user"}})
                }
                (m, InteractiveResolution::Accept) if m == METHODS[2] => {
                    json!({"id":17,"result":{"permissions":{"network":{"enabled":true}},"scope":"turn"}})
                }
                (m, _) if m == METHODS[2] => {
                    json!({"id":17,"result":{"permissions":{},"scope":"turn"}})
                }
                (_, r) => {
                    json!({"id":17,"result":{"decision":match r {InteractiveResolution::Accept=>"accept", InteractiveResolution::Decline=>"decline", _=>"cancel"}}})
                }
            };
            assert_eq!(
                send_with(
                    &mut pending,
                    &threads,
                    s.clone(),
                    Duration::from_secs(1),
                    |wire| async move {
                        assert_eq!(wire, expected);
                        Ok(())
                    }
                )
                .await,
                UserResponseOutcome::Written
            );
            assert!(pending.is_empty());
            assert!(matches!(
                send_with(
                    &mut pending,
                    &threads,
                    s,
                    Duration::from_secs(1),
                    |_| async { panic!("duplicate write") }
                )
                .await,
                UserResponseOutcome::NotSent(_)
            ));
        }
    }
}
#[tokio::test]
async fn changed_approval_context_is_refused_before_any_write() {
    for (method, field, value) in [
        (METHODS[0], "additionalPermissions", json!({"network":true})),
        (METHODS[1], "grantRoot", json!("/different")),
        (
            METHODS[2],
            "permissions",
            json!({"network":{"enabled":true,"host":"other"}}),
        ),
    ] {
        let (mut pending, threads, s) = fixture(method, InteractiveResolution::Accept);
        pending.get_mut(&s.request.request_id).unwrap().params[field] = value;
        assert!(matches!(
            send_with(
                &mut pending,
                &threads,
                s,
                Duration::from_secs(1),
                |_| async { panic!("changed grant") }
            )
            .await,
            UserResponseOutcome::NotSent(_)
        ));
        assert_eq!(pending.len(), 1);
    }
}
#[tokio::test]
async fn approval_write_errors_and_deadlines_never_restore_retryability() {
    for method in METHODS {
        for stall in [false, true] {
            let (mut pending, threads, s) = fixture(method, InteractiveResolution::Cancel);
            let outcome = send_with(
                &mut pending,
                &threads,
                s.clone(),
                Duration::from_millis(10),
                |_| async move {
                    if stall {
                        std::future::pending::<()>().await;
                    }
                    anyhow::bail!("injected partial write");
                },
            )
            .await;
            assert!(matches!(outcome, UserResponseOutcome::Unknown(_)));
            assert!(pending.is_empty());
            assert!(matches!(
                send_with(
                    &mut pending,
                    &threads,
                    s,
                    Duration::from_secs(1),
                    |_| async { panic!("retry") }
                )
                .await,
                UserResponseOutcome::NotSent(_)
            ));
        }
    }
}
#[tokio::test]
async fn invalid_resolution_and_missing_thread_leave_request_unconsumed() {
    for removed in [false, true] {
        let (mut pending, mut threads, s) = fixture(METHODS[3], InteractiveResolution::Accept);
        if removed {
            threads.clear();
        }
        assert!(matches!(
            send_with(
                &mut pending,
                &threads,
                s,
                Duration::from_secs(1),
                |_| async { panic!("invalid send") }
            )
            .await,
            UserResponseOutcome::NotSent(_)
        ));
        assert_eq!(pending.len(), 1);
    }
}
#[test]
fn approval_context_debug_never_exposes_wire_extensions() {
    let (_, _, s) = fixture(METHODS[0], InteractiveResolution::Accept);
    if let crate::conversation::InteractiveRequestKind::CommandApproval { context, .. } =
        &s.request.kind
    {
        assert!(!format!("{context:?}").contains("echo test"));
        assert!(format!("{context:?}").contains("redacted"));
    } else {
        panic!("command");
    }
}
#[tokio::test]
async fn production_actor_stops_after_actual_websocket_write_failure() {
    use crate::app_server_target::{ResolvedAppServerEndpoint, ResolvedAppServerTarget};
    use futures_util::SinkExt;
    use tokio_tungstenite::{accept_async, tungstenite::Message};
    tokio::time::timeout(Duration::from_secs(5), async {
        let (pending, threads, s) = fixture(METHODS[0], InteractiveResolution::Accept);
        let p = pending.values().next().unwrap();
        let wire = json!({"id":17,"method":p.method,"params":p.params});
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            ws.send(Message::Text(wire.to_string().into()))
                .await
                .unwrap();
            ws.close(None).await.unwrap();
        });
        let target = ResolvedAppServerTarget {
            name: "approval-fixture".into(),
            endpoint: ResolvedAppServerEndpoint::WebSocket {
                url: format!("ws://{address}"),
                auth_token: None,
            },
        };
        let mut rpc = RpcSession::connect(&target).await.unwrap();
        let request = rpc.read_wire_message().await.unwrap().unwrap();
        assert!(rpc.read_wire_message().await.unwrap().is_none());
        peer.await.unwrap();
        // The real transport has consumed Close. Keep the observed request queued,
        // making SendAfterClosing deterministic rather than relying on TCP timing.
        let mut second = request.clone();
        second["id"] = json!(18);
        rpc.queued_messages.extend([request, second]);
        let initial = FakeBackend::seeded().snapshot();
        let (tx, rx) = watch::channel(initial.clone());
        let (events, mut received) = mpsc::channel(1);
        let (commands, command_rx) = mpsc::channel(8);
        let actor = tokio::spawn(run_registry_actor(
            rpc,
            threads.into_values().collect(),
            initial.status,
            tx,
            events,
            command_rx,
            (None, target),
        ));
        assert!(matches!(
            received.recv().await,
            Some(ConversationEvent::InteractiveRequested(_))
        ));
        commands
            .send(BackendCommand::SubmitUserResponse(s))
            .await
            .unwrap();
        // The second request backpressures the single-threaded actor until the
        // response command is queued; no sleeps or scheduler-speed assumptions.
        assert!(matches!(
            received.recv().await,
            Some(ConversationEvent::InteractiveRequested(_))
        ));
        assert!(matches!(
            received.recv().await,
            Some(ConversationEvent::UserResponse {
                outcome: UserResponseOutcome::Unknown(_),
                ..
            })
        ));
        actor.await.unwrap();
        assert!(!rx.borrow().status.connected);
        assert!(commands.is_closed());
    })
    .await
    .expect("actor failure must be bounded");
}
