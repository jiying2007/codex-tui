use super::{SourceStatus, headless_field, options::HeadlessOptions};
use crate::{
    app_server,
    app_server_target::ResolvedAppServerTarget,
    backend::{BackendSnapshot, CodexBackend, FakeBackend},
    domain::ThreadSummary,
    store::{AppConfig, FileStore},
};
use anyhow::{Context, Result};

pub(super) enum SnapshotSource {
    Fixture { scaled: bool },
    Target(ResolvedAppServerTarget),
}

impl SnapshotSource {
    pub(super) fn resolve(options: &HeadlessOptions) -> Result<Self, Box<SourceStatus>> {
        if options.fake {
            return Ok(Self::Fixture {
                scaled: options.fixture_10k,
            });
        }
        let mut status = SourceStatus::unavailable(options.target.clone());
        let resolved = (|| -> Result<Self> {
            let path = FileStore::discover()?.config_path();
            // Reading Headless selection must not materialize a default config.
            let config = match std::fs::read_to_string(&path) {
                Ok(text) => toml::from_str::<AppConfig>(&text)
                    .with_context(|| format!("parse config {}", path.display()))?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppConfig::default(),
                Err(error) => {
                    return Err(error).with_context(|| format!("read config {}", path.display()));
                }
            };
            status.target_id = Some(
                options
                    .target
                    .as_deref()
                    .unwrap_or(&config.app_server.active)
                    .trim()
                    .into(),
            );
            let target =
                ResolvedAppServerTarget::resolve(&config.app_server, options.target.as_deref())?;
            Ok(Self::Target(target))
        })();
        resolved.map_err(|error| {
            status.error = Some(format!("{error:#}"));
            Box::new(status)
        })
    }

    pub(super) async fn load(self) -> (Vec<ThreadSummary>, SourceStatus) {
        let (result, mut source) = match self {
            Self::Fixture { scaled } => {
                let backend = if scaled {
                    FakeBackend::scaled(10_000)
                } else {
                    FakeBackend::seeded()
                };
                let mut source = SourceStatus::unavailable(None);
                source.scope = "fixture";
                (Ok(backend.snapshot()), source)
            }
            Self::Target(target) => {
                let mut source = SourceStatus::unavailable(Some(target.name.clone()));
                source.source = target.status_source();
                source.transport = Some(target.endpoint.transport_label().into());
                (app_server::probe_target(target).await, source)
            }
        };
        match result {
            Ok(snapshot) => {
                source.observe(&snapshot);
                (snapshot.threads, source)
            }
            Err(error) => {
                source.error = Some(format!("{error:#}"));
                (vec![], source)
            }
        }
    }
}

impl SourceStatus {
    fn unavailable(target_id: Option<String>) -> Self {
        Self {
            source: "codex-app-server".into(),
            target_id,
            transport: None,
            scope: "single-target",
            connected: false,
            version: None,
            platform: None,
            capabilities: vec![],
            optional_capabilities_missing: vec![],
            last_refresh_unix_ms: None,
            degraded: true,
            error: None,
        }
    }

    fn observe(&mut self, snapshot: &BackendSnapshot) {
        let status = &snapshot.status;
        self.source = status.source.clone();
        self.connected = status.connected;
        self.version = status.version.clone();
        self.platform = status.platform.clone();
        self.capabilities = status.capabilities.clone();
        self.optional_capabilities_missing = status.optional_capabilities_missing.clone();
        self.last_refresh_unix_ms = status.last_refresh_unix_ms;
        self.degraded = !status.connected || status.error.is_some();
        self.error = status.error.clone();
    }
}

pub(super) fn print_source(source: &SourceStatus) {
    println!(
        "source: {} connected={}",
        headless_field(&source.source),
        source.connected
    );
    println!(
        "target: {} transport={} scope={}",
        headless_field(source.target_id.as_deref().unwrap_or("none")),
        headless_field(source.transport.as_deref().unwrap_or("none")),
        source.scope
    );
    if let Some(error) = &source.error {
        println!("source-error: {}", headless_field(error));
    }
}
