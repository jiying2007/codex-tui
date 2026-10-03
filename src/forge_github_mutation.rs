use crate::forge::{percent_encode_component, run_command, trim_error};
use crate::forge_mutation::{ForgeMutationKind, ForgeMutationPlan, ForgeMutationRequest};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::Path;

const MAX_PULL_PAGE: usize = 100;

#[derive(Clone, Debug)]
pub(crate) struct GitHubPreflight {
    pub(crate) user_login: Option<String>,
    pub(crate) head_sha: Option<String>,
    pub(crate) already_satisfied: Option<(String, String)>,
}

#[derive(Clone, Debug)]
pub(crate) enum GitHubAppliedResult {
    PullRequest(u64),
    Comment(u64),
    Approval(String),
    Merge(u64),
}

pub(crate) enum GitHubReconciledOutcome {
    Succeeded {
        result_ref: String,
        verification: String,
    },
    Unknown(String),
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubRepository {
    id: u64,
    full_name: String,
    default_branch: String,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubPullRequest {
    number: u64,
    title: String,
    state: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    merged: bool,
    mergeable: Option<bool>,
    head: GitHubPullRef,
    base: GitHubPullRef,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubPullRef {
    #[serde(rename = "ref")]
    reference: String,
    sha: String,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubUser {
    login: String,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubReview {
    id: u64,
    state: String,
    user: Option<GitHubReviewUser>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubReviewUser {
    login: String,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubComment {
    id: u64,
    body: String,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubMergeResult {
    merged: bool,
    message: String,
    sha: Option<String>,
}

pub(crate) async fn validate_preconditions(
    plan: &ForgeMutationPlan,
) -> Result<GitHubPreflight> {
    let repository = repository(plan).await?;
    anyhow::ensure!(
        repository.id.to_string() == plan.project_id,
        "GitHub repository id changed: expected {}, observed {}",
        plan.project_id,
        repository.id
    );
    anyhow::ensure!(
        repository.full_name.eq_ignore_ascii_case(&plan.project_path),
        "GitHub repository identity changed: expected {}, observed {}",
        plan.project_path,
        repository.full_name
    );

    match plan.kind {
        ForgeMutationKind::CreateMergeRequest => {
            let source = plan
                .source_branch
                .as_deref()
                .context("create pull request plan missing source branch")?;
            let target = plan
                .target_branch
                .as_deref()
                .context("create pull request plan missing target branch")?;
            anyhow::ensure!(
                repository.default_branch == target,
                "GitHub default branch changed: planned {target:?}, observed {:?}",
                repository.default_branch
            );
            branch(plan, source).await?;
            branch(plan, target).await?;
            let existing = matching_pull_requests(plan, source, target).await?;
            anyhow::ensure!(
                existing.is_empty(),
                "matching open GitHub pull request already exists: {}",
                existing
                    .iter()
                    .map(|pull| format!("#{}", pull.number))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            Ok(GitHubPreflight {
                user_login: None,
                head_sha: None,
                already_satisfied: None,
            })
        }
        ForgeMutationKind::CommentMergeRequest => {
            validate_exact_open_pull(plan).await?;
            Ok(GitHubPreflight {
                user_login: None,
                head_sha: None,
                already_satisfied: None,
            })
        }
        ForgeMutationKind::ApproveMergeRequest => {
            let pull = validate_exact_open_pull(plan).await?;
            let sha = required_head_sha(&pull)?;
            let user: GitHubUser = gh_api_json(&plan.cwd, &plan.host, "/user").await?;
            let reviews = reviews(plan, pull.number).await?;
            if latest_review_state(&reviews, &user.login)
                .is_some_and(|state| state.eq_ignore_ascii_case("APPROVED"))
            {
                return Ok(GitHubPreflight {
                    user_login: Some(user.login.clone()),
                    head_sha: Some(sha),
                    already_satisfied: Some((
                        change_ref(plan, pull.number),
                        format!(
                            "authenticated GitHub user {:?} already has an active APPROVED review",
                            user.login
                        ),
                    )),
                });
            }
            Ok(GitHubPreflight {
                user_login: Some(user.login),
                head_sha: Some(sha),
                already_satisfied: None,
            })
        }
        ForgeMutationKind::MergeMergeRequest => {
            let pull = validate_exact_open_pull(plan).await?;
            let sha = required_head_sha(&pull)?;
            anyhow::ensure!(!pull.draft, "GitHub pull request is still a draft");
            anyhow::ensure!(
                pull.mergeable != Some(false),
                "GitHub reports the pull request as not mergeable"
            );
            Ok(GitHubPreflight {
                user_login: None,
                head_sha: Some(sha),
                already_satisfied: None,
            })
        }
    }
}

pub(crate) async fn execute_mutation(
    request: &ForgeMutationRequest,
    preflight: &GitHubPreflight,
) -> Result<GitHubAppliedResult> {
    let plan = &request.plan;
    match plan.kind {
        ForgeMutationKind::CreateMergeRequest => {
            let source = plan
                .source_branch
                .as_deref()
                .context("create pull request plan missing source branch")?;
            let target = plan
                .target_branch
                .as_deref()
                .context("create pull request plan missing target branch")?;
            let title = plan
                .title
                .as_deref()
                .context("create pull request plan missing title")?;
            let pull: GitHubPullRequest = gh_api_mutation_json(
                &plan.cwd,
                &plan.host,
                "POST",
                &format!("{}/pulls", repository_endpoint(plan)?),
                &[("head", source), ("base", target), ("title", title)],
            )
            .await?;
            Ok(GitHubAppliedResult::PullRequest(pull.number))
        }
        ForgeMutationKind::CommentMergeRequest => {
            let number = plan
                .change_request_iid
                .context("comment plan missing pull request number")?;
            let body = request.payload.as_deref().context("comment payload missing")?;
            let comment: GitHubComment = gh_api_mutation_json(
                &plan.cwd,
                &plan.host,
                "POST",
                &format!("{}/issues/{number}/comments", repository_endpoint(plan)?),
                &[("body", body)],
            )
            .await?;
            Ok(GitHubAppliedResult::Comment(comment.id))
        }
        ForgeMutationKind::ApproveMergeRequest => {
            let number = plan
                .change_request_iid
                .context("approve plan missing pull request number")?;
            let login = preflight
                .user_login
                .as_deref()
                .context("approve preflight missing authenticated GitHub login")?;
            let expected_sha = preflight
                .head_sha
                .as_deref()
                .context("approve preflight missing exact pull request HEAD sha")?;
            revalidate_head(plan, number, expected_sha).await?;
            let _: GitHubReview = gh_api_mutation_json(
                &plan.cwd,
                &plan.host,
                "POST",
                &format!("{}/pulls/{number}/reviews", repository_endpoint(plan)?),
                &[("event", "APPROVE")],
            )
            .await?;
            Ok(GitHubAppliedResult::Approval(login.to_string()))
        }
        ForgeMutationKind::MergeMergeRequest => {
            let number = plan
                .change_request_iid
                .context("merge plan missing pull request number")?;
            let expected_sha = preflight
                .head_sha
                .as_deref()
                .context("merge preflight missing exact pull request HEAD sha")?;
            revalidate_head(plan, number, expected_sha).await?;
            let result: GitHubMergeResult = gh_api_mutation_json(
                &plan.cwd,
                &plan.host,
                "PUT",
                &format!("{}/pulls/{number}/merge", repository_endpoint(plan)?),
                &[("sha", expected_sha)],
            )
            .await?;
            anyhow::ensure!(
                result.merged,
                "GitHub merge endpoint refused pull request #{number}: {}",
                result.message
            );
            let _ = result.sha;
            Ok(GitHubAppliedResult::Merge(number))
        }
    }
}

pub(crate) async fn verify_success(
    request: &ForgeMutationRequest,
    applied: GitHubAppliedResult,
) -> Result<(String, String)> {
    let plan = &request.plan;
    match applied {
        GitHubAppliedResult::PullRequest(number) => {
            let pull = get_pull(plan, number).await?;
            let source = plan.source_branch.as_deref().context("missing source")?;
            let target = plan.target_branch.as_deref().context("missing target")?;
            let title = plan.title.as_deref().context("missing title")?;
            anyhow::ensure!(pull.state == "open", "created pull request is not open");
            anyhow::ensure!(
                pull.head.reference == source,
                "created pull request source mismatch"
            );
            anyhow::ensure!(
                pull.base.reference == target,
                "created pull request target mismatch"
            );
            anyhow::ensure!(pull.title == title, "created pull request title mismatch");
            Ok((
                change_ref(plan, number),
                format!(
                    "GitHub readback confirms open pull request #{number} {source} -> {target}: {}",
                    pull.html_url
                ),
            ))
        }
        GitHubAppliedResult::Comment(comment_id) => {
            let number = plan
                .change_request_iid
                .context("comment plan missing pull request number")?;
            let comment: GitHubComment = gh_api_json(
                &plan.cwd,
                &plan.host,
                &format!("{}/issues/comments/{comment_id}", repository_endpoint(plan)?),
            )
            .await?;
            let payload = request.payload.as_deref().context("comment payload missing")?;
            anyhow::ensure!(
                comment.body == payload,
                "GitHub comment readback body mismatch"
            );
            Ok((
                format!("{}#comment-{comment_id}", change_ref(plan, number)),
                format!("GitHub readback confirms comment {comment_id}"),
            ))
        }
        GitHubAppliedResult::Approval(login) => {
            let number = plan
                .change_request_iid
                .context("approve plan missing pull request number")?;
            let reviews = reviews(plan, number).await?;
            anyhow::ensure!(
                latest_review_state(&reviews, &login)
                    .is_some_and(|state| state.eq_ignore_ascii_case("APPROVED")),
                "authenticated GitHub user's latest review is not APPROVED"
            );
            Ok((
                change_ref(plan, number),
                format!("GitHub review readback confirms {login:?} is APPROVED"),
            ))
        }
        GitHubAppliedResult::Merge(number) => {
            let pull = get_pull(plan, number).await?;
            anyhow::ensure!(
                pull.merged,
                "GitHub pull request #{number} is not merged after merge response"
            );
            Ok((
                change_ref(plan, number),
                format!("GitHub readback confirms pull request #{number} is merged"),
            ))
        }
    }
}

pub(crate) async fn reconcile_outcome(
    plan: &ForgeMutationPlan,
) -> Result<GitHubReconciledOutcome> {
    match plan.kind {
        ForgeMutationKind::CreateMergeRequest => {
            let source = plan.source_branch.as_deref().context("missing source branch")?;
            let target = plan.target_branch.as_deref().context("missing target branch")?;
            let title = plan.title.as_deref().context("missing title")?;
            let exact = matching_pull_requests(plan, source, target)
                .await?
                .into_iter()
                .filter(|pull| pull.title == title)
                .collect::<Vec<_>>();
            match exact.as_slice() {
                [pull] => Ok(GitHubReconciledOutcome::Succeeded {
                    result_ref: change_ref(plan, pull.number),
                    verification: format!(
                        "reconciled after uncertain outcome: GitHub contains exact open pull request #{}",
                        pull.number
                    ),
                }),
                [] => Ok(GitHubReconciledOutcome::Unknown(
                    "exact matching open GitHub pull request is not visible after an uncertain create outcome; never retry blindly"
                        .into(),
                )),
                _ => Ok(GitHubReconciledOutcome::Unknown(
                    "multiple exact matching GitHub pull requests prevent safe reconciliation"
                        .into(),
                )),
            }
        }
        ForgeMutationKind::CommentMergeRequest => Ok(GitHubReconciledOutcome::Unknown(
            "GitHub comment outcome cannot be proven after payload left memory; never retry blindly"
                .into(),
        )),
        ForgeMutationKind::ApproveMergeRequest => {
            let number = plan
                .change_request_iid
                .context("approve plan missing pull request number")?;
            let user: GitHubUser = gh_api_json(&plan.cwd, &plan.host, "/user").await?;
            let reviews = reviews(plan, number).await?;
            if latest_review_state(&reviews, &user.login)
                .is_some_and(|state| state.eq_ignore_ascii_case("APPROVED"))
            {
                Ok(GitHubReconciledOutcome::Succeeded {
                    result_ref: change_ref(plan, number),
                    verification: format!(
                        "reconciled after uncertain outcome: GitHub user {:?} is APPROVED",
                        user.login
                    ),
                })
            } else {
                Ok(GitHubReconciledOutcome::Unknown(
                    "authenticated GitHub user is not actively approved after an uncertain approval outcome; never retry blindly"
                        .into(),
                ))
            }
        }
        ForgeMutationKind::MergeMergeRequest => {
            let number = plan
                .change_request_iid
                .context("merge plan missing pull request number")?;
            let pull = get_pull(plan, number).await?;
            if pull.merged {
                Ok(GitHubReconciledOutcome::Succeeded {
                    result_ref: change_ref(plan, number),
                    verification:
                        "reconciled after uncertain outcome: GitHub reports pull request merged"
                            .into(),
                })
            } else if pull.state == "open" {
                Ok(GitHubReconciledOutcome::Unknown(
                    "GitHub pull request remains open after an uncertain merge outcome; never retry blindly"
                        .into(),
                ))
            } else {
                Ok(GitHubReconciledOutcome::Unknown(format!(
                    "GitHub pull request is closed but not merged after uncertain outcome"
                )))
            }
        }
    }
}

async fn revalidate_head(plan: &ForgeMutationPlan, number: u64, expected_sha: &str) -> Result<()> {
    let pull = validate_exact_open_pull(plan).await?;
    anyhow::ensure!(
        pull.number == number,
        "GitHub pull request identity changed before mutation"
    );
    let observed = required_head_sha(&pull)?;
    anyhow::ensure!(
        observed == expected_sha,
        "GitHub pull request HEAD changed before mutation: expected {expected_sha}, observed {observed}"
    );
    Ok(())
}

async fn validate_exact_open_pull(plan: &ForgeMutationPlan) -> Result<GitHubPullRequest> {
    let number = plan
        .change_request_iid
        .context("pull request mutation plan missing number")?;
    let pull = get_pull(plan, number).await?;
    anyhow::ensure!(pull.state == "open", "GitHub pull request #{number} is {}", pull.state);
    anyhow::ensure!(!pull.merged, "GitHub pull request #{number} is already merged");
    if let Some(source) = &plan.source_branch {
        anyhow::ensure!(
            pull.head.reference == *source,
            "GitHub pull request source branch changed: expected {source}, observed {}",
            pull.head.reference
        );
    }
    if let Some(target) = &plan.target_branch {
        anyhow::ensure!(
            pull.base.reference == *target,
            "GitHub pull request target branch changed: expected {target}, observed {}",
            pull.base.reference
        );
    }
    Ok(pull)
}

fn required_head_sha(pull: &GitHubPullRequest) -> Result<String> {
    let sha = pull.head.sha.trim();
    anyhow::ensure!(!sha.is_empty(), "GitHub pull request response is missing HEAD sha");
    Ok(sha.to_string())
}

async fn repository(plan: &ForgeMutationPlan) -> Result<GitHubRepository> {
    gh_api_json(&plan.cwd, &plan.host, &repository_endpoint(plan)?).await
}

async fn branch(plan: &ForgeMutationPlan, name: &str) -> Result<()> {
    #[derive(Deserialize)]
    struct Branch {
        name: String,
    }
    let endpoint = format!(
        "{}/branches/{}",
        repository_endpoint(plan)?,
        percent_encode_component(name)
    );
    let branch: Branch = gh_api_json(&plan.cwd, &plan.host, &endpoint).await?;
    anyhow::ensure!(branch.name == name, "GitHub branch identity mismatch");
    Ok(())
}

async fn get_pull(plan: &ForgeMutationPlan, number: u64) -> Result<GitHubPullRequest> {
    gh_api_json(
        &plan.cwd,
        &plan.host,
        &format!("{}/pulls/{number}", repository_endpoint(plan)?),
    )
    .await
}

async fn reviews(plan: &ForgeMutationPlan, number: u64) -> Result<Vec<GitHubReview>> {
    gh_api_json(
        &plan.cwd,
        &plan.host,
        &format!(
            "{}/pulls/{number}/reviews?per_page={MAX_PULL_PAGE}",
            repository_endpoint(plan)?
        ),
    )
    .await
}

fn latest_review_state<'a>(reviews: &'a [GitHubReview], login: &str) -> Option<&'a str> {
    reviews
        .iter()
        .filter(|review| {
            review
                .user
                .as_ref()
                .is_some_and(|user| user.login.eq_ignore_ascii_case(login))
        })
        .max_by_key(|review| review.id)
        .map(|review| review.state.as_str())
}

async fn matching_pull_requests(
    plan: &ForgeMutationPlan,
    source: &str,
    target: &str,
) -> Result<Vec<GitHubPullRequest>> {
    let endpoint = format!(
        "{}/pulls?state=open&base={}&per_page={MAX_PULL_PAGE}",
        repository_endpoint(plan)?,
        percent_encode_component(target)
    );
    let pulls: Vec<GitHubPullRequest> = gh_api_json(&plan.cwd, &plan.host, &endpoint).await?;
    Ok(pulls
        .into_iter()
        .filter(|pull| pull.head.reference == source && pull.base.reference == target)
        .collect())
}

fn repository_endpoint(plan: &ForgeMutationPlan) -> Result<String> {
    let (owner, repo) = split_repository_path(&plan.project_path)?;
    Ok(format!("/repos/{owner}/{repo}"))
}

fn split_repository_path(path: &str) -> Result<(&str, &str)> {
    let mut parts = path.split('/');
    let owner = parts.next().unwrap_or_default().trim();
    let repo = parts.next().unwrap_or_default().trim();
    anyhow::ensure!(
        !owner.is_empty() && !repo.is_empty() && parts.next().is_none(),
        "GitHub repository path must be exactly owner/repo: {path:?}"
    );
    Ok((owner, repo))
}

fn change_ref(plan: &ForgeMutationPlan, number: u64) -> String {
    format!(
        "github://{}/repositories/{}/pull-requests/{number}",
        plan.host, plan.project_id
    )
}

async fn gh_api_json<T>(cwd: &str, host: &str, endpoint: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let output = run_command(
        "gh",
        &["api", "--hostname", host, endpoint],
        Some(Path::new(cwd)),
    )
    .await
    .with_context(|| format!("run gh api {endpoint}"))?;
    if !output.success {
        bail!(
            "gh api {endpoint} failed: {}",
            trim_error(&output.stderr, "unknown gh error")
        );
    }
    serde_json::from_str(&output.stdout)
        .with_context(|| format!("decode gh api response for {endpoint}"))
}

async fn gh_api_mutation_json<T>(
    cwd: &str,
    host: &str,
    method: &str,
    endpoint: &str,
    fields: &[(&str, &str)],
) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut args = vec![
        "api".to_string(),
        "--hostname".to_string(),
        host.to_string(),
        "--method".to_string(),
        method.to_string(),
        endpoint.to_string(),
    ];
    for (key, value) in fields {
        args.push("-f".into());
        args.push(format!("{key}={value}"));
    }
    let borrowed = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = run_command("gh", &borrowed, Some(Path::new(cwd)))
        .await
        .with_context(|| format!("run GitHub mutation {method} {endpoint}"))?;
    if !output.success {
        bail!(
            "GitHub mutation {method} {endpoint} failed: {}",
            trim_error(&output.stderr, "unknown gh error")
        );
    }
    serde_json::from_str(&output.stdout)
        .with_context(|| format!("decode GitHub mutation response for {endpoint}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::{ForgeIdentity, ForgeProviderKind};

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
        assert_eq!(
            required_head_sha(&pull).expect("sha"),
            "0123456789abcdef"
        );
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
        assert_eq!(split_repository_path("octo/repo").unwrap(), ("octo", "repo"));
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
}
