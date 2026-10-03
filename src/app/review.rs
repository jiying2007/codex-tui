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
    use crate::forge::{
        ChangeRequestSummary, ForgeIdentity, ForgeObservation, ForgeProviderKind,
    };
    use crate::git::GitContext;

    #[test]
    fn non_review_view_never_emits_external_open_effect() {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.view = View::Registry;
        assert!(app.open_review_external().is_empty());
    }

    #[test]
    fn review_external_prefers_change_request_url_over_repository_url() {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        let thread_id = app.threads[0].id.clone();
        let cwd = app.threads[0].metadata.cwd.clone();
        let mut git = GitContext::pending(thread_id.clone(), cwd.clone());
        git.is_repository = true;
        git.branch = Some("feature/review".into());
        app.git_contexts.insert(thread_id.0.clone(), git);

        let mut forge = ForgeObservation::pending(thread_id.clone(), cwd);
        forge.identity = Some(ForgeIdentity {
            provider: ForgeProviderKind::GitHub,
            host: "github.com".into(),
            project_id: "123".into(),
            path_with_namespace: "octo/repo".into(),
            web_url: "https://github.com/octo/repo".into(),
            default_branch: Some("main".into()),
        });
        forge.change_requests.push(ChangeRequestSummary {
            iid: 7,
            title: "Review me".into(),
            state: "open".into(),
            source_branch: "feature/review".into(),
            target_branch: "main".into(),
            web_url: "https://github.com/octo/repo/pull/7".into(),
            updated_at: None,
            draft: false,
            detailed_merge_status: Some("mergeable".into()),
            blocking_discussions_resolved: Some(true),
        });
        app.forge_observations.insert(thread_id.0.clone(), forge);
        app.view = View::Review(thread_id.clone());

        assert_eq!(
            app.review_external_url(&thread_id).as_deref(),
            Some("https://github.com/octo/repo/pull/7")
        );
        assert_eq!(
            app.open_review_external(),
            vec![Effect::OpenExternalUrl {
                url: "https://github.com/octo/repo/pull/7".into()
            }]
        );
    }
}
