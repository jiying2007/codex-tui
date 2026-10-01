use crate::domain::{AttentionReason, RuntimeStatus, ThreadId, ThreadMetadata, ThreadSummary};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendFingerprint {
    pub name: String,
    pub version: String,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendStatus {
    pub source: String,
    pub connected: bool,
    pub version: Option<String>,
    pub platform: Option<String>,
    pub codex_home: Option<String>,
    pub capabilities: Vec<String>,
    pub optional_capabilities_missing: Vec<String>,
    pub registry_complete: bool,
    pub last_refresh_unix_ms: Option<u64>,
    pub error: Option<String>,
}

impl BackendStatus {
    pub fn starting(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            connected: false,
            version: None,
            platform: None,
            codex_home: None,
            capabilities: vec![],
            optional_capabilities_missing: vec![],
            registry_complete: false,
            last_refresh_unix_ms: None,
            error: None,
        }
    }

    pub fn fake() -> Self {
        Self {
            source: "fake".into(),
            connected: true,
            version: Some(env!("CARGO_PKG_VERSION").into()),
            platform: Some(std::env::consts::OS.into()),
            codex_home: None,
            capabilities: vec![
                "thread/list".into(),
                "thread/status/changed".into(),
                "attention-fixtures".into(),
            ],
            optional_capabilities_missing: vec![],
            registry_complete: true,
            last_refresh_unix_ms: None,
            error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendSnapshot {
    pub generation: u64,
    pub threads: Vec<ThreadSummary>,
    pub status: BackendStatus,
}

pub trait CodexBackend {
    fn fingerprint(&self) -> BackendFingerprint;
    fn snapshot(&self) -> BackendSnapshot;
    fn tick(&mut self) -> BackendSnapshot;
}

#[derive(Clone, Debug)]
pub struct FakeBackend {
    generation: u64,
    threads: Vec<ThreadSummary>,
}

impl Default for FakeBackend {
    fn default() -> Self {
        Self::seeded()
    }
}

impl FakeBackend {
    pub fn seeded() -> Self {
        Self {
            generation: 0,
            threads: vec![
                ThreadSummary {
                    id: ThreadId::new("thread-impl"),
                    workspace: "codex-tui".into(),
                    title: "Implement M0 control-plane skeleton".into(),
                    runtime: RuntimeStatus::Working,
                    attention: vec![],
                    pinned: true,
                    alias: Some("M0 bootstrap".into()),
                    metadata: ThreadMetadata::default(),
                },
                ThreadSummary {
                    id: ThreadId::new("thread-kws"),
                    workspace: "kws-pipeline".into(),
                    title: "Decoder boundary replay".into(),
                    runtime: RuntimeStatus::WaitingHuman,
                    attention: vec![AttentionReason::UserInputRequired],
                    pinned: false,
                    alias: None,
                    metadata: ThreadMetadata::default(),
                },
                ThreadSummary {
                    id: ThreadId::new("thread-audio"),
                    workspace: "audio-pipeline".into(),
                    title: "Review VAD evidence".into(),
                    runtime: RuntimeStatus::Ready,
                    attention: vec![AttentionReason::ReadyForReview],
                    pinned: false,
                    alias: None,
                    metadata: ThreadMetadata::default(),
                },
                ThreadSummary {
                    id: ThreadId::new("thread-platform"),
                    workspace: "engineering-platform".into(),
                    title: "Outbox dispatcher follow-up".into(),
                    runtime: RuntimeStatus::Inactive,
                    attention: vec![],
                    pinned: false,
                    alias: None,
                    metadata: ThreadMetadata::default(),
                },
            ],
        }
    }

    pub fn scaled(count: usize) -> Self {
        let mut threads = Vec::with_capacity(count);
        for index in 0..count {
            let runtime = match index % 5 {
                0 => RuntimeStatus::Working,
                1 => RuntimeStatus::WaitingHuman,
                2 => RuntimeStatus::Ready,
                3 => RuntimeStatus::SystemError,
                _ => RuntimeStatus::Inactive,
            };
            let attention = match index % 17 {
                0 => vec![AttentionReason::ApprovalRequired],
                1 => vec![AttentionReason::UserInputRequired],
                2 => vec![AttentionReason::ReadyForReview],
                _ => vec![],
            };
            threads.push(ThreadSummary {
                id: ThreadId::new(format!("thread-scale-{index:05}")),
                workspace: format!("repo-{:03}", index % 64),
                title: format!("Synthetic work item {index:05}"),
                runtime,
                attention,
                pinned: index % 97 == 0,
                alias: None,
                metadata: ThreadMetadata::default(),
            });
        }
        Self {
            generation: 0,
            threads,
        }
    }
}

impl CodexBackend for FakeBackend {
    fn fingerprint(&self) -> BackendFingerprint {
        BackendFingerprint {
            name: "fake".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            capabilities: vec![
                "thread/list".into(),
                "thread/status/changed".into(),
                "attention-fixtures".into(),
            ],
        }
    }

    fn snapshot(&self) -> BackendSnapshot {
        BackendSnapshot {
            generation: self.generation,
            threads: self.threads.clone(),
            status: BackendStatus::fake(),
        }
    }

    fn tick(&mut self) -> BackendSnapshot {
        self.generation += 1;
        if let Some(first) = self.threads.first_mut() {
            first.runtime = match self.generation % 4 {
                0 => RuntimeStatus::Working,
                1 => RuntimeStatus::WaitingHuman,
                2 => RuntimeStatus::Ready,
                _ => RuntimeStatus::Inactive,
            };
            first.attention = if self.generation.is_multiple_of(4) {
                vec![AttentionReason::ApprovalRequired]
            } else {
                vec![]
            };
        }
        self.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attention_is_independent_from_runtime_status() {
        let mut backend = FakeBackend::seeded();
        let mut seen_working_attention = false;
        for _ in 0..16 {
            let snapshot = backend.tick();
            let first = &snapshot.threads[0];
            if first.runtime == RuntimeStatus::Working && first.needs_attention() {
                seen_working_attention = true;
                break;
            }
        }
        assert!(
            seen_working_attention,
            "fake backend should prove attention is orthogonal to runtime"
        );
    }
}
