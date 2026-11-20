//! Enqueue a native thread/resume only for the selected live Target and known ThreadId.
use super::*;
use anyhow::ensure;

impl RegistryHandle {
    /// Admission only: Ok means the read/subscribe request was queued, not resumed.
    /// A later exact-ID thread/resume acknowledgement establishes subscription.
    pub fn load_conversation_checked(
        &self,
        observed: &BackendStatus,
        thread_id: ThreadId,
    ) -> Result<()> {
        self.verify_observed_thread(observed, &thread_id)?;
        self.load_conversation(thread_id)
    }
}

impl RegistryHandle {
    pub fn load_older_conversation_checked(
        &self,
        observed: &BackendStatus,
        thread_id: ThreadId,
        turn_cursor: Option<String>,
        item_cursor: Option<String>,
    ) -> Result<()> {
        self.verify_observed_thread(observed, &thread_id)?;
        self.load_older_conversation(thread_id, turn_cursor, item_cursor)
    }

    pub fn refresh_thread_queue_checked(
        &self,
        observed: &BackendStatus,
        thread_id: ThreadId,
    ) -> Result<()> {
        self.verify_observed_thread(observed, &thread_id)?;
        self.refresh_thread_queue(thread_id)
    }

    pub fn jump_to_transcript_hit_checked(
        &self,
        observed: &BackendStatus,
        hit: crate::transcript_search::TranscriptSearchHit,
    ) -> Result<()> {
        self.verify_observed_thread(observed, &hit.thread_id)?;
        self.jump_to_transcript_hit(hit)
    }

    fn verify_observed_thread(&self, observed: &BackendStatus, id: &ThreadId) -> Result<()> {
        self.scope.verify(observed)?;
        let snapshot = self.rx.borrow();
        verify_observed_thread(&snapshot, observed, id)
    }
}

fn verify_observed_thread(
    snapshot: &BackendSnapshot,
    observed: &BackendStatus,
    id: &ThreadId,
) -> Result<()> {
    ensure!(
        observed.connected
            && snapshot.status.connected
            && snapshot.status.source == observed.source
            && snapshot.status.codex_home == observed.codex_home,
        "Codex Target/home observation changed; locate again before resuming"
    );
    ensure!(
        !id.0.trim().is_empty() && snapshot.threads.iter().any(|thread| thread.id == *id),
        "selected ThreadId not in the observed Target registry; locate again"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    fn snapshot() -> BackendSnapshot {
        let mut current = FakeBackend::seeded().snapshot();
        current.status.source = "codex-app-server[target=A transport=stdio]".into();
        current.status.codex_home = Some("/home/a/.codex".into());
        current
    }

    #[test]
    fn equal_thread_id_on_other_target_or_home_has_no_resume_admission() {
        let current = snapshot();
        let id = current.threads[0].id.clone();
        assert!(verify_observed_thread(&current, &current.status, &id).is_ok());
        let mut other = current.status.clone();
        other.source = "codex-app-server[target=B transport=stdio]".into();
        assert!(verify_observed_thread(&current, &other, &id).is_err());
        other.source = current.status.source.clone();
        other.codex_home = Some("/home/b/.codex".into());
        assert!(verify_observed_thread(&current, &other, &id).is_err());
        assert!(
            verify_observed_thread(&current, &current.status, &ThreadId::new("missing")).is_err()
        );
    }

    #[test]
    fn disconnected_and_stale_lookups_cannot_be_sent_to_resume() {
        let mut current = snapshot();
        let id = current.threads[0].id.clone();
        let mut observed = current.status.clone();
        observed.connected = false;
        assert!(verify_observed_thread(&current, &observed, &id).is_err());
        observed.connected = true;
        current.status.connected = false;
        assert!(verify_observed_thread(&current, &observed, &id).is_err());
        current.status.connected = true;
        current.threads.clear();
        assert!(verify_observed_thread(&current, &observed, &id).is_err());
    }
}
