use crate::app_server_target::{ResolvedAppServerEndpoint, ResolvedAppServerTarget};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines};
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
#[cfg(unix)]
use tokio_tungstenite::client_async;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{HeaderValue, header::AUTHORIZATION},
    },
};

enum Transport {
    Stdio {
        _child: Box<Child>,
        reader: Box<Lines<BufReader<ChildStdout>>>,
        writer: Box<BufWriter<ChildStdin>>,
    },
    WebSocket {
        stream: Box<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    },
    #[cfg(unix)]
    UnixSocket { stream: WebSocketStream<UnixStream> },
}

pub struct AppServerTransport {
    inner: Transport,
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
                if let Some(stderr) = child.stderr.take() {
                    tokio::spawn(async move {
                        let mut lines = BufReader::new(stderr).lines();
                        while let Ok(Some(_line)) = lines.next_line().await {}
                    });
                }
                Transport::Stdio {
                    _child: Box::new(child),
                    reader: Box::new(BufReader::new(stdout).lines()),
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
                let (stream, _) = connect_async(request).await.with_context(|| {
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
                    let (stream, _) = client_async("ws://localhost/rpc", socket)
                        .await
                        .context("perform App Server Unix-socket WebSocket handshake")?;
                    Transport::UnixSocket { stream }
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
            Transport::Stdio { reader, .. } => loop {
                let Some(line) = reader.next_line().await.context("read App Server stdio")? else {
                    return Ok(None);
                };
                if line.trim().is_empty() {
                    continue;
                }
                return serde_json::from_str(&line)
                    .context("decode App Server stdio JSON")
                    .map(Some);
            },
            Transport::WebSocket { stream } => read_websocket_json(stream.as_mut()).await,
            #[cfg(unix)]
            Transport::UnixSocket { stream } => read_websocket_json(stream).await,
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
            Transport::UnixSocket { stream } => write_websocket_json(stream, value).await,
        }
    }
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
                return serde_json::from_str(text.as_str())
                    .context("decode App Server WebSocket JSON")
                    .map(Some);
            }
            Message::Binary(bytes) => {
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
