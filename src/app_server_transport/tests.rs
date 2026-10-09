use super::*;
use crate::app_server_target::{
    AppServerConfig, AppServerTargetConfig, ResolvedAppServerEndpoint, ResolvedAppServerTarget,
};
use serde_json::json;
use tokio_tungstenite::accept_async;

#[tokio::test]
async fn stdio_reader_enforces_limit_before_unbounded_line_growth() {
    use tokio::io::{AsyncWriteExt, BufReader, duplex};

    let (client, mut server) = duplex(128);
    let writer = tokio::spawn(async move {
        server
            .write_all(b"{\"value\":\"abcdefghijklmnopqrstuvwxyz\"}\n")
            .await
            .unwrap();
    });
    let mut reader = BufReader::new(client);
    let error = read_bounded_line(&mut reader, 16)
        .await
        .expect_err("oversized line must fail");
    assert!(error.to_string().contains("16 byte limit"));
    writer.await.unwrap();
}

#[tokio::test]
async fn stdio_reader_allows_exact_limit_with_crlf() {
    use tokio::io::{AsyncWriteExt, BufReader, duplex};

    let (client, mut server) = duplex(128);
    let writer = tokio::spawn(async move {
        server.write_all(b"1234567890abcdef\r\n").await.unwrap();
    });
    let mut reader = BufReader::new(client);
    assert_eq!(
        read_bounded_line(&mut reader, 16).await.unwrap(),
        Some(b"1234567890abcdef".to_vec())
    );
    writer.await.unwrap();
}

#[tokio::test]
async fn stdio_reader_skips_blank_lines_and_decodes_json() {
    use tokio::io::{AsyncWriteExt, BufReader, duplex};

    let (client, mut server) = duplex(128);
    let writer = tokio::spawn(async move {
        server.write_all(b"\n  \r\n{\"id\":1}\n").await.unwrap();
    });
    let mut reader = BufReader::new(client);
    assert_eq!(
        read_stdio_json(&mut reader).await.unwrap(),
        Some(json!({"id":1}))
    );
    writer.await.unwrap();
}

#[test]
fn websocket_limits_are_explicit_and_defense_in_depth_check_matches() {
    let config = websocket_config();
    assert_eq!(config.max_message_size, Some(APP_SERVER_MAX_MESSAGE_BYTES));
    assert_eq!(config.max_frame_size, Some(APP_SERVER_MAX_FRAME_BYTES));
    ensure_message_size(
        APP_SERVER_MAX_MESSAGE_BYTES,
        APP_SERVER_MAX_MESSAGE_BYTES,
        "fixture",
    )
    .unwrap();
    assert!(
        ensure_message_size(
            APP_SERVER_MAX_MESSAGE_BYTES + 1,
            APP_SERVER_MAX_MESSAGE_BYTES,
            "fixture"
        )
        .unwrap_err()
        .to_string()
        .contains("byte limit")
    );
}

#[test]
fn remote_target_error_context_never_formats_bearer_token() {
    let mut config = AppServerConfig {
        active: "remote".into(),
        ..AppServerConfig::default()
    };
    config.targets.insert(
        "remote".into(),
        AppServerTargetConfig::Websocket {
            url: "wss://example.test/rpc?workspace=a".into(),
            auth_token_env: None,
        },
    );
    let target = ResolvedAppServerTarget::resolve(&config, None).expect("remote target");
    assert_eq!(target.diagnostic_endpoint(), "wss://example.test/rpc");
}

#[tokio::test]
async fn remote_websocket_handshake_has_a_bounded_connection_deadline() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stalled handshake peer");
    let address = listener.local_addr().expect("stalled peer address");
    let server = tokio::spawn(async move {
        let (_stream, _) = listener
            .accept()
            .await
            .expect("accept TCP without WS reply");
        tokio::time::sleep(Duration::from_millis(250)).await;
    });
    let target = ResolvedAppServerTarget {
        name: "stalled-websocket-fixture".into(),
        endpoint: ResolvedAppServerEndpoint::WebSocket {
            url: format!("ws://{address}/rpc"),
            auth_token: None,
        },
    };
    let error = AppServerTransport::connect_with_deadline(&target, Duration::from_millis(35))
        .await
        .err()
        .expect("stalled WebSocket upgrade must time out");
    assert!(error.to_string().contains("connect/handshake timed out"));
    assert!(!format!("{error:#}").contains("Bearer"));
    server.abort();
}

