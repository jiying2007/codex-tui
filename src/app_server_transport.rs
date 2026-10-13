use crate::app_server_target::{ResolvedAppServerEndpoint, ResolvedAppServerTarget};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::task::JoinSet;
use tokio::time::timeout;
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
        // At most one drain. Dropping a connection aborts it even if a descendant
        // retains the stderr writer; no detached task can outlive this owner.
        _stderr_tasks: JoinSet<()>,
        reader: Box<BufReader<ChildStdout>>,
        pending_line: Vec<u8>,
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

mod health;
use health::TransportHealth;

pub struct AppServerTransport {
    inner: Transport,
    health: TransportHealth,
}

const APP_SERVER_MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const APP_SERVER_MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const APP_SERVER_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const APP_SERVER_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

fn websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(APP_SERVER_MAX_MESSAGE_BYTES))
        .max_frame_size(Some(APP_SERVER_MAX_FRAME_BYTES))
}

impl AppServerTransport {
    pub async fn connect(target: &ResolvedAppServerTarget) -> Result<Self> {
        Self::connect_with_deadline(target, APP_SERVER_CONNECT_TIMEOUT).await
    }

    async fn connect_with_deadline(
        target: &ResolvedAppServerTarget,
        deadline: Duration,
    ) -> Result<Self> {
        timeout(deadline, Self::connect_unbounded(target))
            .await
            .with_context(|| {
                format!(
                    "App Server target {:?} connect/handshake timed out after {}ms",
                    target.name,
                    deadline.as_millis()
                )
            })?
    }

    async fn connect_unbounded(target: &ResolvedAppServerTarget) -> Result<Self> {
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
                let mut stderr_tasks = JoinSet::new();
                if let Some(mut stderr) = child.stderr.take() {
                    stderr_tasks.spawn(async move {
                        let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
                    });
                }
                Transport::Stdio {
                    _child: Box::new(child),
                    _stderr_tasks: stderr_tasks,
                    reader: Box::new(BufReader::new(stdout)),
                    pending_line: Vec::new(),
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

        Ok(Self {
            inner,
            health: TransportHealth::default(),
        })
    }

    pub(crate) fn is_quarantined(&self) -> bool {
        self.health.is_quarantined()
    }

    pub async fn read_json(&mut self) -> Result<Option<Value>> {
        self.health.check()?;
        let result = match &mut self.inner {
            Transport::Stdio {
                reader,
                pending_line,
                ..
            } => read_stdio_json(reader.as_mut(), pending_line).await,
            Transport::WebSocket { stream } => read_websocket_json(stream.as_mut()).await,
            #[cfg(unix)]
            Transport::UnixSocket { stream } => read_websocket_json(stream.as_mut()).await,
        };
        self.health.observe_read(&result);
        result
    }

    pub async fn write_json(&mut self, value: &Value) -> Result<()> {
        self.health
            .write(APP_SERVER_WRITE_TIMEOUT, async {
                match &mut self.inner {
                    Transport::Stdio { writer, .. } => {
                        write_stdio_json(writer.as_mut(), value).await
                    }
                    Transport::WebSocket { stream } => {
                        write_websocket_json(stream.as_mut(), value).await
                    }
                    #[cfg(unix)]
                    Transport::UnixSocket { stream } => {
                        write_websocket_json(stream.as_mut(), value).await
                    }
                }
            })
            .await
    }
}

async fn write_stdio_json<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    value: &Value,
) -> Result<()> {
    let mut encoded = serde_json::to_vec(value).context("encode App Server stdio JSON")?;
    encoded.push(b'\n');
    writer
        .write_all(&encoded)
        .await
        .context("write App Server stdio")?;
    writer.flush().await.context("flush App Server stdio")
}

async fn read_stdio_json<R>(reader: &mut R, pending: &mut Vec<u8>) -> Result<Option<Value>>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let Some(bytes) = read_bounded_line(reader, APP_SERVER_MAX_MESSAGE_BYTES, pending).await?
        else {
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

// fill_buf is cancellation-safe; consume and durable append have no intervening await.
async fn read_bounded_line<R>(
    reader: &mut R,
    limit: usize,
    bytes: &mut Vec<u8>,
) -> Result<Option<Vec<u8>>>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let available = reader.fill_buf().await.context("read App Server stdio")?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            ensure_message_size(bytes.len(), limit, "stdio")?;
            return Ok(Some(std::mem::take(bytes)));
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        anyhow::ensure!(
            bytes.len().saturating_add(take) <= limit.saturating_add(2),
            "App Server stdio JSON exceeds {limit} byte limit"
        );
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            while matches!(bytes.last(), Some(b'\n' | b'\r')) {
                bytes.pop();
            }
            ensure_message_size(bytes.len(), limit, "stdio")?;
            return Ok(Some(std::mem::take(bytes)));
        }
        anyhow::ensure!(
            bytes.len() <= limit.saturating_add(1),
            "App Server stdio JSON exceeds {limit} byte limit"
        );
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
mod tests;

#[cfg(all(test, target_os = "linux"))]
mod cancelled_write_test;

#[cfg(all(test, target_os = "linux"))]
mod reader_lifecycle_tests;
