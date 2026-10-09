use crate::domain::ThreadId;
use crate::forge::{
    CapabilityState, ChangeRequestSummary, ForgeCapability, ForgeFuture, ForgeIdentity,
    ForgeIssueSummary, ForgeObservation, ForgeProvider, ForgeProviderKind, ForgeReviewSummary,
    ForgeReviewTarget, PipelineSummary, RemoteIdentity, resolve_git_remote, run_command,
    trim_error,
};
use crate::operation::now_unix_ms;
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

const DEFAULT_PAGE_SIZE: usize = crate::forge_types::FORGE_OVERVIEW_PAGE_SIZE;
const REVIEW_THREAD_PAGE_SIZE: usize = 100;

#[derive(Clone, Copy, Debug, Default)]
pub struct GitHubProvider;

impl ForgeProvider for GitHubProvider {
    fn probe<'a>(&'a self, thread_id: ThreadId, cwd: String) -> ForgeFuture<'a, ForgeObservation> {
        Box::pin(async move {
            let remote = match resolve_git_remote(Path::new(&cwd)).await {
                Ok(remote) => remote,
                Err(error) => {
                    return ForgeObservation::unavailable(thread_id, cwd, error.to_string());
                }
            };
            match probe_github_with_remote(thread_id.clone(), cwd.clone(), remote).await {
                Ok(observation) => observation,
                Err(error) => ForgeObservation::unavailable(thread_id, cwd, error.to_string()),
            }
        })
    }

    fn probe_review<'a>(
        &'a self,
        target: ForgeReviewTarget,
    ) -> ForgeFuture<'a, ForgeReviewSummary> {
        Box::pin(async move {
            if target.provider != ForgeProviderKind::GitHub {
                return unavailable_review(
                    target.thread_id,
                    target.cwd,
                    target.change_request_iid,
                    "GitHub provider received non-GitHub review target".into(),
                );
            }
            probe_github_review(
                target.thread_id,
                target.cwd,
                target.host,
                target.project_path,
                target.change_request_iid,
            )
            .await
        })
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRepository {
    id: u64,
    full_name: String,
    html_url: String,
    default_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubIssue {
    number: u64,
    title: String,
    state: String,
    html_url: String,
    updated_at: Option<String>,
    pull_request: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct GitHubPullRequest {
    number: u64,
    title: String,
    state: String,
    html_url: String,
    updated_at: Option<String>,
    #[serde(default)]
    draft: bool,
    head: GitHubPullRef,
    base: GitHubPullRef,
}

#[derive(Debug, Deserialize)]
struct GitHubPullRef {
    #[serde(rename = "ref")]
    reference: String,
}

#[derive(Debug, Default, Deserialize)]
struct GitHubWorkflowRuns {
    #[serde(default)]
    workflow_runs: Vec<GitHubWorkflowRun>,
}

#[derive(Debug, Deserialize)]
struct GitHubWorkflowRun {
    id: u64,
    status: String,
    conclusion: Option<String>,
    head_branch: Option<String>,
    html_url: String,
    updated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubPullReview {
    id: u64,
    state: String,
    user: Option<GitHubUser>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubUser {
    id: u64,
}

#[derive(Debug, Deserialize)]
struct GitHubGraphQlEnvelope {
    data: GitHubGraphQlData,
}

#[derive(Debug, Deserialize)]
struct GitHubGraphQlData {
    repository: Option<GitHubGraphQlRepository>,
}

#[derive(Debug, Deserialize)]
struct GitHubGraphQlRepository {
    #[serde(rename = "pullRequest")]
    pull_request: Option<GitHubGraphQlPullRequest>,
}

#[derive(Debug, Deserialize)]
struct GitHubGraphQlPullRequest {
    #[serde(rename = "reviewThreads")]
    review_threads: GitHubReviewThreads,
}

#[derive(Debug, Deserialize)]
struct GitHubReviewThreads {
    #[serde(default)]
    nodes: Vec<GitHubReviewThread>,
    #[serde(rename = "pageInfo")]
    page_info: GitHubPageInfo,
}

#[derive(Debug, Deserialize)]
struct GitHubReviewThread {
    #[serde(rename = "isResolved")]
    is_resolved: bool,
}

#[derive(Debug, Deserialize)]
struct GitHubPageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
}

pub(crate) async fn probe_github_with_remote(
    thread_id: ThreadId,
    cwd: String,
    remote: RemoteIdentity,
) -> Result<ForgeObservation> {
    let (owner, repo) = split_repository_path(&remote.path_with_namespace)?;
    let repo_endpoint = format!("/repos/{owner}/{repo}");

    // Match the M6a bounded refresh shape: repository identity + issues + PRs + workflow runs.
    let repository: GitHubRepository = gh_api_json(&cwd, &remote.host, &repo_endpoint).await?;
    anyhow::ensure!(
        repository
            .full_name
            .eq_ignore_ascii_case(&remote.path_with_namespace),
        "GitHub repository identity mismatch: remote={} api={}",
        remote.path_with_namespace,
        repository.full_name
    );

    let issues_endpoint = format!(
        "{repo_endpoint}/issues?state=all&sort=updated&direction=desc&per_page={DEFAULT_PAGE_SIZE}"
    );
    let pulls_endpoint = format!("{repo_endpoint}/pulls?state=open&per_page={DEFAULT_PAGE_SIZE}");
    let runs_endpoint = format!("{repo_endpoint}/actions/runs?per_page={DEFAULT_PAGE_SIZE}");

    let (issues_result, pulls_result, runs_result) = tokio::join!(
        gh_api_json::<Vec<GitHubIssue>>(&cwd, &remote.host, &issues_endpoint),
        gh_api_json::<Vec<GitHubPullRequest>>(&cwd, &remote.host, &pulls_endpoint),
        gh_api_json::<GitHubWorkflowRuns>(&cwd, &remote.host, &runs_endpoint),
    );

    let (mut capabilities, freshness, error) = crate::forge::core_observation_status(
        ForgeProviderKind::GitHub,
        issues_result.is_ok(),
        pulls_result.is_ok(),
        runs_result.is_ok(),
    );
    capabilities.insert(ForgeCapability::IssueBoards, CapabilityState::Unavailable);
    capabilities.insert(ForgeCapability::WorkItems, CapabilityState::Unavailable);

    // GitHub's Issues endpoint also returns PRs. Filtering them must not
    // erase the evidence that the *raw* first page was already full.
    let overview_source_page_saturated = issues_result
        .as_ref()
        .is_ok_and(|rows| rows.len() >= DEFAULT_PAGE_SIZE)
        || pulls_result
            .as_ref()
            .is_ok_and(|rows| rows.len() >= DEFAULT_PAGE_SIZE)
        || runs_result
            .as_ref()
            .is_ok_and(|page| page.workflow_runs.len() >= DEFAULT_PAGE_SIZE);
    let issues = issues_result.unwrap_or_default();
    let pulls = pulls_result.unwrap_or_default();
    let runs = runs_result.unwrap_or_default();

    Ok(ForgeObservation {
        thread_id,
        cwd,
        remote_name: Some(remote.remote_name),
        remote_url: Some(remote.remote_url),
        identity: Some(ForgeIdentity {
            provider: ForgeProviderKind::GitHub,
            host: remote.host,
            project_id: repository.id.to_string(),
            path_with_namespace: repository.full_name,
            web_url: repository.html_url,
            default_branch: repository.default_branch,
        }),
        capabilities,
        issues: issues
            .into_iter()
            .filter(|issue| issue.pull_request.is_none())
            .map(|issue| ForgeIssueSummary {
                iid: issue.number,
                title: issue.title,
                state: issue.state,
                web_url: issue.html_url,
                updated_at: issue.updated_at,
            })
            .collect(),
        change_requests: pulls
            .into_iter()
            .map(|pull| ChangeRequestSummary {
                iid: pull.number,
                title: pull.title,
                state: pull.state,
                source_branch: pull.head.reference,
                target_branch: pull.base.reference,
                web_url: pull.html_url,
                updated_at: pull.updated_at,
                draft: pull.draft,
                detailed_merge_status: None,
                blocking_discussions_resolved: None,
            })
            .collect(),
        pipelines: runs
            .workflow_runs
            .into_iter()
            .filter_map(|run| {
                let reference = run.head_branch?;
                Some(PipelineSummary {
                    id: run.id,
                    status: normalize_run_status(&run.status, run.conclusion.as_deref()),
                    reference,
                    web_url: run.html_url,
                    updated_at: run.updated_at,
                })
            })
            .collect(),
        overview_source_page_saturated,
        review: None,
        observed_at_unix_ms: now_unix_ms(),
        freshness,
        error,
    })
}

async fn probe_github_review(
    thread_id: ThreadId,
    cwd: String,
    host: String,
    project_path: String,
    change_request_iid: u64,
) -> ForgeReviewSummary {
    let (owner, repo) = match split_repository_path(&project_path) {
        Ok(parts) => parts,
        Err(error) => {
            return unavailable_review(thread_id, cwd, change_request_iid, error.to_string());
        }
    };
    let reviews_endpoint = format!("/repos/{owner}/{repo}/pulls/{change_request_iid}/reviews");

    let reviews = crate::forge::bounded_review_pages(|page| {
        let cwd = cwd.clone();
        let host = host.clone();
        let endpoint = format!("{reviews_endpoint}?per_page=100&page={page}");
        async move { gh_api_json::<Vec<GitHubPullReview>>(&cwd, &host, &endpoint).await }
    });
    let threads = github_review_threads(&cwd, &host, owner, repo, change_request_iid);
    let (reviews_result, threads_result) = tokio::join!(reviews, threads);

    let approvals_available = reviews_result.is_ok();
    let (approved_by_count, changes_requested_by_count) = reviews_result
        .as_ref()
        .map(|reviews| latest_review_counts(reviews))
        .unwrap_or((0, 0));

    let (discussions_total, unresolved_discussions, discussions_available) =
        match threads_result.as_ref() {
            Ok(threads) if !threads.page_info.has_next_page => {
                let unresolved = threads
                    .nodes
                    .iter()
                    .filter(|thread| !thread.is_resolved)
                    .count();
                (threads.nodes.len(), unresolved, true)
            }
            _ => (0, 0, false),
        };

    let mut errors = Vec::new();
    if let Err(error) = reviews_result {
        errors.push(format!("reviews: {error}"));
    }
    match threads_result {
        Err(error) => errors.push(format!("review threads: {error}")),
        Ok(threads) if threads.page_info.has_next_page => errors.push(format!(
            "review threads: more than {REVIEW_THREAD_PAGE_SIZE} threads; bounded projection unavailable"
        )),
        Ok(_) => {}
    }

    ForgeReviewSummary {
        thread_id,
        cwd,
        change_request_iid,
        approvals_required: None,
        approvals_left: None,
        approved_by_count,
        changes_requested_by_count,
        discussions_total,
        unresolved_discussions,
        approvals_available,
        discussions_available,
        observed_at_unix_ms: now_unix_ms(),
        error: (!errors.is_empty()).then(|| errors.join("; ")),
    }
}

fn latest_review_counts(reviews: &[GitHubPullReview]) -> (usize, usize) {
    let mut latest = BTreeMap::<u64, &GitHubPullReview>::new();
    for review in reviews {
        let Some(user) = &review.user else {
            continue;
        };
        match latest.get(&user.id) {
            Some(current) if current.id >= review.id => {}
            _ => {
                latest.insert(user.id, review);
            }
        }
    }

    let approved = latest
        .values()
        .filter(|review| review.state.eq_ignore_ascii_case("APPROVED"))
        .count();
    let changes_requested = latest
        .values()
        .filter(|review| review.state.eq_ignore_ascii_case("CHANGES_REQUESTED"))
        .count();
    (approved, changes_requested)
}

async fn github_review_threads(
    cwd: &str,
    host: &str,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<GitHubReviewThreads> {
    const QUERY: &str = "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){pullRequest(number:$number){reviewThreads(first:100){nodes{isResolved}pageInfo{hasNextPage}}}}}";
    let number = number.to_string();
    let response: GitHubGraphQlEnvelope = gh_graphql_json(
        cwd,
        host,
        QUERY,
        &[
            ("owner", owner, false),
            ("name", repo, false),
            ("number", &number, true),
        ],
    )
    .await?;
    response
        .data
        .repository
        .and_then(|repository| repository.pull_request)
        .map(|pull| pull.review_threads)
        .ok_or_else(|| anyhow!("GitHub GraphQL pull request reviewThreads are unavailable"))
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

async fn gh_graphql_json<T>(
    cwd: &str,
    host: &str,
    query: &str,
    fields: &[(&str, &str, bool)],
) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut args = vec![
        "api".to_string(),
        "--hostname".to_string(),
        host.to_string(),
        "graphql".to_string(),
        "-f".to_string(),
        format!("query={query}"),
    ];
    for (key, value, typed) in fields {
        args.push(if *typed { "-F" } else { "-f" }.to_string());
        args.push(format!("{key}={value}"));
    }
    let borrowed = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = run_command("gh", &borrowed, Some(Path::new(cwd)))
        .await
        .context("run gh api graphql")?;
    if !output.success {
        bail!(
            "gh api graphql failed: {}",
            trim_error(&output.stderr, "unknown gh GraphQL error")
        );
    }
    serde_json::from_str(&output.stdout).context("decode gh GraphQL response")
}

fn split_repository_path(path: &str) -> Result<(&str, &str)> {
    let mut components = path.split('/');
    let owner = components.next().unwrap_or_default().trim();
    let repo = components.next().unwrap_or_default().trim();
    anyhow::ensure!(
        !owner.is_empty() && !repo.is_empty() && components.next().is_none(),
        "GitHub repository path must be exactly owner/repo: {path:?}"
    );
    Ok((owner, repo))
}

fn normalize_run_status(status: &str, conclusion: Option<&str>) -> String {
    match conclusion.map(str::to_ascii_lowercase).as_deref() {
        Some("failure") => "failed".into(),
        Some("cancelled") | Some("canceled") => "canceled".into(),
        Some(value) => value.to_string(),
        None => status.to_ascii_lowercase(),
    }
}

fn unavailable_review(
    thread_id: ThreadId,
    cwd: String,
    change_request_iid: u64,
    error: String,
) -> ForgeReviewSummary {
    ForgeReviewSummary {
        thread_id,
        cwd,
        change_request_iid,
        approvals_required: None,
        approvals_left: None,
        approved_by_count: 0,
        changes_requested_by_count: 0,
        discussions_total: 0,
        unresolved_discussions: 0,
        approvals_available: false,
        discussions_available: false,
        observed_at_unix_ms: now_unix_ms(),
        error: Some(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
