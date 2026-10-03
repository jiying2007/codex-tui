use crate::app_server_target::{
    ResolvedAppServerEndpoint, ResolvedAppServerTarget,
};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines};
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, client_async, connect_async,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{
            HeaderValue,
            header::AUTHORIZATION,
        },
    },
};

enum Transport {
    Stdio {
        _child: Child,
        reader: Lines<BufReader<ChildStdout>>,
        writer: BufWriter<ChildStdin>,
    },
    WebSocket {
        stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    },
    #[cfg(unix)]
    UnixSocket {
        stream: WebSocketStream<UnixStream>,
    },
}

pub struct AppServerTransport {
    inner: Transport,
}

impl AppServerTransport {
    pub async fn connect(target: &ResolvedAppServerTarget) -> Result<Self> {
        let inner = match &target.endpoint {
            ResolvedAppServerEndpoint::Stdio { codex_bin } => {
                let mut command =
                    Command::new(codex_bin.clone().unwrap_or_else(|| "codex".into()));
                command
                    .args(["app-server", "--listen", "stdio://"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .kill_on_drop(true);
                let mut child = command.spawn().with_context(|| {
                    format!(
                        "spawn local Codex App Server target {:?}",
                        target.name
                    )
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
                    _child: child,
                    reader: BufReader::new(stdout).lines(),
                    writer: BufWriter::new(stdin),
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
                let (stream, _) = connect_async(request)
                    .await
                    .with_context(|| {
                        format!(
                            "connect App Server target {:?} at {}",
                            target.name,
                            target.diagnostic_endpoint()
                        )
                    })?;
                Transport::WebSocket { stream }
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
                let Some(line) = reader
                    .next_line()
                    .await
                    .context("read App Server stdio")?
                else {
                    return Ok(None);
                };
                if line.trim().is_empty() {
                    continue;
                }
                return serde_json::from_str(&line)
                    .context("decode App Server stdio JSON")
                    .map(Some);
            },
            Transport::WebSocket { stream } => read_websocket_json(stream).await,
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
            Transport::WebSocket { stream } => write_websocket_json(stream, value).await,
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

async fn write_websocket_json<S>(
    stream: &mut WebSocketStream<S>,
    value: &Value,
) -> Result<()>
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
        AppServerConfig, AppServerTargetConfig, ResolvedAppServerTarget,
    };

    #[test]
    fn remote_target_error_context_never_formats_bearer_token() {
        let mut config = AppServerConfig::default();
        config.active = "remote".into();
        config.targets.insert(
            "remote".into(),
            AppServerTargetConfig::Websocket {
                url: "wss://example.test/rpc?workspace=a".into(),
                auth_token_env: None,
            },
        );
        let target =
            ResolvedAppServerTarget::resolve(&config, None).expect("remote target");
        assert_eq!(
            target.diagnostic_endpoint(),
            "wss://example.test/rpc"
        );
    }
}
