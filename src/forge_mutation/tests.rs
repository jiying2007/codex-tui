use super::*;

#[test]
fn confirmed_revision_is_frozen_for_both_providers_and_fails_closed() {
    const EXPECTED: &str = "0123456789abcdef0123456789abcdef01234567";
    const MOVED: &str = "fedcba9876543210fedcba9876543210fedcba98";
    for identity in [identity(), github_identity()] {
        let plan = ForgeMutationPlan::approve_merge_request(
            &identity,
            "/repo".into(),
            7,
            "feature".into(),
            "main".into(),
            1,
        )
        .unwrap();
        assert!(confirmed_head_sha(&plan).is_err());
        assert!(plan.clone().bind_confirmed_head("deadbeef").is_err());
        let frozen = plan.bind_confirmed_head(EXPECTED).unwrap();
        assert_eq!(confirmed_head_sha(&frozen).unwrap(), EXPECTED);
        assert!(require_confirmed_head(&frozen, EXPECTED).is_ok());
        assert!(require_confirmed_head(&frozen, MOVED).is_err());
        assert!(frozen.clone().bind_confirmed_head(MOVED).is_err());
        let mut duplicate = frozen.clone();
        duplicate
            .preconditions
            .push(precondition("confirmed-head-sha", MOVED));
        assert!(confirmed_head_sha(&duplicate).is_err());
        let encoded = serde_json::to_string(&frozen).unwrap();
        let restored: ForgeMutationPlan = serde_json::from_str(&encoded).unwrap();
        assert_eq!(confirmed_head_sha(&restored).unwrap(), EXPECTED);
    }
}

fn identity() -> ForgeIdentity {
    ForgeIdentity {
        provider: ForgeProviderKind::GitLab,
        host: "gitlab.example.com".into(),
        project_id: "42".into(),
        path_with_namespace: "team/repo".into(),
        web_url: "https://gitlab.example.com/team/repo".into(),
        default_branch: Some("main".into()),
    }
}

fn github_identity() -> ForgeIdentity {
    ForgeIdentity {
        provider: ForgeProviderKind::GitHub,
        host: "github.com".into(),
        project_id: "123".into(),
        path_with_namespace: "octo/repo".into(),
        web_url: "https://github.com/octo/repo".into(),
        default_branch: Some("main".into()),
    }
}

