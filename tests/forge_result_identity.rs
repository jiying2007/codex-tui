//! A change-request IID is not globally unique across repositories or hosts.
use codex_tui::{
    app::{Action, AppState, reduce},
    backend::{CodexBackend, FakeBackend},
    domain::ThreadId,
    forge::{
        CapabilityState, ChangeRequestSummary, ForgeCapability, ForgeIdentity, ForgeObservation,
        ForgeProviderKind, ForgeReviewSummary, ForgeReviewTarget,
    },
    git::GitContext,
};

fn fixture() -> AppState {
    let mut threads = FakeBackend::scaled(2).snapshot().threads;
    threads[0].metadata.cwd = "/repo".into();
    threads[1].metadata.cwd = "/other".into();
    let mut state = AppState::new(threads);
    for thread in state.threads.clone() {
        let mut context = GitContext::pending(thread.id, thread.metadata.cwd);
        context.is_repository = true;
        context.observed_at_unix_ms = 1;
        reduce(&mut state, Action::GitContextLoaded(context));
    }
    state
}
fn observation(state: &AppState) -> ForgeObservation {
    let thread = &state.threads[0];
    let mut value = ForgeObservation::pending(thread.id.clone(), thread.metadata.cwd.clone());
    value.identity = Some(ForgeIdentity {
        provider: ForgeProviderKind::GitLab,
        host: "gitlab.example.com".into(),
        project_id: "42".into(),
        path_with_namespace: "team/repo".into(),
        web_url: "https://gitlab.example.com/team/repo".into(),
        default_branch: Some("main".into()),
    });
    value.observed_at_unix_ms = 10;
    value.change_requests.push(ChangeRequestSummary {
        iid: 7,
        title: "MR".into(),
        state: "opened".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
        web_url: "https://gitlab.example.com/team/repo/-/merge_requests/7".into(),
        updated_at: None,
        draft: false,
        detailed_merge_status: None,
        blocking_discussions_resolved: None,
    });
    value
}
fn target(value: &ForgeObservation) -> ForgeReviewTarget {
    let identity = value.identity.as_ref().unwrap();
    ForgeReviewTarget {
        thread_id: value.thread_id.clone(),
        cwd: value.cwd.clone(),
        provider: identity.provider,
        host: identity.host.clone(),
        project_id: identity.project_id.clone(),
        project_path: identity.path_with_namespace.clone(),
        change_request_iid: 7,
    }
}
fn summary(target: &ForgeReviewTarget) -> ForgeReviewSummary {
    ForgeReviewSummary {
        thread_id: target.thread_id.clone(),
        cwd: target.cwd.clone(),
        change_request_iid: target.change_request_iid,
        approvals_required: Some(1),
        approvals_left: Some(0),
        approved_by_count: 1,
        changes_requested_by_count: 0,
        discussions_total: 2,
        unresolved_discussions: 0,
        approvals_available: true,
        discussions_available: true,
        observed_at_unix_ms: 20,
        error: None,
    }
}
fn install_review(state: &mut AppState, target: ForgeReviewTarget, summary: ForgeReviewSummary) {
    assert!(
        reduce(
            state,
            Action::ForgeReviewLoaded(Box::new(codex_tui::forge::ForgeReviewResult {
                target,
                summary
            },))
        )
        .is_empty()
    );
}

#[test]
fn unknown_thread_observation_is_not_reintroduced() {
    let mut state = fixture();
    let mut value = observation(&state);
    value.thread_id = ThreadId::new("removed");
    value.cwd = "/gone".into();
    reduce(&mut state, Action::ForgeObservationLoaded(value));
    assert!(!state.forge_observations.contains_key("removed"));
}
#[test]
fn former_cwd_observation_cannot_replace_current_source() {
    let mut state = fixture();
    let old = observation(&state);
    let mut threads = state.threads.clone();
    threads[0].metadata.cwd = "/new".into();
    reduce(&mut state, Action::ReplaceThreads(threads));
    let fresh = observation(&state);
    reduce(&mut state, Action::ForgeObservationLoaded(fresh.clone()));
    reduce(&mut state, Action::ForgeObservationLoaded(old));
    assert_eq!(
        state.forge_observations.get(&fresh.thread_id.0),
        Some(&fresh)
    );
}
#[test]
fn same_iid_in_a_different_repository_does_not_reuse_approvals() {
    let mut state = fixture();
    let mut old = observation(&state);
    old.review = Some(summary(&target(&old)));
    state
        .forge_observations
        .insert(old.thread_id.0.clone(), old);
    let mut fresh = observation(&state);
    fresh.identity.as_mut().unwrap().project_id = "99".into();
    reduce(&mut state, Action::ForgeObservationLoaded(fresh.clone()));
    assert_eq!(
        state.forge_observations.get(&fresh.thread_id.0),
        Some(&fresh)
    );
}
#[test]
fn same_source_refresh_keeps_its_existing_review() {
    let mut state = fixture();
    let mut old = observation(&state);
    old.review = Some(summary(&target(&old)));
    state
        .forge_observations
        .insert(old.thread_id.0.clone(), old.clone());
    let fresh = observation(&state);
    reduce(&mut state, Action::ForgeObservationLoaded(fresh));
    assert_eq!(
        state.forge_observations[&old.thread_id.0].review,
        old.review
    );
}
#[test]
fn mismatched_review_source_is_rejected_even_when_cwd_and_iid_match() {
    for field in 0..4 {
        let mut state = fixture();
        let value = observation(&state);
        let mut request = target(&value);
        let response = summary(&request);
        match field {
            0 => request.provider = ForgeProviderKind::GitHub,
            1 => request.host = "different.example.com".into(),
            2 => request.project_id = "99".into(),
            _ => request.project_path = "team/different".into(),
        }
        state
            .forge_observations
            .insert(value.thread_id.0.clone(), value.clone());
        install_review(&mut state, request, response);
        assert_eq!(
            state.forge_observations.get(&value.thread_id.0),
            Some(&value),
            "source field {field}"
        );
    }
}
#[test]
fn removed_thread_review_does_not_update_orphaned_cached_observation() {
    let mut state = fixture();
    let value = observation(&state);
    let request = target(&value);
    let response = summary(&request);
    state
        .forge_observations
        .insert(value.thread_id.0.clone(), value.clone());
    reduce(&mut state, Action::ReplaceThreads(vec![]));
    let before = state.forge_observations.clone();
    install_review(&mut state, request, response);
    assert_eq!(state.forge_observations, before);
}
#[test]
fn matching_live_review_installs_summary_and_capabilities() {
    let mut state = fixture();
    let value = observation(&state);
    let request = target(&value);
    let response = summary(&request);
    state
        .forge_observations
        .insert(value.thread_id.0.clone(), value.clone());
    install_review(&mut state, request, response.clone());
    let installed = &state.forge_observations[&value.thread_id.0];
    assert_eq!(installed.review, Some(response));
    assert_eq!(
        installed.capabilities[&ForgeCapability::ApprovalSummary],
        CapabilityState::Available
    );
}
