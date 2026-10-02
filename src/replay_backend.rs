use crate::{
    app_server_registry,
    backend::{BackendFingerprint, BackendSnapshot, CodexBackend},
};
use anyhow::{Result, ensure};

#[derive(Clone, Debug)]
pub struct ReplayBackend {
    frames: Vec<BackendSnapshot>,
    index: usize,
}

impl ReplayBackend {
    pub fn from_jsonl(input: &str) -> Result<Self> {
        let frames = app_server_registry::replay_registry_jsonl(input)?;
        ensure!(!frames.is_empty(), "replay fixture produced no frames");
        Ok(Self { frames, index: 0 })
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn reset(&mut self) {
        self.index = 0;
    }
}

impl CodexBackend for ReplayBackend {
    fn fingerprint(&self) -> BackendFingerprint {
        BackendFingerprint {
            name: "replay-app-server".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            capabilities: vec!["registry-replay".into()],
        }
    }

    fn snapshot(&self) -> BackendSnapshot {
        self.frames[self.index].clone()
    }

    fn tick(&mut self) -> BackendSnapshot {
        if self.index + 1 < self.frames.len() {
            self.index += 1;
        }
        self.snapshot()
    }
}