#[test]
fn mutation_coordinator_channels_are_bounded() {
    let source = include_str!("../forge_mutation.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    assert!(!production.contains("unbounded_channel"));
    assert!(!production.contains("UnboundedSender"));
    assert!(!production.contains("UnboundedReceiver"));
    assert!(production.contains("JoinSet"));
}

#[test]
fn mutation_command_queue_reports_backpressure() {
    let (tx, _rx) = mpsc::channel(1);
    queue_mutation_command(&tx, ForgeMutationCommand::Recover).expect("first command");
    let error = queue_mutation_command(&tx, ForgeMutationCommand::Recover)
        .expect_err("second command must hit bounded queue");
    assert!(error.to_string().contains("queue is full"));
}

#[test]
fn create_plan_is_exact_and_non_force() {
    let plan = ForgeMutationPlan::create_merge_request(
        &identity(),
        "/repo".into(),
        "feature/m6b".into(),
        "main".into(),
        "Ship M6b".into(),
        1,
    )
    .expect("plan");
    assert_eq!(plan.kind, ForgeMutationKind::CreateMergeRequest);
    assert_eq!(plan.source_branch.as_deref(), Some("feature/m6b"));
    assert_eq!(plan.target_branch.as_deref(), Some("main"));
    assert_eq!(plan.change_request_iid, None);
    assert!(plan.expected_side_effect.contains("feature/m6b"));
}

#[test]
fn comment_plan_persists_length_not_body() {
    let body = "please address this";
    let plan = ForgeMutationPlan::comment_merge_request(
        &identity(),
        "/repo".into(),
        7,
        "feature".into(),
        "main".into(),
        body.len(),
        1,
    )
    .expect("plan");
    let json = serde_json::to_string(&plan).expect("serialize");
    assert!(json.contains("\"payload_bytes\":19"));
    assert!(!json.contains(body));
}

#[test]
fn merge_plan_never_requests_force_or_bypass() {
    let plan = ForgeMutationPlan::merge_merge_request(
        &identity(),
        "/repo".into(),
        7,
        "feature".into(),
        "main".into(),
        1,
    )
    .expect("plan");
    assert_eq!(plan.kind, ForgeMutationKind::MergeMergeRequest);
    assert!(
        plan.preconditions
            .iter()
            .any(|item| item.key == "approval-rules-satisfied")
    );
    assert!(
        !plan
            .expected_side_effect
            .to_ascii_lowercase()
            .contains("force")
    );
}

#[test]
fn invalid_project_identity_fails_closed() {
    let mut identity = identity();
    identity.project_id = "group/repo".into();
    assert!(
        ForgeMutationPlan::approve_merge_request(
            &identity,
            "/repo".into(),
            7,
            "feature".into(),
            "main".into(),
            1,
        )
        .is_err()
    );
}
#[test]
fn branch_bound_gitlab_mutation_refuses_fork_or_unknown_source_project() {
    let plan = ForgeMutationPlan::merge_merge_request(
        &identity(),
        "/repo".into(),
        7,
        "feature".into(),
        "main".into(),
        1,
    )
    .expect("plan");
    let mut mr: GitLabMergeRequest = serde_json::from_value(serde_json::json!({
        "iid": 7,
        "title": "Cross project",
        "state": "opened",
        "source_branch": "feature",
        "target_branch": "main",
        "source_project_id": 42,
        "target_project_id": 42,
        "web_url": "https://gitlab.example.com/team/repo/-/merge_requests/7",
        "sha": "0123456789abcdef"
    }))
    .expect("MR fixture");
    assert!(is_own_project_mr(&mr, &plan).expect("source identity"));

    mr.source_project_id = Some(999);
    assert!(!is_own_project_mr(&mr, &plan).expect("fork identity"));
    mr.source_project_id = None;
    assert!(is_own_project_mr(&mr, &plan).is_err());
    mr.source_project_id = Some(42);
    mr.target_project_id = None;
    assert!(is_own_project_mr(&mr, &plan).is_err());
    mr.target_project_id = Some(999);
    assert!(is_own_project_mr(&mr, &plan).is_err());
}

#[test]
fn gitlab_mr_fixture_requires_exact_head_sha() {
    let mr: GitLabMergeRequest = serde_json::from_value(serde_json::json!({
        "iid": 7,
        "title": "Ship M6b",
        "state": "opened",
        "source_branch": "feature/m6b",
        "target_branch": "main",
        "web_url": "https://gitlab.example.com/team/repo/-/merge_requests/7",
        "sha": "0123456789abcdef",
        "draft": false,
        "detailed_merge_status": "mergeable",
        "blocking_discussions_resolved": true,
        "head_pipeline": { "status": "success" }
    }))
    .expect("decode GitLab MR fixture");

    assert_eq!(required_mr_sha(&mr).expect("head sha"), "0123456789abcdef");

    let missing: GitLabMergeRequest = serde_json::from_value(serde_json::json!({
        "iid": 8,
        "title": "Missing head",
        "state": "opened",
        "source_branch": "feature/missing",
        "target_branch": "main",
        "web_url": "https://gitlab.example.com/team/repo/-/merge_requests/8",
        "sha": null,
        "draft": false
    }))
    .expect("decode missing-sha fixture");
    assert!(required_mr_sha(&missing).is_err());
}

#[test]
fn approval_state_fixture_counts_only_unsatisfied_required_rules() {
    let state: GitLabApprovalState = serde_json::from_value(serde_json::json!({
        "approval_rules_overwritten": true,
        "rules": [
            { "approvals_required": 2, "approved": true },
            { "approvals_required": 1, "approved": false },
            { "approvals_required": 0, "approved": false }
        ]
    }))
    .expect("decode approval-state fixture");

    assert_eq!(unsatisfied_required_approval_rules(&state), 1);
}

#[test]
fn only_explicit_gitlab_mergeable_status_passes() {
    for status in [
        None,
        Some("checking"),
        Some("ci_still_running"),
        Some("not_approved"),
        Some("policies_denied"),
        Some("conflict"),
        Some("unchecked"),
        Some("can_be_merged"),
    ] {
        assert!(require_gitlab_merge_ready(status).is_err(), "{status:?}");
    }
    assert!(require_gitlab_merge_ready(Some("mergeable")).is_ok());
}

#[test]
fn gitlab_approval_fallback_rejects_missing_or_positive_counts() {
    for remaining in [None, Some(1), Some(10)] {
        assert!(
            require_gitlab_approval_fallback(&GitLabApprovals {
                approvals_left: remaining,
                approvals_required: None,
                approved: None,
                approved_by: vec![],
            })
            .is_err()
        );
    }
    assert!(
        require_gitlab_approval_fallback(&GitLabApprovals {
            approvals_left: Some(0),
            approvals_required: None,
            approved: None,
            approved_by: vec![],
        })
        .is_ok()
    );
}

#[test]
fn github_plans_reuse_confirmation_contract_and_provider_identity() {
    let create = ForgeMutationPlan::create_merge_request(
        &github_identity(),
        "/repo".into(),
        "feature/github".into(),
        "main".into(),
        "Ship GitHub support".into(),
        1,
    )
    .expect("GitHub create plan");
    assert_eq!(create.provider, ForgeProviderKind::GitHub);
    assert!(create.expected_side_effect.contains("GitHub pull request"));
    assert!(
        create
            .preconditions
            .iter()
            .any(|condition| { condition.key == "provider" && condition.expected == "github" })
    );

    let approve = ForgeMutationPlan::approve_merge_request(
        &github_identity(),
        "/repo".into(),
        7,
        "feature/github".into(),
        "main".into(),
        2,
    )
    .expect("GitHub approve plan");
    assert!(approve.preconditions.iter().any(|condition| {
        condition.key == "head-sha-revalidated" && condition.expected == "true"
    }));
}

#[test]
fn approve_and_merge_plans_advertise_head_sha_revalidation() {
    for plan in [
        ForgeMutationPlan::approve_merge_request(
            &identity(),
            "/repo".into(),
            7,
            "feature".into(),
            "main".into(),
            1,
        )
        .expect("approve plan"),
        ForgeMutationPlan::merge_merge_request(
            &identity(),
            "/repo".into(),
            7,
            "feature".into(),
            "main".into(),
            2,
        )
        .expect("merge plan"),
    ] {
        assert!(plan.preconditions.iter().any(|precondition| {
            precondition.key == "head-sha-revalidated" && precondition.expected == "true"
        }));
    }
}

#[test]
fn gitlab_approval_evidence_requires_explicit_fields_and_types() {
    for payload in [
        "{}",
        "{\"rules\":null}",
        "{\"rules\":[{\"approved\":false}]}",
        "{\"rules\":[{\"approvals_required\":1}]}",
        "{\"rules\":[{\"approvals_required\":null,\"approved\":true}]}",
        "{\"rules\":[{\"approvals_required\":0,\"approved\":\"true\"}]}",
        "{\"rules\":[{\"approvals_required\":-1,\"approved\":true}]}",
    ] {
        let error = serde_json::from_str::<GitLabApprovalState>(payload).unwrap_err();
        let wrapped = anyhow::Error::new(error).context("decode glab api response");
        assert!(
            wrapped.downcast_ref::<serde_json::Error>().is_some(),
            "{payload}"
        );
    }
    let empty: GitLabApprovalState = serde_json::from_str("{\"rules\":[]}").unwrap();
    assert_eq!(unsatisfied_required_approval_rules(&empty), 0);
    let pending: GitLabApprovalState =
        serde_json::from_str("{\"rules\":[{\"approvals_required\":1,\"approved\":false}]}")
            .unwrap();
    assert_eq!(unsatisfied_required_approval_rules(&pending), 1);
}

#[test]
fn gitlab_146_legacy_merge_readiness_is_version_bounded_and_conflict_explicit() {
    assert!(require_legacy_gitlab_merge_ready(
        "14.6.1",
        Some("can_be_merged"),
        Some(false),
    )
    .is_ok());
    assert!(require_legacy_gitlab_merge_ready(
        "15.5.9-ee",
        Some("can_be_merged"),
        Some(false),
    )
    .is_ok());

    for version in ["13.12.0", "15.6.0", "16.0.0", "latest", ""] {
        assert!(
            require_legacy_gitlab_merge_ready(
                version,
                Some("can_be_merged"),
                Some(false)
            )
            .is_err(),
            "{version}"
        );
    }
    for status in [None, Some("checking"), Some("cannot_be_merged")] {
        assert!(
            require_legacy_gitlab_merge_ready("14.6.1", status, Some(false)).is_err(),
            "{status:?}"
        );
    }
    for conflict in [None, Some(true)] {
        assert!(require_legacy_gitlab_merge_ready(
            "14.6.1",
            Some("can_be_merged"),
            conflict
        )
        .is_err());
    }

    // A modern non-mergeable detailed status must never be replaced by the
    // older, less expressive 'can_be_merged' field.
    assert!(require_gitlab_merge_ready(Some("not_approved")).is_err());
}

#[test]
fn gitlab_legacy_ce_approvals_accept_only_the_documented_free_shape() {
    let legacy = GitLabApprovals {
        approvals_left: None,
        approvals_required: None,
        approved: Some(false),
        approved_by: vec![],
    };
    assert!(require_legacy_gitlab_approval_fallback(&legacy).is_ok());
    // Modern fallback cannot infer zero approvals from missing EE fields.
    assert!(require_gitlab_approval_fallback(&legacy).is_err());

    for invalid in [
        GitLabApprovals {
            approved: None,
            ..legacy.clone()
        },
        GitLabApprovals {
            approved: Some(true),
            ..legacy.clone()
        },
        GitLabApprovals {
            approvals_required: Some(1),
            ..legacy.clone()
        },
        GitLabApprovals {
            approvals_left: Some(1),
            ..legacy.clone()
        },
    ] {
        assert!(require_legacy_gitlab_approval_fallback(&invalid).is_err());
    }
    assert!(require_legacy_gitlab_approval_fallback(&GitLabApprovals {
        approvals_left: Some(0),
        approvals_required: Some(0),
        approved: Some(true),
        approved_by: vec![],
    })
    .is_ok());
}

#[test]
fn legacy_rules_fallback_rejects_access_denied_and_ambiguous_errors() {
    assert!(legacy_gitlab_rules_unavailable(&anyhow::anyhow!(
        "glab api /approval_state failed: 404 Not Found"
    )));
    for error in [
        "glab api failed: 403 Forbidden",
        "connection timed out",
        "decode glab api response",
        "401 Unauthorized",
    ] {
        assert!(!legacy_gitlab_rules_unavailable(&anyhow::anyhow!("{error}")));
    }
}

#[test]
fn gitlab_146_response_decodes_without_newer_status_or_ee_approval_fields() {
    let mr: GitLabMergeRequest = serde_json::from_value(serde_json::json!({
        "iid": 1,
        "title": "v1.4 test",
        "state": "opened",
        "source_branch": "qualification/tui-v14-device",
        "target_branch": "master",
        "source_project_id": 366,
        "target_project_id": 366,
        "web_url": "http://gitlab.test/embedded/test/-/merge_requests/1",
        "sha": "5372e2f6ba57934217b9330cfccec51d680c1ca9",
        "merge_status": "can_be_merged",
        "detailed_merge_status": null,
        "has_conflicts": false,
        "draft": false,
        "blocking_discussions_resolved": true,
        "head_pipeline": null
    }))
    .expect("decode legacy MR");
    assert_eq!(mr.detailed_merge_status, None);
    require_legacy_gitlab_merge_ready(
        "14.6.1",
        mr.merge_status.as_deref(),
        mr.has_conflicts
    )
    .expect("legacy merge readiness");

    let approvals: GitLabApprovals = serde_json::from_value(serde_json::json!({
        "approved": false,
        "approved_by": []
    }))
    .expect("decode legacy CE approvals");
    require_legacy_gitlab_approval_fallback(&approvals).expect("legacy CE approvals");
}
