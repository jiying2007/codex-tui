use super::*;

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
            gitlab_observation_status(issues, merge_requests, pipelines);
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
    let (capabilities, freshness, error) = gitlab_observation_status(false, false, false);
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
    let (capabilities, freshness, error) = gitlab_observation_status(true, true, true);
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
