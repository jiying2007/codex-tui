use super::*;

#[test]
fn pipeline_head_repository_prevents_fork_branch_collision() {
    let row = |repo: serde_json::Value| {
        serde_json::from_value::<GitHubWorkflowRun>(serde_json::json!({
            "id": 123,
            "status": "completed",
            "conclusion": "failure",
            "head_branch": "feature",
            "head_repository": repo,
            "html_url": "https://github.com/octo/repo/actions/runs/123",
            "updated_at": "2026-10-10T12:00:00Z"
        }))
        .expect("GitHub Actions row")
    };
    let own = row(serde_json::json!({"id": 10, "full_name": "octo/repo"}));
    let fork = row(serde_json::json!({"id": 20, "full_name": "alice/repo"}));
    let unknown = row(serde_json::Value::Null);
    assert_eq!(
        own.branch_for_local_projection(10).as_deref(),
        Some("feature")
    );
    assert_eq!(
        fork.branch_for_local_projection(10).as_deref(),
        Some("alice/repo:feature")
    );
    assert_eq!(
        unknown.branch_for_local_projection(10).as_deref(),
        Some("unverified-source:feature")
    );
    assert_ne!(
        fork.branch_for_local_projection(10).as_deref(),
        Some("feature")
    );
    assert_ne!(
        unknown.branch_for_local_projection(10).as_deref(),
        Some("feature")
    );
}

#[test]
fn fork_and_unverified_sources_cannot_match_a_local_branch_name() {
    let local: GitHubPullRef = serde_json::from_value(serde_json::json!({
        "ref": "feature", "repo": {"id": 123, "full_name": "octo/repo"}
    }))
    .expect("same-repo head");
    let fork: GitHubPullRef = serde_json::from_value(serde_json::json!({
        "ref": "feature", "repo": {"id": 999, "full_name": "alice/repo"}
    }))
    .expect("fork head");
    let unknown: GitHubPullRef = serde_json::from_value(serde_json::json!({
        "ref": "feature", "repo": null
    }))
    .expect("unverified head");
    assert_eq!(local.branch_for_local_projection(123), "feature");
    assert_eq!(fork.branch_for_local_projection(123), "alice/repo:feature");
    assert_eq!(
        unknown.branch_for_local_projection(123),
        "unverified-source:feature"
    );
    assert_ne!(fork.branch_for_local_projection(123), "feature");
    assert_ne!(unknown.branch_for_local_projection(123), "feature");
}

#[test]
fn github_repository_path_is_exact_owner_repo() {
    assert_eq!(
        split_repository_path("openai/codex").unwrap(),
        ("openai", "codex")
    );
    assert!(split_repository_path("group/sub/repo").is_err());
    assert!(split_repository_path("repo").is_err());
}

#[test]
fn github_issue_fixture_distinguishes_pull_requests() {
    let issues: Vec<GitHubIssue> = serde_json::from_value(serde_json::json!([
        {
            "number": 1,
            "title": "Issue",
            "state": "open",
            "html_url": "https://github.com/o/r/issues/1",
            "updated_at": "2026-09-30T00:00:00Z",
            "pull_request": null
        },
        {
            "number": 2,
            "title": "PR-shaped issue",
            "state": "open",
            "html_url": "https://github.com/o/r/pull/2",
            "updated_at": "2026-09-30T00:00:00Z",
            "pull_request": {"url":"https://api.github.com/repos/o/r/pulls/2"}
        }
    ]))
    .expect("fixture");
    assert_eq!(
        issues
            .iter()
            .filter(|issue| issue.pull_request.is_none())
            .count(),
        1
    );
}

#[test]
fn latest_review_per_user_controls_normalized_review_state() {
    let reviews: Vec<GitHubPullReview> = serde_json::from_value(serde_json::json!([
        {"id": 1, "state": "APPROVED", "user": {"id": 10}},
        {"id": 2, "state": "CHANGES_REQUESTED", "user": {"id": 10}},
        {"id": 3, "state": "APPROVED", "user": {"id": 20}},
        {"id": 4, "state": "COMMENTED", "user": {"id": 30}}
    ]))
    .expect("fixture");
    assert_eq!(latest_review_counts(&reviews), (1, 1));
}

#[test]
fn github_action_failure_maps_to_existing_pipeline_attention_semantics() {
    assert_eq!(normalize_run_status("completed", Some("failure")), "failed");
    assert_eq!(normalize_run_status("in_progress", None), "in_progress");
    assert_eq!(
        normalize_run_status("completed", Some("success")),
        "success"
    );
}

#[test]
fn all_github_core_reads_failed_are_not_fresh_or_successful() {
    let (capabilities, freshness, error) =
        crate::forge::core_observation_status(ForgeProviderKind::GitHub, false, false, false);
    assert_eq!(freshness, crate::forge::ForgeFreshness::Unavailable);
    assert!(
        error
            .as_deref()
            .is_some_and(|message| message.contains("github"))
    );
    for core in [
        ForgeCapability::Issues,
        ForgeCapability::MergeRequests,
        ForgeCapability::Pipelines,
    ] {
        assert_eq!(capabilities.get(&core), Some(&CapabilityState::Unavailable));
    }
}

#[test]
fn one_github_capability_failure_preserves_healthy_capabilities() {
    let (caps, freshness, error) =
        crate::forge::core_observation_status(ForgeProviderKind::GitHub, true, false, true);
    assert_eq!(freshness, crate::forge::ForgeFreshness::Fresh);
    assert!(error.is_none());
    assert_eq!(
        caps.get(&ForgeCapability::MergeRequests),
        Some(&CapabilityState::Unavailable)
    );
    assert_eq!(
        caps.get(&ForgeCapability::Issues),
        Some(&CapabilityState::Available)
    );
}

#[test]
fn bounded_review_threads_fail_closed_when_pagination_is_needed() {
    let envelope: GitHubGraphQlEnvelope = serde_json::from_value(serde_json::json!({
        "data": {
            "repository": {
                "pullRequest": {
                    "reviewThreads": {
                        "nodes": [
                            {"isResolved": false},
                            {"isResolved": true}
                        ],
                        "pageInfo": {"hasNextPage": true}
                    }
                }
            }
        }
    }))
    .expect("fixture");
    let threads = envelope
        .data
        .repository
        .unwrap()
        .pull_request
        .unwrap()
        .review_threads;
    assert!(threads.page_info.has_next_page);
    assert_eq!(
        threads
            .nodes
            .iter()
            .filter(|thread| !thread.is_resolved)
            .count(),
        1
    );
}
