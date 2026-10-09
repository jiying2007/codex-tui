use super::*;

#[derive(Clone)]
struct SlowProbeProvider {
    active: Arc<std::sync::atomic::AtomicUsize>,
    max_active: Arc<std::sync::atomic::AtomicUsize>,
    delay: Duration,
}

impl SlowProbeProvider {
    fn record_enter(
        active: &std::sync::atomic::AtomicUsize,
        max_active: &std::sync::atomic::AtomicUsize,
    ) {
        let current = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        max_active.fetch_max(current, std::sync::atomic::Ordering::SeqCst);
    }
}

impl ForgeProvider for SlowProbeProvider {
    fn probe<'a>(&'a self, thread_id: ThreadId, cwd: String) -> ForgeFuture<'a, ForgeObservation> {
        let active = Arc::clone(&self.active);
        let max_active = Arc::clone(&self.max_active);
        let delay = self.delay;
        Box::pin(async move {
            Self::record_enter(&active, &max_active);
            tokio::time::sleep(delay).await;
            active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            ForgeObservation::unavailable(thread_id, cwd, "fixture")
        })
    }

    fn probe_review<'a>(
        &'a self,
        target: ForgeReviewTarget,
    ) -> ForgeFuture<'a, ForgeReviewSummary> {
        Box::pin(async move {
            ForgeReviewSummary {
                thread_id: target.thread_id,
                cwd: target.cwd,
                change_request_iid: target.change_request_iid,
                approvals_required: None,
                approvals_left: None,
                approved_by_count: 0,
                changes_requested_by_count: 0,
                discussions_total: 0,
                unresolved_discussions: 0,
                approvals_available: false,
                discussions_available: false,
                observed_at_unix_ms: now_unix_ms(),
                error: Some("fixture".into()),
            }
        })
    }
}

#[tokio::test]
async fn forge_command_deadline_fails_closed_without_hanging() {
    let error = with_command_deadline::<()>(
        "fixture-forge",
        Duration::from_millis(5),
        std::future::pending(),
    )
    .await
    .expect_err("pending forge command must time out");
    assert!(
        error.to_string().contains("fixture-forge timed out"),
        "timeout must retain command context: {error:#}"
    );
}

#[test]
fn unauthenticated_custom_forge_host_fails_closed() {
    let error = provider_from_auth_state("git.internal.example", false, false)
        .expect_err("custom host without auth must be unavailable");
    assert!(error.to_string().contains("authenticate with gh or glab"));
}