#[tokio::test]
async fn websocket_transport_round_trips_json_rpc_frames() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind websocket fixture");
    let address = listener.local_addr().expect("fixture address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept websocket");
        let mut websocket = accept_async(stream).await.expect("server handshake");
        let request = websocket
            .next()
            .await
            .expect("request frame")
            .expect("request");
        let Message::Text(text) = request else {
            panic!("expected text JSON-RPC frame");
        };
        let value: Value = serde_json::from_str(text.as_str()).expect("request json");
        assert_eq!(value["method"], "thread/list");
        websocket
            .send(Message::Text(
                serde_json::to_string(&json!({
                    "id": 1,
                    "result": {"data": []}
                }))
                .expect("response json")
                .into(),
            ))
            .await
            .expect("send response");
    });

    let target = ResolvedAppServerTarget {
        name: "fixture".into(),
        endpoint: ResolvedAppServerEndpoint::WebSocket {
            url: format!("ws://{address}/rpc"),
            auth_token: None,
        },
    };
    let mut transport = AppServerTransport::connect(&target)
        .await
        .expect("connect websocket target");
    transport
        .write_json(&json!({
            "id": 1,
            "method": "thread/list",
            "params": {}
        }))
        .await
        .expect("write request");
    let response = transport
        .read_json()
        .await
        .expect("read response")
        .expect("response");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["data"], json!([]));
    server.await.expect("fixture server");
}

#[tokio::test]
async fn websocket_notification_order_and_clean_close_are_observed() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("notification fixture");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("handshake");
        for sequence in [1, 2] {
            socket
                .send(Message::Text(
                    json!({"method": "thread/updated", "params": {"sequence": sequence}})
                        .to_string()
                        .into(),
                ))
                .await
                .expect("notification");
        }
        socket.close(None).await.expect("close fixture");
    });
    let target = ResolvedAppServerTarget {
        name: "notification-fixture".into(),
        endpoint: ResolvedAppServerEndpoint::WebSocket {
            url: format!("ws://{address}/rpc"),
            auth_token: None,
        },
    };
    let mut client = AppServerTransport::connect(&target).await.expect("connect");
    for expected in [1, 2] {
        let event = client
            .read_json()
            .await
            .expect("read event")
            .expect("event");
        assert_eq!(event["params"]["sequence"], expected);
    }
    assert!(client.read_json().await.expect("clean close").is_none());
    server.await.expect("server");
}

#[tokio::test]
async fn malformed_websocket_event_is_rejected_without_echoing_payload() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("malformed fixture");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("handshake");
        socket
            .send(Message::Text("INVALID_SECRET_VALUE_not_json".into()))
            .await
            .expect("send invalid event");
    });
    let target = ResolvedAppServerTarget {
        name: "bad-json-fixture".into(),
        endpoint: ResolvedAppServerEndpoint::WebSocket {
            url: format!("ws://{address}/rpc"),
            auth_token: None,
        },
    };
    let mut client = AppServerTransport::connect(&target).await.expect("connect");
    let err = client
        .read_json()
        .await
        .expect_err("bad JSON must not become a frame");
    let safe = format!("{err:#}");
    assert!(safe.contains("decode App Server WebSocket JSON"));
    assert!(!safe.contains("INVALID_SECRET_VALUE_not_json"));
    server.await.expect("server");
}

#[cfg(unix)]
#[tokio::test]
async fn unix_socket_transport_uses_websocket_framing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("app-server.sock");
    let listener = tokio::net::UnixListener::bind(&path).expect("bind unix fixture");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept unix socket");
        let mut websocket = accept_async(stream)
            .await
            .expect("unix websocket handshake");
        let request = websocket
            .next()
            .await
            .expect("request frame")
            .expect("request");
        let Message::Text(text) = request else {
            panic!("expected text JSON-RPC frame");
        };
        let value: Value = serde_json::from_str(text.as_str()).expect("request json");
        assert_eq!(value["method"], "initialize");
        websocket
            .send(Message::Text(
                serde_json::to_string(&json!({
                    "id": 1,
                    "result": {"serverInfo": {"version": "fixture"}}
                }))
                .expect("response json")
                .into(),
            ))
            .await
            .expect("send response");
    });

    let target = ResolvedAppServerTarget {
        name: "unix-fixture".into(),
        endpoint: ResolvedAppServerEndpoint::UnixSocket { path },
    };
    let mut transport = AppServerTransport::connect(&target)
        .await
        .expect("connect unix target");
    transport
        .write_json(&json!({
            "id": 1,
            "method": "initialize",
            "params": {}
        }))
        .await
        .expect("write initialize");
    let response = transport
        .read_json()
        .await
        .expect("read response")
        .expect("response");
    assert_eq!(response["result"]["serverInfo"]["version"], "fixture");
    server.await.expect("fixture server");
}
