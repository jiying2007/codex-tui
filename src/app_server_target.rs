use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use url::Url;

fn default_target_name() -> String {
    "local".into()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerConfig {
    #[serde(default = "default_target_name")]
    pub active: String,
    #[serde(default)]
    pub targets: BTreeMap<String, AppServerTargetConfig>,
}

impl Default for AppServerConfig {
    fn default() -> Self {
        Self {
            active: default_target_name(),
            targets: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "kebab-case")]
pub enum AppServerTargetConfig {
    Stdio {
        #[serde(default)]
        codex_bin: Option<String>,
    },
    Websocket {
        url: String,
        #[serde(default)]
        auth_token_env: Option<String>,
    },
    UnixSocket {
        path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedAppServerEndpoint {
    Stdio {
        codex_bin: Option<OsString>,
    },
    WebSocket {
        url: String,
        auth_token: Option<String>,
    },
    UnixSocket {
        path: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedAppServerTarget {
    pub name: String,
    pub endpoint: ResolvedAppServerEndpoint,
}

impl ResolvedAppServerTarget {
    pub fn implicit_local(codex_bin: Option<OsString>) -> Self {
        Self {
            name: "local".into(),
            endpoint: ResolvedAppServerEndpoint::Stdio { codex_bin },
        }
    }

    pub fn resolve(config: &AppServerConfig, override_name: Option<&str>) -> Result<Self> {
        let name = override_name.unwrap_or(&config.active).trim();
        anyhow::ensure!(!name.is_empty(), "App Server target name must not be empty");

        let Some(target) = config.targets.get(name) else {
            if name == "local" {
                return Ok(Self::implicit_local(None));
            }
            anyhow::bail!(
                "unknown App Server target {name:?}; configure [app_server.targets.{name}]"
            );
        };

        let endpoint = match target {
            AppServerTargetConfig::Stdio { codex_bin } => ResolvedAppServerEndpoint::Stdio {
                codex_bin: codex_bin
                    .as_ref()
                    .map(|value| OsString::from(value.trim()))
                    .filter(|value| !value.is_empty()),
            },
            AppServerTargetConfig::Websocket {
                url,
                auth_token_env,
            } => {
                let url = validate_websocket_url(url)?;
                let auth_token = match auth_token_env.as_deref().map(str::trim) {
                    Some("") => anyhow::bail!("auth_token_env must not be empty"),
                    Some(name) => {
                        let token = std::env::var(name).with_context(|| {
                            format!("App Server token environment {name:?} is not set")
                        })?;
                        anyhow::ensure!(
                            !token.trim().is_empty(),
                            "App Server token environment {name:?} is empty"
                        );
                        anyhow::ensure!(
                            websocket_url_supports_auth(&url),
                            "bearer authentication requires wss:// or a loopback ws:// endpoint"
                        );
                        Some(token)
                    }
                    None => None,
                };
                ResolvedAppServerEndpoint::WebSocket {
                    url: url.to_string(),
                    auth_token,
                }
            }
            AppServerTargetConfig::UnixSocket { path } => {
                let path = PathBuf::from(path.trim());
                anyhow::ensure!(
                    path.is_absolute(),
                    "Unix App Server target path must be absolute: {}",
                    path.display()
                );
                ResolvedAppServerEndpoint::UnixSocket { path }
            }
        };

        Ok(Self {
            name: name.to_string(),
            endpoint,
        })
    }

    pub fn status_source(&self) -> String {
        format!(
            "codex-app-server[target={} transport={}]",
            self.name,
            self.endpoint.transport_label()
        )
    }

    pub fn diagnostic_endpoint(&self) -> String {
        match &self.endpoint {
            ResolvedAppServerEndpoint::Stdio { codex_bin } => codex_bin
                .as_ref()
                .map(|value| format!("stdio:// ({})", Path::new(value).display()))
                .unwrap_or_else(|| "stdio:// (codex from PATH)".into()),
            ResolvedAppServerEndpoint::WebSocket { url, .. } => sanitize_websocket_url(url),
            ResolvedAppServerEndpoint::UnixSocket { path } => {
                format!("unix://{}", path.display())
            }
        }
    }
}

impl ResolvedAppServerEndpoint {
    pub fn transport_label(&self) -> &'static str {
        match self {
            Self::Stdio { .. } => "stdio",
            Self::WebSocket { url, .. } => {
                if url.as_bytes().starts_with(b"wss://") {
                    "wss"
                } else {
                    "ws"
                }
            }
            Self::UnixSocket { .. } => "unix",
        }
    }
}

fn validate_websocket_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw.trim()).context("parse App Server WebSocket URL")?;
    anyhow::ensure!(
        matches!(url.scheme(), "ws" | "wss"),
        "App Server WebSocket URL must use ws:// or wss://"
    );
    anyhow::ensure!(
        url.host_str().is_some(),
        "App Server WebSocket URL requires a host"
    );
    anyhow::ensure!(
        url.username().is_empty() && url.password().is_none(),
        "App Server WebSocket URL must not embed credentials; use auth_token_env"
    );
    anyhow::ensure!(
        websocket_url_supports_auth(&url),
        "remote plaintext ws:// App Server target is not allowed; use wss:// or a loopback SSH tunnel"
    );
    Ok(url)
}

fn websocket_url_supports_auth(url: &Url) -> bool {
    if url.scheme() == "wss" {
        return true;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

fn sanitize_websocket_url(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return "<invalid-websocket-url>".into();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_resolves_zero_config_local_stdio() {
        let target = ResolvedAppServerTarget::resolve(&AppServerConfig::default(), None)
            .expect("default target");
        assert_eq!(target.name, "local");
        assert!(matches!(
            target.endpoint,
            ResolvedAppServerEndpoint::Stdio { codex_bin: None }
        ));
    }

    #[test]
    fn named_unix_target_requires_absolute_path() {
        let mut config = AppServerConfig {
            active: "remote".into(),
            ..AppServerConfig::default()
        };
        config.targets.insert(
            "remote".into(),
            AppServerTargetConfig::UnixSocket {
                path: "relative.sock".into(),
            },
        );
        assert!(ResolvedAppServerTarget::resolve(&config, None).is_err());

        let absolute_socket = std::env::temp_dir().join("codex.sock");
        config.targets.insert(
            "remote".into(),
            AppServerTargetConfig::UnixSocket {
                path: absolute_socket.to_string_lossy().into_owned(),
            },
        );
        assert!(matches!(
            ResolvedAppServerTarget::resolve(&config, None)
                .expect("absolute socket")
                .endpoint,
            ResolvedAppServerEndpoint::UnixSocket { .. }
        ));
    }

    #[test]
    fn websocket_diagnostics_strip_query_fragment_and_credentials() {
        let value =
            sanitize_websocket_url("wss://user:secret@example.test:443/rpc?token=secret#fragment");
        assert_eq!(value, "wss://example.test/rpc");
    }

    #[test]
    fn websocket_auth_policy_requires_tls_or_loopback() {
        let loopback = validate_websocket_url("ws://127.0.0.1:4500/rpc").expect("loopback");
        let local = validate_websocket_url("ws://localhost:4500/rpc").expect("localhost");
        let ipv6 = validate_websocket_url("ws://[::1]:4500/rpc").expect("IPv6 loopback");
        let tls = validate_websocket_url("wss://example.test/rpc").expect("tls");
        assert!(websocket_url_supports_auth(&loopback));
        assert!(websocket_url_supports_auth(&local));
        assert!(websocket_url_supports_auth(&ipv6));
        assert!(websocket_url_supports_auth(&tls));
        for plaintext in [
            "ws://192.0.2.8:4500/rpc",
            "ws://internal.example:4500/rpc",
            "ws://localhost.evil.example:4500/rpc",
        ] {
            assert!(validate_websocket_url(plaintext).is_err(), "{plaintext}");
        }
    }

    #[test]
    fn remote_plaintext_is_rejected_even_without_configured_bearer() {
        let mut config = AppServerConfig::default();
        config.targets.insert(
            "insecure".into(),
            AppServerTargetConfig::Websocket {
                url: "ws://192.0.2.8:4500/rpc".into(),
                auth_token_env: None,
            },
        );
        assert!(ResolvedAppServerTarget::resolve(&config, Some("insecure")).is_err());
    }
}
