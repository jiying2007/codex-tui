use super::*;

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
                approved_by: vec![],
            })
            .is_err()
        );
    }
    assert!(
        require_gitlab_approval_fallback(&GitLabApprovals {
            approvals_left: Some(0),
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
