use super::*;


#[test]
fn fork_and_unverified_gitlab_mr_sources_never_match_a_local_branch() {
    let make = |source: serde_json::Value, target: serde_json::Value| {
        serde_json::from_value::<GitLabMergeRequest>(serde_json::json!({
            "iid": 7,
            "title": "Cross-project MR",
            "state": "opened",
            "source_branch": "feature",
            "target_branch": "main",
            "source_project_id": source,
            "target_project_id": target,
            "web_url": "https://gitlab.example.com/team/repo/-/merge_requests/7"
        }))
        .expect("GitLab MR fixture")
    };
    assert_eq!(make(42.into(), 42.into()).branch_for_local_projection("42"), "feature");
    assert_eq!(
        make(43.into(), 42.into()).branch_for_local_projection("42"),
        "project/43:feature"
    );
    assert_eq!(
        make(serde_json::Value::Null, 42.into()).branch_for_local_projection("42"),
        "unverified-source:feature"
    );
    assert_eq!(
        make(42.into(), 100.into()).branch_for_local_projection("42"),
        "project/42:feature"
    );
}

#[test]
fn a_single_failed_gitlab_endpoint_does_not_discard_healthy_data_capabilities() {
    for (issues, merge_requests, pipelines) in [
        (false, true, true),
        (true, false, true),
        (true, true, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let (capabilities, freshness, error) =
            core_observation_status(ForgeProviderKind::GitLab, issues, merge_requests, pipelines);
        assert_eq!(freshness, ForgeFreshness::Fresh);
        assert!(
            error.is_none(),
            "partially healthy project must remain usable"
        );
        for (kind, healthy) in [
            (ForgeCapability::Issues, issues),
            (ForgeCapability::MergeRequests, merge_requests),
            (ForgeCapability::Pipelines, pipelines),
        ] {
            let expected = if healthy {
                CapabilityState::Available
            } else {
                CapabilityState::Unavailable
            };
            assert_eq!(capabilities.get(&kind), Some(&expected));
        }
        assert_eq!(
            capabilities.get(&ForgeCapability::IssueBoards),
            Some(&CapabilityState::Unknown),
            "unprobed feature must not be reported as available"
        );
    }
}

#[test]
fn all_failed_gitlab_data_endpoints_mark_the_projection_unavailable() {
    let (capabilities, freshness, error) =
        core_observation_status(ForgeProviderKind::GitLab, false, false, false);
    assert_eq!(freshness, ForgeFreshness::Unavailable);
    assert!(error.is_some());
    for capability in [
        ForgeCapability::Issues,
        ForgeCapability::MergeRequests,
        ForgeCapability::Pipelines,
    ] {
        assert_eq!(
            capabilities.get(&capability),
            Some(&CapabilityState::Unavailable)
        );
    }
}

#[test]
fn all_healthy_gitlab_data_endpoints_remain_fresh() {
    let (capabilities, freshness, error) =
        core_observation_status(ForgeProviderKind::GitLab, true, true, true);
    assert_eq!(freshness, ForgeFreshness::Fresh);
    assert!(error.is_none());
    for capability in [
        ForgeCapability::Issues,
        ForgeCapability::MergeRequests,
        ForgeCapability::Pipelines,
    ] {
        assert_eq!(
            capabilities.get(&capability),
            Some(&CapabilityState::Available)
        );
    }
}

#[tokio::test]
async fn exactly_full_review_page_requires_another_page() {
    let pages = bounded_review_pages(|number| async move {
        Ok::<Vec<u16>, anyhow::Error>(if number == 1 {
            vec![1; REVIEW_PAGE_SIZE]
        } else {
            vec![2]
        })
    })
    .await
    .expect("complete review pages");
    assert_eq!(pages.len(), REVIEW_PAGE_SIZE + 1);
}

#[tokio::test]
async fn exhausted_review_page_budget_is_not_a_complete_count() {
    let error = bounded_review_pages(|_| async {
        Ok::<Vec<u16>, anyhow::Error>(vec![1; REVIEW_PAGE_SIZE])
    })
    .await
    .expect_err("exceeded cap must fail");
    assert!(error.to_string().contains("pagination budget"));
}

#[tokio::test]
async fn failed_later_page_discards_previous_partial_result() {
    let error = bounded_review_pages(|number| async move {
        if number == 1 {
            Ok::<Vec<u16>, anyhow::Error>(vec![1; REVIEW_PAGE_SIZE])
        } else {
            anyhow::bail!("subsequent page unavailable")
        }
    })
    .await
    .expect_err("partial result must be unavailable");
    assert!(error.to_string().contains("subsequent page unavailable"));
}

#[test]
fn headless_and_ui_see_partial_core_data_as_degraded() {
    let mut observation = ForgeObservation::pending(ThreadId::new("probe"), "repo".into());
    for core in [
        ForgeCapability::Issues,
        ForgeCapability::MergeRequests,
        ForgeCapability::Pipelines,
    ] {
        observation
            .capabilities
            .insert(core, CapabilityState::Available);
    }
    assert!(!observation.core_data_incomplete());
    observation
        .capabilities
        .insert(ForgeCapability::MergeRequests, CapabilityState::Unavailable);
    assert!(observation.core_data_incomplete());
}