#[tokio::test]
async fn forge_actor_runs_bounded_concurrent_probes() {
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let max_active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let provider = Arc::new(SlowProbeProvider {
        active,
        max_active: Arc::clone(&max_active),
        delay: Duration::from_millis(40),
    });
    let mut handle = ForgeHandle::start_with_provider(provider);

    for index in 0..8 {
        handle
            .probe(
                ThreadId::new(format!("thread-{index}")),
                format!("/repo/{index}"),
            )
            .expect("queue probe");
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut received = 0;
    while received < 8 && tokio::time::Instant::now() < deadline {
        while handle.try_recv().is_some() {
            received += 1;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    assert_eq!(received, 8);
    let observed = max_active.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        observed > 1,
        "actor must execute independent probes concurrently"
    );
    assert!(
        observed <= FORGE_MAX_CONCURRENCY,
        "actor exceeded concurrency bound: {observed}"
    );
}

#[tokio::test]
async fn forge_actor_command_queue_applies_backpressure() {
    let provider = Arc::new(SlowProbeProvider {
        active: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        max_active: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        delay: Duration::from_secs(1),
    });
    let handle = ForgeHandle::start_with_provider(provider);

    let mut queue_full = false;
    for index in 0..(FORGE_COMMAND_QUEUE_CAPACITY + FORGE_MAX_CONCURRENCY + 16) {
        let result = handle.probe(
            ThreadId::new(format!("backpressure-{index}")),
            format!("/repo/backpressure/{index}"),
        );
        if result.is_err() {
            queue_full = true;
            break;
        }
    }

    assert!(
        queue_full,
        "bounded Forge command queue must apply backpressure"
    );
}

#[test]
fn provider_routing_is_explicit_and_custom_hosts_use_auth_authority() {
    assert_eq!(
        canonical_provider_for_host("github.com"),
        Some(ForgeProviderKind::GitHub)
    );
    assert_eq!(
        canonical_provider_for_host("GITHUB.COM"),
        Some(ForgeProviderKind::GitHub)
    );
    assert_eq!(
        canonical_provider_for_host("gitlab.com"),
        Some(ForgeProviderKind::GitLab)
    );
    assert_eq!(canonical_provider_for_host("git.internal.example"), None);

    assert_eq!(
        provider_from_auth_state("git.internal.example", true, false).unwrap(),
        ForgeProviderKind::GitHub
    );
    assert_eq!(
        provider_from_auth_state("git.internal.example", false, true).unwrap(),
        ForgeProviderKind::GitLab
    );
    assert!(provider_from_auth_state("git.internal.example", false, false).is_err());
    assert!(provider_from_auth_state("git.internal.example", true, true).is_err());
}

#[test]
fn external_refs_are_provider_specific_behind_forge_identity() {
    let gitlab = ForgeIdentity {
        provider: ForgeProviderKind::GitLab,
        host: "gitlab.example.com".into(),
        project_id: "42".into(),
        path_with_namespace: "team/repo".into(),
        web_url: "https://gitlab.example.com/team/repo".into(),
        default_branch: Some("main".into()),
    };
    assert_eq!(
        gitlab.issue_source_ref(12),
        "gitlab://gitlab.example.com/projects/42/issues/12"
    );
    assert_eq!(
        gitlab.change_request_source_ref(7),
        "gitlab://gitlab.example.com/projects/42/merge-requests/7"
    );

    let github = ForgeIdentity {
        provider: ForgeProviderKind::GitHub,
        host: "github.com".into(),
        project_id: "99".into(),
        path_with_namespace: "owner/repo".into(),
        web_url: "https://github.com/owner/repo".into(),
        default_branch: Some("main".into()),
    };
    assert_eq!(
        github.issue_source_ref(12),
        "github://github.com/repositories/99/issues/12"
    );
    assert_eq!(
        github.change_request_source_ref(7),
        "github://github.com/repositories/99/pull-requests/7"
    );
}

#[test]
fn forge_remote_display_redacts_url_credentials_query_and_fragments() {
    let redacted = redact_git_remote_url(
        "https://alice:never-print@internal.example/group/repo.git?access_token=secret#opaque",
    )
    .expect("redact password-bearing remote");
    assert!(redacted.starts_with("https://internal.example/"));
    for sensitive in ["alice", "never-print", "secret", "opaque", "access_token"] {
        assert!(!redacted.contains(sensitive), "remote leaked {sensitive}");
    }
    assert_eq!(
        redact_git_remote_url("git@internal.example:team/repo.git").unwrap(),
        "internal.example:team/repo.git"
    );
    assert_eq!(
        redact_git_remote_url("ssh://git@internal.example:2222/group/repo.git").unwrap(),
        "ssh://internal.example:2222/group/repo.git"
    );
}

#[test]
fn parses_https_ssh_and_scp_remote_urls() {
    assert_eq!(
        parse_git_remote_url("https://gitlab.example.com/group/sub/project.git"),
        Some(("gitlab.example.com".into(), "group/sub/project".into()))
    );
    assert_eq!(
        parse_git_remote_url("ssh://git@gitlab.example.com:2222/group/project.git"),
        Some(("gitlab.example.com".into(), "group/project".into()))
    );
    assert_eq!(
        parse_git_remote_url("git@gitlab.example.com:group/project.git"),
        Some(("gitlab.example.com".into(), "group/project".into()))
    );
}

#[test]
fn remote_selection_prefers_branch_then_origin_then_unique() {
    let remotes = vec![
        ("origin".into(), "git@example/a/repo.git".into()),
        ("upstream".into(), "git@example/b/repo.git".into()),
    ];
    assert_eq!(
        select_remote(&remotes, Some("upstream")).unwrap().0,
        "upstream"
    );
    assert_eq!(select_remote(&remotes, None).unwrap().0, "origin");

    let unique = vec![("company".into(), "git@example/a/repo.git".into())];
    assert_eq!(select_remote(&unique, None).unwrap().0, "company");
}

#[test]
fn ambiguous_remote_selection_fails_closed() {
    let remotes = vec![
        ("left".into(), "git@example/a/repo.git".into()),
        ("right".into(), "git@example/b/repo.git".into()),
    ];
    assert!(select_remote(&remotes, None).is_err());
}

#[test]
fn project_path_encoding_is_gitlab_rest_safe() {
    assert_eq!(
        percent_encode_project_path("group/sub project/repo"),
        "group%2Fsub%20project%2Frepo"
    );
}

#[test]
fn change_request_and_pipeline_match_exact_branch_only() {
    let observation = ForgeObservation {
        thread_id: ThreadId::new("t"),
        cwd: "/repo".into(),
        remote_name: Some("origin".into()),
        remote_url: Some("git@example:a/repo.git".into()),
        identity: None,
        capabilities: default_capabilities(),
        issues: vec![],
        change_requests: vec![ChangeRequestSummary {
            iid: 7,
            title: "MR".into(),
            state: "opened".into(),
            source_branch: "feature/a".into(),
            target_branch: "main".into(),
            web_url: "https://example/mr/7".into(),
            updated_at: None,
            draft: false,
            detailed_merge_status: None,
            blocking_discussions_resolved: None,
        }],
        pipelines: vec![PipelineSummary {
            id: 9,
            status: "failed".into(),
            reference: "feature/a".into(),
            web_url: "https://example/pipelines/9".into(),
            updated_at: None,
        }],
        overview_source_page_saturated: false,
        review: None,
        observed_at_unix_ms: 1,
        freshness: ForgeFreshness::Fresh,
        error: None,
    };

    assert!(observation.change_request_for_branch("feature/a").is_some());
    assert!(observation.change_request_for_branch("feature").is_none());
    assert!(observation.pipeline_for_branch("feature/a").is_some());
}
#[test]
fn freshness_ages_and_unavailable_observations_never_look_fresh() {
    let mut observation = ForgeObservation {
        thread_id: ThreadId::new("t"),
        cwd: "/repo".into(),
        remote_name: Some("origin".into()),
        remote_url: Some("git@gitlab.example.com:team/repo.git".into()),
        identity: Some(ForgeIdentity {
            provider: ForgeProviderKind::GitLab,
            host: "gitlab.example.com".into(),
            project_id: "42".into(),
            path_with_namespace: "team/repo".into(),
            web_url: "https://gitlab.example.com/team/repo".into(),
            default_branch: Some("main".into()),
        }),
        capabilities: default_capabilities(),
        issues: vec![],
        change_requests: vec![],
        pipelines: vec![],
        overview_source_page_saturated: false,
        review: None,
        observed_at_unix_ms: 100,
        freshness: ForgeFreshness::Fresh,
        error: None,
    };

    assert_eq!(observation.freshness_at(10_100), ForgeFreshness::Fresh);
    assert_eq!(observation.freshness_at(10_101), ForgeFreshness::Aging);
    assert_eq!(observation.freshness_at(60_101), ForgeFreshness::Stale);

    observation.error = Some("offline".into());
    assert_eq!(observation.freshness_at(101), ForgeFreshness::Unavailable);
}
