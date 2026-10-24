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
        context.branch = Some("feature".into());
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
        head_sha: None,
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
        head_sha: value.change_requests[0].head_sha.clone(),
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
        observed_at_unix_ms: codex_tui::operation::now_unix_ms(),
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

#[test]
fn project_refresh_invalidates_nonleader_review_and_rejects_inflight_old_head() {
    use codex_tui::{app::Effect, operation::now_unix_ms, planning::PlanningAttention};
    let mut state = fixture();
    let mut first = observation(&state);
    first.change_requests[0].head_sha = Some("a".repeat(40));
    let mut second = first.clone();
    second.thread_id = state.threads[1].id.clone();
    second.cwd = state.threads[1].metadata.cwd.clone();
    let old_target = target(&second);
    let mut old_summary = summary(&old_target);
    old_summary.changes_requested_by_count = 1;
    second.review = Some(old_summary.clone());
    state
        .forge_observations
        .insert(first.thread_id.0.clone(), first.clone());
    state
        .forge_observations
        .insert(second.thread_id.0.clone(), second.clone());
    reduce(
        &mut state,
        Action::ReconcilePlanning {
            now_unix_ms: now_unix_ms(),
        },
    );
    assert!(
        state
            .work_card_for_thread(&second.thread_id)
            .unwrap()
            .attention
            .contains(&PlanningAttention::ChangeRequested)
    );
    let refresh = reduce(&mut state, Action::RefreshForgeProjections);
    assert!(
        matches!(refresh.as_slice(), [Effect::ProbeForge { thread_id, .. }]
        if *thread_id == first.thread_id)
    );
    first.observed_at_unix_ms = now_unix_ms();
    first.change_requests[0].head_sha = Some("b".repeat(40));
    reduce(&mut state, Action::ForgeObservationLoaded(first));
    assert!(
        state.forge_observations[&second.thread_id.0]
            .review
            .is_none()
    );
    reduce(
        &mut state,
        Action::ReconcilePlanning {
            now_unix_ms: now_unix_ms(),
        },
    );
    assert!(
        !state
            .work_card_for_thread(&second.thread_id)
            .unwrap()
            .attention
            .contains(&PlanningAttention::ChangeRequested)
    );

    install_review(&mut state, old_target, old_summary);
    assert!(
        state.forge_observations[&second.thread_id.0]
            .review
            .is_none()
    );
    state.selected = 1;
    let effects = reduce(&mut state, Action::OpenReview);
    let next_target = effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::ProbeForgeReview(target) => Some(target),
            _ => None,
        })
        .expect("changed head requires a fresh review probe");
    assert_eq!(next_target.head_sha, Some("b".repeat(40)));
    let response = summary(&next_target);
    install_review(&mut state, next_target, response);
    assert!(
        !reduce(&mut state, Action::OpenReview)
            .iter()
            .any(|effect| matches!(effect, Effect::ProbeForgeReview(_)))
    );
}

#[test]
fn open_review_reprobes_expired_and_failed_review_without_showing_old_counts() {
    use codex_tui::app::Effect;
    for failed in [false, true] {
        let mut state = fixture();
        let mut value = observation(&state);
        let mut review = summary(&target(&value));
        if failed {
            review.error = Some("synthetic provider failure".into());
        } else {
            review.observed_at_unix_ms = 1;
        }
        value.review = Some(review);
        state
            .forge_observations
            .insert(value.thread_id.0.clone(), value.clone());
        let effects = reduce(&mut state, Action::OpenReview);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::ProbeForgeReview(_))),
            "failed={failed}"
        );
        assert!(
            state.forge_observations[&value.thread_id.0]
                .review
                .is_none()
        );
        assert_eq!(
            state.forge_observations[&value.thread_id.0].capabilities
                [&ForgeCapability::ApprovalSummary],
            CapabilityState::Unknown
        );
    }
}

#[test]
fn project_refresh_drops_expired_review_even_when_head_is_unchanged() {
    let mut state = fixture();
    let mut old = observation(&state);
    let mut review = summary(&target(&old));
    review.observed_at_unix_ms = 1;
    old.review = Some(review);
    state
        .forge_observations
        .insert(old.thread_id.0.clone(), old.clone());
    let fresh = observation(&state);
    reduce(&mut state, Action::ForgeObservationLoaded(fresh));
    assert!(state.forge_observations[&old.thread_id.0].review.is_none());
}

#[test]
fn branch_change_drops_old_review_and_rejects_old_response_in_the_same_project() {
    use codex_tui::{app::Effect, operation::now_unix_ms, planning::PlanningAttention};
    let mut state = fixture();
    let mut value = observation(&state);
    value.change_requests[0].head_sha = Some("a".repeat(40));
    let mut other_change = value.change_requests[0].clone();
    other_change.iid = 8;
    other_change.source_branch = "other-feature".into();
    other_change.head_sha = Some("b".repeat(40));
    value.change_requests.push(other_change);
    let old_target = target(&value);
    let mut old_summary = summary(&old_target);
    old_summary.changes_requested_by_count = 1;
    value.review = Some(old_summary.clone());
    state
        .forge_observations
        .insert(value.thread_id.0.clone(), value.clone());
    reduce(
        &mut state,
        Action::ReconcilePlanning {
            now_unix_ms: now_unix_ms(),
        },
    );
    assert!(
        state
            .work_card_for_thread(&value.thread_id)
            .unwrap()
            .attention
            .contains(&PlanningAttention::ChangeRequested)
    );

    let mut context = state.git_contexts[&value.thread_id.0].clone();
    context.branch = Some("other-feature".into());
    reduce(&mut state, Action::GitContextLoaded(context));
    assert!(
        state.forge_observations[&value.thread_id.0]
            .review
            .is_none()
    );
    install_review(&mut state, old_target, old_summary);
    assert!(
        state.forge_observations[&value.thread_id.0]
            .review
            .is_none()
    );
    reduce(
        &mut state,
        Action::ReconcilePlanning {
            now_unix_ms: now_unix_ms(),
        },
    );
    assert!(
        !state
            .work_card_for_thread(&value.thread_id)
            .unwrap()
            .attention
            .contains(&PlanningAttention::ChangeRequested)
    );
    let effects = reduce(&mut state, Action::OpenReview);
    assert!(effects.iter().any(|effect| matches!(effect,
        Effect::ProbeForgeReview(target) if target.change_request_iid == 8
            && target.head_sha.as_deref() == Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    )));
}

#[test]
fn overview_refresh_does_not_reuse_review_for_a_different_selected_change() {
    let mut state = fixture();
    let mut old = observation(&state);
    old.review = Some(summary(&target(&old)));
    state
        .forge_observations
        .insert(old.thread_id.0.clone(), old.clone());
    let mut fresh = observation(&state);
    let mut selected = fresh.change_requests[0].clone();
    selected.iid = 8;
    fresh.change_requests.insert(0, selected);
    reduce(&mut state, Action::ForgeObservationLoaded(fresh));
    assert!(state.forge_observations[&old.thread_id.0].review.is_none());
}
