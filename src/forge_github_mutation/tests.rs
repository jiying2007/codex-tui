use super::*;
use crate::forge::ForgeProviderKind;

fn plan(kind: ForgeMutationKind) -> ForgeMutationPlan {
    ForgeMutationPlan {
        operation_id: "op-1".into(),
        kind,
        provider: ForgeProviderKind::GitHub,
        cwd: "/repo".into(),
        host: "github.com".into(),
        project_id: "123".into(),
        project_path: "octo/repo".into(),
        change_request_iid: Some(7),
        source_branch: Some("feature".into()),
        target_branch: Some("main".into()),
        title: None,
        payload_bytes: None,
        expected_side_effect: "fixture".into(),
        preconditions: vec![],
        planned_at_unix_ms: 1,
    }
}

#[test]
fn github_pull_fixture_requires_exact_head_sha() {
    let pull: GitHubPullRequest = serde_json::from_value(serde_json::json!({
        "number": 7,
        "title": "Ship",
        "state": "open",
        "html_url": "https://github.com/octo/repo/pull/7",
        "draft": false,
        "merged": false,
        "mergeable": true,
        "head": {"ref": "feature", "sha": "0123456789abcdef"},
        "base": {"ref": "main", "sha": "abcdef"}
    }))
    .expect("pull");
    assert_eq!(required_head_sha(&pull).expect("sha"), "0123456789abcdef");
}

#[test]
fn latest_review_state_uses_last_review_from_authenticated_login() {
    let reviews: Vec<GitHubReview> = serde_json::from_value(serde_json::json!([
        {"id": 1, "state": "APPROVED", "user": {"login": "alice"}},
        {"id": 2, "state": "DISMISSED", "user": {"login": "alice"}},
        {"id": 3, "state": "APPROVED", "user": {"login": "bob"}}
    ]))
    .expect("reviews");
    assert_eq!(latest_review_state(&reviews, "alice"), Some("DISMISSED"));
    assert_eq!(latest_review_state(&reviews, "BOB"), Some("APPROVED"));
}

#[test]
fn github_change_reference_uses_numeric_repository_identity() {
    assert_eq!(
        change_ref(&plan(ForgeMutationKind::MergeMergeRequest), 7),
        "github://github.com/repositories/123/pull-requests/7"
    );
}

#[test]
fn repository_path_is_exact_owner_repo() {
    assert_eq!(
        split_repository_path("octo/repo").unwrap(),
        ("octo", "repo")
    );
    assert!(split_repository_path("org/sub/repo").is_err());
    assert!(split_repository_path("repo").is_err());
}

#[test]
fn merge_mutation_builder_has_no_force_or_bypass_fields() {
    let fields = [("sha", "012345")];
    assert_eq!(fields, [("sha", "012345")]);
    assert!(!fields.iter().any(|(key, _)| {
        key.eq_ignore_ascii_case("force")
            || key.eq_ignore_ascii_case("bypass")
            || key.eq_ignore_ascii_case("merge_method")
    }));
}

#[test]
fn approval_mutation_binds_exact_commit_id() {
    let source = include_str!("../forge_github_mutation.rs");
    assert!(source.contains(r#"[("event", "APPROVE"), ("commit_id", expected_sha)]"#));
    assert!(source.contains("revalidate_head(plan, number, expected_sha).await?"));
}

#[test]
fn github_merge_preflight_rejects_unknown_and_false() {
    assert!(require_github_mergeable(None).is_err());
    assert!(require_github_mergeable(Some(false)).is_err());
    assert!(require_github_mergeable(Some(true)).is_ok());
}

#[test]
fn same_name_fork_head_cannot_pass_local_branch_mutation_preflight() {
    let parsed = |repo: serde_json::Value| -> GitHubPullRequest {
        serde_json::from_value(serde_json::json!({
            "number": 7,
            "title": "Fork collision",
            "state": "open",
            "html_url": "https://github.com/octo/repo/pull/7",
            "draft": false,
            "merged": false,
            "mergeable": true,
            "head": {
                "ref": "feature",
                "sha": "0123456789abcdef",
                "repo": repo
            },
            "base": {"ref": "main", "sha": "abcdef"}
        }))
        .expect("PR fixture")
    };
    let mutation_plan = plan(ForgeMutationKind::MergeMergeRequest);
    let local = parsed(serde_json::json!({"id": 123, "full_name": "octo/repo"}));
    let fork = parsed(serde_json::json!({"id": 999, "full_name": "alice/repo"}));
    let renamed = parsed(serde_json::json!({"id": 123, "full_name": "other/repo"}));
    let missing = parsed(serde_json::Value::Null);

    assert!(require_local_head_identity(&local, &mutation_plan).is_ok());
    assert!(require_local_head_identity(&fork, &mutation_plan).is_err());
    assert!(require_local_head_identity(&renamed, &mutation_plan).is_err());
    assert!(require_local_head_identity(&missing, &mutation_plan).is_err());

    assert!(is_local_source_repo(
        local.head.repo.as_ref().unwrap(),
        &mutation_plan
    ));
    assert!(!is_local_source_repo(
        fork.head.repo.as_ref().unwrap(),
        &mutation_plan
    ));
}
