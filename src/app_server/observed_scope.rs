//! A live handle owns one App Server. Its initialized scope fences wrong-target writes.
use crate::backend::BackendStatus;
use anyhow::{Result, ensure};

/// Never persisted, serialized or used as an identity across host boundaries.
pub(super) struct ObservedScope {
    target: String,
    codex_home: Option<String>,
}

impl ObservedScope {
    pub(super) fn capture(status: &BackendStatus) -> Self {
        Self {
            target: status.source.clone(),
            codex_home: status.codex_home.clone(),
        }
    }

    pub(super) fn verify(&self, observed: &BackendStatus) -> Result<()> {
        ensure!(
            observed.connected
                && self.target == observed.source
                && self.codex_home == observed.codex_home,
            "active App Server Target/Codex home changed; write not queued"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(target: &str, home: Option<&str>) -> BackendStatus {
        let mut status = BackendStatus::fake();
        status.source = target.into();
        status.codex_home = home.map(str::to_owned);
        status
    }

    #[test]
    fn target_and_home_mismatch_reject_even_same_thread_id() {
        let guard = ObservedScope::capture(&source("target=A", Some("/home/a/.codex")));
        assert!(
            guard
                .verify(&source("target=A", Some("/home/a/.codex")))
                .is_ok()
        );
        for other in [
            source("target=B", Some("/home/a/.codex")),
            source("target=A", Some("/home/b/.codex")),
            source("target=A", None),
        ] {
            assert!(guard.verify(&other).is_err());
        }
    }

    #[test]
    fn disconnect_or_new_home_cannot_authorize_old_actor() {
        let guard = ObservedScope::capture(&source("target=A", None));
        assert!(guard.verify(&source("target=A", None)).is_ok());
        let mut offline = source("target=A", None);
        offline.connected = false;
        assert!(guard.verify(&offline).is_err());
        assert!(guard.verify(&source("target=A", Some("/unknown"))).is_err());
    }
}
