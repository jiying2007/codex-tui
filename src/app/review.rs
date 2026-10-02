use super::{AppState, Effect, View};
use crate::domain::ThreadId;

impl AppState {
    pub(super) fn review_external_url(&self, thread_id: &ThreadId) -> Option<String> {
        let observation = self.forge_observation(thread_id)?;
        let identity = observation.identity.as_ref()?;
        let branch = self
            .git_context(thread_id)
            .and_then(|context| context.branch.as_deref());
        if let Some(branch) = branch
            && let Some(change) = observation.change_request_for_branch(branch)
        {
            return Some(change.web_url.clone());
        }
        Some(identity.web_url.clone())
    }

    pub(super) fn open_review_external(&self) -> Vec<Effect> {
        let View::Review(thread_id) = &self.view else {
            return vec![];
        };
        let Some(url) = self.review_external_url(thread_id) else {
            return vec![];
        };
        vec![Effect::OpenExternalUrl { url }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    #[test]
    fn non_review_view_never_emits_external_open_effect() {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.view = View::Registry;
        assert!(app.open_review_external().is_empty());
    }
}
