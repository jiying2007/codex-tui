use crate::app_server_target::{ResolvedAppServerEndpoint, ResolvedAppServerTarget};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
#[cfg(unix)]
use tokio_tungstenite::client_async_with_config;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_with_config,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{HeaderValue, header::AUTHORIZATION},
        protocol::WebSocketConfig,
    },
};

enum Transport {
    Stdio {
        _child: Box<Child>,
        reader: Box<BufReader<ChildStdout>>,
        writer: Box<BufWriter<ChildStdin>>,
    },
    WebSocket {
        stream: Box<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    },
    #[cfg(unix)]
    UnixSocket {
        stream: Box<WebSocketStream<UnixStream>>,
    },
}

pub struct AppServerTransport {
    inner: Transport,
}

const APP_SERVER_MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const APP_SERVER_MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

fn websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(APP_SERVER_MAX_MESSAGE_BYTES))
        .max_frame_size(Some(APP_SERVER_MAX_FRAME_BYTES))
}

impl AppServerTransport {
    pub async fn connect(target: &ResolvedAppServerTarget) -> Result<Self> {
        let inner = match &target.endpoint {
            ResolvedAppServerEndpoint::Stdio { codex_bin } => {
                let mut command = Command::new(codex_bin.clone().unwrap_or_else(|| "codex".into()));
                command
                    .args(["app-server", "--listen", "stdio://"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .kill_on_drop(true);
                let mut child = command.spawn().with_context(|| {
                    format!("spawn local Codex App Server target {:?}", target.name)
                })?;
                let stdin = child
                    .stdin
                    .take()
                    .context("Codex App Server stdin unavailable")?;
                let stdout = child
                    .stdout
                    .take()
                    .context("Codex App Server stdout unavailable")?;
                if let Some(mut stderr) = child.stderr.take() {
                    tokio::spawn(async move {
                        let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
                    });
                }
                Transport::Stdio {
                    _child: Box::new(child),
                    reader: Box::new(BufReader::new(stdout)),
                    writer: Box::new(BufWriter::new(stdin)),
                }
            }
            ResolvedAppServerEndpoint::WebSocket { url, auth_token } => {
                let mut request = url
                    .as_str()
                    .into_client_request()
                    .context("build App Server WebSocket request")?;
                if let Some(token) = auth_token {
                    let value = HeaderValue::from_str(&format!("Bearer {token}"))
                        .context("encode App Server bearer authorization header")?;
                    request.headers_mut().insert(AUTHORIZATION, value);
                }
                let (stream, _) =
                    connect_async_with_config(request, Some(websocket_config()), false)
                        .await
                        .with_context(|| {
                            format!(
                                "connect App Server target {:?} at {}",
                                target.name,
                                target.diagnostic_endpoint()
                            )
                        })?;
                Transport::WebSocket {
                    stream: Box::new(stream),
                }
            }
            ResolvedAppServerEndpoint::UnixSocket { path } => {
                #[cfg(unix)]
                {
                    let socket = UnixStream::connect(path).await.with_context(|| {
                        format!("connect App Server Unix socket {}", path.display())
                    })?;
                    let (stream, _) = client_async_with_config(
                        "ws://localhost/rpc",
                        socket,
                        Some(websocket_config()),
                    )
                    .await
                    .context("perform App Server Unix-socket WebSocket handshake")?;
                    Transport::UnixSocket {
                        stream: Box::new(stream),
                    }
                }
                #[cfg(not(unix))]
                {
                    anyhow::bail!(
                        "Unix App Server targets are unsupported on this platform: {}",
                        path.display()
                    );
                }
            }
        };

        Ok(Self { inner })
    }

    pub async fn read_json(&mut self) -> Result<Option<Value>> {
        match &mut self.inner {
            Transport::Stdio { reader, .. } => read_stdio_json(reader.as_mut()).await,
            Transport::WebSocket { stream } => read_websocket_json(stream.as_mut()).await,
            #[cfg(unix)]
            Transport::UnixSocket { stream } => read_websocket_json(stream.as_mut()).await,
        }
    }

    pub async fn write_json(&mut self, value: &Value) -> Result<()> {
        match &mut self.inner {
            Transport::Stdio { writer, .. } => {
                let mut encoded =
                    serde_json::to_vec(value).context("encode App Server stdio JSON")?;
                encoded.push(b'\n');
                writer
                    .write_all(&encoded)
                    .await
                    .context("write App Server stdio")?;
                writer.flush().await.context("flush App Server stdio")
            }
            Transport::WebSocket { stream } => write_websocket_json(stream.as_mut(), value).await,
            #[cfg(unix)]
            Transport::UnixSocket { stream } => write_websocket_json(stream.as_mut(), value).await,
        }
    }
}

async fn read_stdio_json<R>(reader: &mut R) -> Result<Option<Value>>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let Some(bytes) = read_bounded_line(reader, APP_SERVER_MAX_MESSAGE_BYTES).await? else {
            return Ok(None);
        };
        if bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        return serde_json::from_slice(&bytes)
            .context("decode App Server stdio JSON")
            .map(Some);
    }
}

async fn read_bounded_line<R>(reader: &mut R, limit: usize) -> Result<Option<Vec<u8>>>
where
    R: AsyncBufRead + Unpin,
{
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf().await.context("read App Server stdio")?;
        if available.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Ok(Some(bytes))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        anyhow::ensure!(
            bytes.len().saturating_add(take) <= limit.saturating_add(1),
            "App Server stdio JSON exceeds {limit} byte limit"
        );
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            while matches!(bytes.last(), Some(b'\n' | b'\r')) {
                bytes.pop();
            }
            return Ok(Some(bytes));
        }
    }
}

fn ensure_message_size(bytes: usize, limit: usize, transport: &str) -> Result<()> {
    anyhow::ensure!(
        bytes <= limit,
        "App Server {transport} JSON exceeds {limit} byte limit"
    );
    Ok(())
}

async fn read_websocket_json<S>(stream: &mut WebSocketStream<S>) -> Result<Option<Value>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        let Some(frame) = stream.next().await else {
            return Ok(None);
        };
        let frame = frame.context("read App Server WebSocket frame")?;
        match frame {
            Message::Text(text) => {
                ensure_message_size(text.len(), APP_SERVER_MAX_MESSAGE_BYTES, "WebSocket text")?;
                return serde_json::from_str(text.as_str())
                    .context("decode App Server WebSocket JSON")
                    .map(Some);
            }
            Message::Binary(bytes) => {
                ensure_message_size(
                    bytes.len(),
                    APP_SERVER_MAX_MESSAGE_BYTES,
                    "WebSocket binary",
                )?;
                return serde_json::from_slice(bytes.as_ref())
                    .context("decode App Server WebSocket binary JSON")
                    .map(Some);
            }
            Message::Close(_) => return Ok(None),
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        }
    }
}

async fn write_websocket_json<S>(stream: &mut WebSocketStream<S>, value: &Value) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let text = serde_json::to_string(value).context("encode App Server WebSocket JSON")?;
    stream
        .send(Message::Text(text.into()))
        .await
        .context("write App Server WebSocket JSON")
}

#[cfg(test)]
mod tests {
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
}
