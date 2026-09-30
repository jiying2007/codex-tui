use crate::domain::ThreadId;
use crate::forge_github::{GitHubProvider, probe_github_with_remote};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_STDOUT_BYTES: usize = 1024 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;
const DEFAULT_PAGE_SIZE: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ForgeProviderKind {
    GitLab,
    GitHub,
}

impl ForgeProviderKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::GitLab => "gitlab",
            Self::GitHub => "github",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeIdentity {
    pub provider: ForgeProviderKind,
    pub host: String,
    pub project_id: String,
    pub path_with_namespace: String,
    pub web_url: String,
    pub default_branch: Option<String>,
}

impl ForgeIdentity {
    pub fn issue_source_ref(&self, iid: u64) -> String {
        match self.provider {
            ForgeProviderKind::GitLab => format!(
                "gitlab://{}/projects/{}/issues/{iid}",
                self.host, self.project_id
            ),
            ForgeProviderKind::GitHub => format!(
                "github://{}/repositories/{}/issues/{iid}",
                self.host, self.project_id
            ),
        }
    }

    pub fn change_request_source_ref(&self, iid: u64) -> String {
        match self.provider {
            ForgeProviderKind::GitLab => format!(
                "gitlab://{}/projects/{}/merge-requests/{iid}",
                self.host, self.project_id
            ),
            ForgeProviderKind::GitHub => format!(
                "github://{}/repositories/{}/pull-requests/{iid}",
                self.host, self.project_id
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ForgeCapability {
    Issues,
    IssueBoards,
    MergeRequests,
    Pipelines,
    ApprovalSummary,
    Discussions,
    WorkItems,
}

impl ForgeCapability {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Issues => "issues",
            Self::IssueBoards => "issue-boards",
            Self::MergeRequests => "merge-requests",
            Self::Pipelines => "pipelines",
            Self::ApprovalSummary => "approval-summary",
            Self::Discussions => "discussions",
            Self::WorkItems => "work-items",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityState {
    Available,
    Unavailable,
    Unknown,
}

impl CapabilityState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForgeFreshness {
    Fresh,
    Aging,
    Stale,
    Unavailable,
}

impl ForgeFreshness {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Aging => "aging",
            Self::Stale => "stale",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeIssueSummary {
    pub iid: u64,
    pub title: String,
    pub state: String,
    pub web_url: String,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeRequestSummary {
    pub iid: u64,
    pub title: String,
    pub state: String,
    pub source_branch: String,
    pub target_branch: String,
    pub web_url: String,
    pub updated_at: Option<String>,
    pub draft: bool,
    pub detailed_merge_status: Option<String>,
    pub blocking_discussions_resolved: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineSummary {
    pub id: u64,
    pub status: String,
    pub reference: String,
    pub web_url: String,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueBoardSummary {
    pub id: u64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeReviewSummary {
    pub thread_id: ThreadId,
    pub cwd: String,
    pub change_request_iid: u64,
    pub approvals_required: Option<u64>,
    pub approvals_left: Option<u64>,
    pub approved_by_count: usize,
    pub changes_requested_by_count: usize,
    pub discussions_total: usize,
    pub unresolved_discussions: usize,
    pub approvals_available: bool,
    pub discussions_available: bool,
    pub observed_at_unix_ms: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeObservation {
    pub thread_id: ThreadId,
    pub cwd: String,
    pub remote_name: Option<String>,
    pub remote_url: Option<String>,
    pub identity: Option<ForgeIdentity>,
    pub capabilities: BTreeMap<ForgeCapability, CapabilityState>,
    pub issues: Vec<ForgeIssueSummary>,
    pub change_requests: Vec<ChangeRequestSummary>,
    pub pipelines: Vec<PipelineSummary>,
    pub review: Option<ForgeReviewSummary>,
    pub observed_at_unix_ms: u64,
    pub freshness: ForgeFreshness,
    pub error: Option<String>,
}

impl ForgeObservation {
    pub fn pending(thread_id: ThreadId, cwd: String) -> Self {
        Self {
            thread_id,
            cwd,
            remote_name: None,
            remote_url: None,
            identity: None,
            capabilities: default_capabilities(),
            issues: vec![],
            change_requests: vec![],
            pipelines: vec![],
            review: None,
            observed_at_unix_ms: 0,
            freshness: ForgeFreshness::Unavailable,
            error: None,
        }
    }

    pub fn unavailable(thread_id: ThreadId, cwd: String, error: impl Into<String>) -> Self {
        Self {
            thread_id,
            cwd,
            remote_name: None,
            remote_url: None,
            identity: None,
            capabilities: default_capabilities(),
            issues: vec![],
            change_requests: vec![],
            pipelines: vec![],
            review: None,
            observed_at_unix_ms: now_unix_ms(),
            freshness: ForgeFreshness::Unavailable,
            error: Some(error.into()),
        }
    }

    pub fn change_request_for_branch(&self, branch: &str) -> Option<&ChangeRequestSummary> {
        self.change_requests
            .iter()
            .find(|change| change.source_branch == branch)
    }

    pub fn pipeline_for_branch(&self, branch: &str) -> Option<&PipelineSummary> {
        self.pipelines
            .iter()
            .find(|pipeline| pipeline.reference == branch)
    }

    pub fn freshness_at(&self, now_unix_ms: u64) -> ForgeFreshness {
        if self.observed_at_unix_ms == 0 || self.error.is_some() || self.identity.is_none() {
            return ForgeFreshness::Unavailable;
        }
        let age = now_unix_ms.saturating_sub(self.observed_at_unix_ms);
        if age <= 10_000 {
            ForgeFreshness::Fresh
        } else if age <= 60_000 {
            ForgeFreshness::Aging
        } else {
            ForgeFreshness::Stale
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteIdentity {
    pub remote_name: String,
    pub remote_url: String,
    pub host: String,
    pub path_with_namespace: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeDoctorSnapshot {
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    pub authenticated: Option<bool>,
    pub server_version: Option<String>,
    pub server_edition: Option<String>,
    pub server_tier: Option<String>,
    pub remote: Option<RemoteIdentity>,
    pub observation: ForgeObservation,
    pub boards: Vec<IssueBoardSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeReviewTarget {
    pub thread_id: ThreadId,
    pub cwd: String,
    pub provider: ForgeProviderKind,
    pub host: String,
    pub project_id: String,
    pub project_path: String,
    pub change_request_iid: u64,
}

pub type ForgeFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait ForgeProvider: Send + Sync {
    fn probe<'a>(&'a self, thread_id: ThreadId, cwd: String) -> ForgeFuture<'a, ForgeObservation>;

    fn probe_review<'a>(&'a self, target: ForgeReviewTarget)
    -> ForgeFuture<'a, ForgeReviewSummary>;
}

pub fn canonical_provider_for_host(host: &str) -> Option<ForgeProviderKind> {
    if host.eq_ignore_ascii_case("github.com") {
        Some(ForgeProviderKind::GitHub)
    } else if host.eq_ignore_ascii_case("gitlab.com") {
        Some(ForgeProviderKind::GitLab)
    } else {
        None
    }
}

fn provider_from_auth_state(
    host: &str,
    github_authenticated: bool,
    gitlab_authenticated: bool,
) -> Result<ForgeProviderKind> {
    if let Some(provider) = canonical_provider_for_host(host) {
        return Ok(provider);
    }
    match (github_authenticated, gitlab_authenticated) {
        (true, false) => Ok(ForgeProviderKind::GitHub),
        (false, true) => Ok(ForgeProviderKind::GitLab),
        (true, true) => bail!(
            "forge provider is ambiguous for host {host}: both gh and glab are authenticated"
        ),
        (false, false) => bail!(
            "forge provider not configured for host {host}; authenticate with gh or glab"
        ),
    }
}

async fn detect_provider(cwd: &str, host: &str) -> Result<ForgeProviderKind> {
    if let Some(provider) = canonical_provider_for_host(host) {
        return Ok(provider);
    }

    let gh_args = ["auth", "status", "--hostname", host];
    let glab_args = ["auth", "status", "--hostname", host];
    let (gh, glab) = tokio::join!(
        run_command("gh", &gh_args, Some(Path::new(cwd))),
        run_command("glab", &glab_args, Some(Path::new(cwd))),
    );
    provider_from_auth_state(
        host,
        gh.is_ok_and(|output| output.success),
        glab.is_ok_and(|output| output.success),
    )
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RoutingForgeProvider;

impl ForgeProvider for RoutingForgeProvider {
    fn probe<'a>(&'a self, thread_id: ThreadId, cwd: String) -> ForgeFuture<'a, ForgeObservation> {
        Box::pin(async move {
            let remote = match resolve_git_remote(Path::new(&cwd)).await {
                Ok(remote) => remote,
                Err(error) => {
                    return ForgeObservation::unavailable(thread_id, cwd, error.to_string());
                }
            };
            let provider = match detect_provider(&cwd, &remote.host).await {
                Ok(provider) => provider,
                Err(error) => {
                    return ForgeObservation::unavailable(thread_id, cwd, error.to_string());
                }
            };
            let result = match provider {
                ForgeProviderKind::GitHub => {
                    probe_github_with_remote(thread_id.clone(), cwd.clone(), remote).await
                }
                ForgeProviderKind::GitLab => {
                    probe_gitlab_with_remote(thread_id.clone(), cwd.clone(), remote).await
                }
            };
            result.unwrap_or_else(|error| {
                ForgeObservation::unavailable(thread_id, cwd, error.to_string())
            })
        })
    }

    fn probe_review<'a>(
        &'a self,
        target: ForgeReviewTarget,
    ) -> ForgeFuture<'a, ForgeReviewSummary> {
        match target.provider {
            ForgeProviderKind::GitHub => GitHubProvider.probe_review(target),
            ForgeProviderKind::GitLab => GitLabProvider.probe_review(target),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GitLabProvider;

impl ForgeProvider for GitLabProvider {
    fn probe<'a>(&'a self, thread_id: ThreadId, cwd: String) -> ForgeFuture<'a, ForgeObservation> {
        Box::pin(async move {
            match probe_gitlab(thread_id.clone(), cwd.clone()).await {
                Ok(observation) => observation,
                Err(error) => ForgeObservation::unavailable(thread_id, cwd, error.to_string()),
            }
        })
    }

    fn probe_review<'a>(
        &'a self,
        target: ForgeReviewTarget,
    ) -> ForgeFuture<'a, ForgeReviewSummary> {
        Box::pin(probe_change_request_review(
            target.thread_id,
            target.cwd,
            target.host,
            target.project_id,
            target.change_request_iid,
        ))
    }
}

#[derive(Clone, Debug)]
pub enum ForgeCommand {
    Probe { thread_id: ThreadId, cwd: String },
    ProbeReview(ForgeReviewTarget),
}

#[derive(Clone, Debug)]
pub enum ForgeEvent {
    Observation(Box<ForgeObservation>),
    Review(ForgeReviewSummary),
}

pub struct ForgeHandle {
    command_tx: mpsc::UnboundedSender<ForgeCommand>,
    event_rx: mpsc::UnboundedReceiver<ForgeEvent>,
    task: JoinHandle<()>,
}

impl ForgeHandle {
    pub fn start() -> Self {
        Self::start_with_provider(Arc::new(RoutingForgeProvider))
    }

    pub fn start_with_provider(provider: Arc<dyn ForgeProvider>) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(run_actor(provider, command_rx, event_tx));
        Self {
            command_tx,
            event_rx,
            task,
        }
    }

    pub fn probe(&self, thread_id: ThreadId, cwd: String) -> Result<()> {
        self.command_tx
            .send(ForgeCommand::Probe { thread_id, cwd })
            .map_err(|_| anyhow!("Forge actor is not available"))
    }

    pub fn probe_review(&self, target: ForgeReviewTarget) -> Result<()> {
        self.command_tx
            .send(ForgeCommand::ProbeReview(target))
            .map_err(|_| anyhow!("Forge actor is not available"))
    }

    pub fn try_recv(&mut self) -> Option<ForgeEvent> {
        self.event_rx.try_recv().ok()
    }
}

impl Drop for ForgeHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run_actor(
    provider: Arc<dyn ForgeProvider>,
    mut command_rx: mpsc::UnboundedReceiver<ForgeCommand>,
    event_tx: mpsc::UnboundedSender<ForgeEvent>,
) {
    while let Some(command) = command_rx.recv().await {
        match command {
            ForgeCommand::Probe { thread_id, cwd } => {
                let observation = provider.probe(thread_id, cwd).await;
                let _ = event_tx.send(ForgeEvent::Observation(Box::new(observation)));
            }
            ForgeCommand::ProbeReview(target) => {
                let review = provider.probe_review(target).await;
                let _ = event_tx.send(ForgeEvent::Review(review));
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct GitLabProject {
    id: serde_json::Value,
    path_with_namespace: String,
    web_url: String,
    default_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitLabIssue {
    iid: u64,
    title: String,
    state: String,
    web_url: String,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitLabMergeRequest {
    iid: u64,
    title: String,
    state: String,
    source_branch: String,
    target_branch: String,
    web_url: String,
    updated_at: Option<String>,
    #[serde(default)]
    draft: bool,
    detailed_merge_status: Option<String>,
    blocking_discussions_resolved: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct GitLabPipeline {
    id: u64,
    status: String,
    #[serde(rename = "ref")]
    reference: String,
    web_url: String,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitLabBoard {
    id: u64,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitLabVersion {
    version: String,
    #[serde(default)]
    enterprise: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct GitLabApprovals {
    approvals_required: Option<u64>,
    approvals_left: Option<u64>,
    #[serde(default)]
    approved_by: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct GitLabDiscussion {
    #[serde(default)]
    notes: Vec<GitLabDiscussionNote>,
}

#[derive(Debug, Deserialize)]
struct GitLabDiscussionNote {
    #[serde(default)]
    resolvable: bool,
    resolved: Option<bool>,
}

pub async fn probe_thread(thread_id: ThreadId, cwd: String) -> ForgeObservation {
    RoutingForgeProvider.probe(thread_id, cwd).await
}

pub async fn probe_gitlab(thread_id: ThreadId, cwd: String) -> Result<ForgeObservation> {
    let remote = resolve_git_remote(Path::new(&cwd)).await?;
    probe_gitlab_with_remote(thread_id, cwd, remote).await
}

pub(crate) async fn probe_gitlab_with_remote(
    thread_id: ThreadId,
    cwd: String,
    remote: RemoteIdentity,
) -> Result<ForgeObservation> {
    let project_path = percent_encode_project_path(&remote.path_with_namespace);

    let project: GitLabProject =
        glab_api_json(&cwd, &remote.host, &format!("/projects/{project_path}"))
            .await
            .context("resolve GitLab project")?;

    let project_id = json_id_to_string(&project.id)?;
    let encoded_id = percent_encode_component(&project_id);

    // Keep the normal user-visible refresh at four glab API subprocesses:
    // project identity + issues + merge requests + pipelines.
    let issues: Vec<GitLabIssue> = glab_api_json(
        &cwd,
        &remote.host,
        &format!(
            "/projects/{encoded_id}/issues?scope=all&state=all&order_by=updated_at&sort=desc&per_page={DEFAULT_PAGE_SIZE}"
        ),
    )
    .await
    .context("load GitLab issues")?;

    let merge_requests: Vec<GitLabMergeRequest> = glab_api_json(
        &cwd,
        &remote.host,
        &format!("/projects/{encoded_id}/merge_requests?scope=all&state=opened&per_page={DEFAULT_PAGE_SIZE}"),
    )
    .await
    .context("load GitLab merge requests")?;

    let pipelines: Vec<GitLabPipeline> = glab_api_json(
        &cwd,
        &remote.host,
        &format!("/projects/{encoded_id}/pipelines?per_page={DEFAULT_PAGE_SIZE}"),
    )
    .await
    .context("load GitLab pipelines")?;

    let mut capabilities = default_capabilities();
    capabilities.insert(ForgeCapability::Issues, CapabilityState::Available);
    capabilities.insert(ForgeCapability::MergeRequests, CapabilityState::Available);
    capabilities.insert(ForgeCapability::Pipelines, CapabilityState::Available);

    Ok(ForgeObservation {
        thread_id,
        cwd,
        remote_name: Some(remote.remote_name),
        remote_url: Some(remote.remote_url),
        identity: Some(ForgeIdentity {
            provider: ForgeProviderKind::GitLab,
            host: remote.host,
            project_id,
            path_with_namespace: project.path_with_namespace,
            web_url: project.web_url,
            default_branch: project.default_branch,
        }),
        capabilities,
        issues: issues
            .into_iter()
            .map(|issue| ForgeIssueSummary {
                iid: issue.iid,
                title: issue.title,
                state: issue.state,
                web_url: issue.web_url,
                updated_at: issue.updated_at,
            })
            .collect(),
        change_requests: merge_requests
            .into_iter()
            .map(|change| ChangeRequestSummary {
                iid: change.iid,
                title: change.title,
                state: change.state,
                source_branch: change.source_branch,
                target_branch: change.target_branch,
                web_url: change.web_url,
                updated_at: change.updated_at,
                draft: change.draft,
                detailed_merge_status: change.detailed_merge_status,
                blocking_discussions_resolved: change.blocking_discussions_resolved,
            })
            .collect(),
        pipelines: pipelines
            .into_iter()
            .map(|pipeline| PipelineSummary {
                id: pipeline.id,
                status: pipeline.status,
                reference: pipeline.reference,
                web_url: pipeline.web_url,
                updated_at: pipeline.updated_at,
            })
            .collect(),
        review: None,
        observed_at_unix_ms: now_unix_ms(),
        freshness: ForgeFreshness::Fresh,
        error: None,
    })
}

pub async fn probe_issue_boards(cwd: &str) -> Result<Vec<IssueBoardSummary>> {
    let remote = resolve_git_remote(Path::new(cwd)).await?;
    let project_path = percent_encode_project_path(&remote.path_with_namespace);
    let project: GitLabProject =
        glab_api_json(cwd, &remote.host, &format!("/projects/{project_path}")).await?;
    let project_id = percent_encode_component(&json_id_to_string(&project.id)?);
    let boards: Vec<GitLabBoard> = glab_api_json(
        cwd,
        &remote.host,
        &format!("/projects/{project_id}/boards?per_page={DEFAULT_PAGE_SIZE}"),
    )
    .await?;
    Ok(boards
        .into_iter()
        .map(|board| IssueBoardSummary {
            id: board.id,
            name: board.name.unwrap_or_else(|| format!("Board {}", board.id)),
        })
        .collect())
}

pub async fn probe_change_request_review(
    thread_id: ThreadId,
    cwd: String,
    host: String,
    project_id: String,
    change_request_iid: u64,
) -> ForgeReviewSummary {
    let encoded_project_id = percent_encode_component(&project_id);
    let approvals_endpoint =
        format!("/projects/{encoded_project_id}/merge_requests/{change_request_iid}/approvals");
    let discussions_endpoint = format!(
        "/projects/{encoded_project_id}/merge_requests/{change_request_iid}/discussions?per_page=100"
    );

    let (approvals_result, discussions_result) = tokio::join!(
        glab_api_json::<GitLabApprovals>(&cwd, &host, &approvals_endpoint),
        glab_api_json::<Vec<GitLabDiscussion>>(&cwd, &host, &discussions_endpoint),
    );

    let approvals_available = approvals_result.is_ok();
    let discussions_available = discussions_result.is_ok();

    let (approvals_required, approvals_left, approved_by_count) = approvals_result
        .as_ref()
        .map(|approvals| {
            (
                approvals.approvals_required,
                approvals.approvals_left,
                approvals.approved_by.len(),
            )
        })
        .unwrap_or((None, None, 0));

    let (discussions_total, unresolved_discussions) = discussions_result
        .as_ref()
        .map(|discussions| {
            let unresolved = discussions
                .iter()
                .flat_map(|discussion| &discussion.notes)
                .filter(|note| note.resolvable && note.resolved != Some(true))
                .count();
            (discussions.len(), unresolved)
        })
        .unwrap_or((0, 0));

    let mut errors = Vec::new();
    if let Err(error) = approvals_result {
        errors.push(format!("approvals: {error}"));
    }
    if let Err(error) = discussions_result {
        errors.push(format!("discussions: {error}"));
    }

    ForgeReviewSummary {
        thread_id,
        cwd,
        change_request_iid,
        approvals_required,
        approvals_left,
        approved_by_count,
        changes_requested_by_count: 0,
        discussions_total,
        unresolved_discussions,
        approvals_available,
        discussions_available,
        observed_at_unix_ms: now_unix_ms(),
        error: (!errors.is_empty()).then(|| errors.join("; ")),
    }
}

pub async fn doctor(cwd: String) -> ForgeDoctorSnapshot {
    let remote = resolve_git_remote(Path::new(&cwd)).await.ok();
    let provider = match remote.as_ref() {
        Some(remote) => detect_provider(&cwd, &remote.host).await.ok(),
        None => None,
    };

    let client_name = provider.map(|provider| match provider {
        ForgeProviderKind::GitLab => "glab".to_string(),
        ForgeProviderKind::GitHub => "gh".to_string(),
    });
    let client_version = match provider {
        Some(ForgeProviderKind::GitLab) => run_command("glab", &["version"], Some(Path::new(&cwd)))
            .await
            .ok()
            .and_then(|output| {
                output
                    .success
                    .then(|| first_nonempty_line(&output.stdout).map(ToOwned::to_owned))
                    .flatten()
            }),
        Some(ForgeProviderKind::GitHub) => run_command("gh", &["--version"], Some(Path::new(&cwd)))
            .await
            .ok()
            .and_then(|output| {
                output
                    .success
                    .then(|| first_nonempty_line(&output.stdout).map(ToOwned::to_owned))
                    .flatten()
            }),
        None => None,
    };

    let (authenticated, server_version, server_edition, server_tier) =
        if let (Some(remote), Some(provider)) = (&remote, provider) {
            match provider {
                ForgeProviderKind::GitLab => {
                    let auth_args = ["auth", "status", "--hostname", remote.host.as_str()];
                    let auth = run_command("glab", &auth_args, Some(Path::new(&cwd)));
                    let version = glab_api_json::<GitLabVersion>(&cwd, &remote.host, "/version");
                    let (auth, version) = tokio::join!(auth, version);
                    let version = version.ok();
                    (
                        auth.ok().map(|output| output.success),
                        version.as_ref().map(|version| version.version.clone()),
                        version.and_then(|version| {
                            version.enterprise.map(|enterprise| {
                                if enterprise {
                                    "enterprise".to_string()
                                } else {
                                    "community".to_string()
                                }
                            })
                        }),
                        None,
                    )
                }
                ForgeProviderKind::GitHub => {
                    let auth_args = ["auth", "status", "--hostname", remote.host.as_str()];
                    let auth = run_command("gh", &auth_args, Some(Path::new(&cwd))).await;
                    (auth.ok().map(|output| output.success), None, None, None)
                }
            }
        } else {
            (None, None, None, None)
        };

    let mut observation = probe_thread(ThreadId::new("doctor-forge"), cwd.clone()).await;
    let boards_result = if observation
        .identity
        .as_ref()
        .is_some_and(|identity| identity.provider == ForgeProviderKind::GitLab)
    {
        Some(probe_issue_boards(&cwd).await)
    } else {
        None
    };
    let boards = match boards_result {
        Some(Ok(boards)) => {
            observation
                .capabilities
                .insert(ForgeCapability::IssueBoards, CapabilityState::Available);
            boards
        }
        Some(Err(_)) => {
            observation
                .capabilities
                .insert(ForgeCapability::IssueBoards, CapabilityState::Unavailable);
            vec![]
        }
        None => vec![],
    };

    ForgeDoctorSnapshot {
        client_name,
        client_version,
        authenticated,
        server_version,
        server_edition,
        server_tier,
        remote,
        observation,
        boards,
    }
}

pub(crate) async fn resolve_git_remote(cwd: &Path) -> Result<RemoteIdentity> {
    let remotes_output = run_command(
        "git",
        &["-C", cwd.to_string_lossy().as_ref(), "remote", "-v"],
        Some(cwd),
    )
    .await
    .context("list Git remotes")?;
    if !remotes_output.success {
        bail!(
            "git remote failed: {}",
            trim_error(&remotes_output.stderr, "unknown git error")
        );
    }

    let remotes = parse_fetch_remotes(&remotes_output.stdout);
    if remotes.is_empty() {
        bail!("repository has no Git fetch remotes");
    }

    let branch_output = run_command(
        "git",
        &[
            "-C",
            cwd.to_string_lossy().as_ref(),
            "symbolic-ref",
            "--quiet",
            "--short",
            "HEAD",
        ],
        Some(cwd),
    )
    .await
    .context("resolve current Git branch")?;

    let branch_remote = if branch_output.success {
        let branch = branch_output.stdout.trim();
        if branch.is_empty() {
            None
        } else {
            let key = format!("branch.{branch}.remote");
            let configured = run_command(
                "git",
                &[
                    "-C",
                    cwd.to_string_lossy().as_ref(),
                    "config",
                    "--get",
                    &key,
                ],
                Some(cwd),
            )
            .await
            .context("resolve branch upstream remote")?;
            configured
                .success
                .then(|| configured.stdout.trim().to_string())
                .filter(|name| !name.is_empty() && name != ".")
        }
    } else {
        None
    };

    let selected = select_remote(&remotes, branch_remote.as_deref())?;
    let (host, path_with_namespace) = parse_git_remote_url(&selected.1)
        .ok_or_else(|| anyhow!("unsupported Git remote URL: {}", selected.1))?;

    Ok(RemoteIdentity {
        remote_name: selected.0.clone(),
        remote_url: selected.1.clone(),
        host,
        path_with_namespace,
    })
}

fn parse_fetch_remotes(stdout: &str) -> Vec<(String, String)> {
    let mut remotes = Vec::new();
    for line in stdout.lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else {
            continue;
        };
        let Some(url) = fields.next() else {
            continue;
        };
        let Some(kind) = fields.next() else {
            continue;
        };
        if kind != "(fetch)" {
            continue;
        }
        if remotes.iter().any(|(known, _)| known == name) {
            continue;
        }
        remotes.push((name.to_string(), url.to_string()));
    }
    remotes
}

fn select_remote<'a>(
    remotes: &'a [(String, String)],
    branch_remote: Option<&str>,
) -> Result<&'a (String, String)> {
    if let Some(branch_remote) = branch_remote
        && let Some(remote) = remotes.iter().find(|(name, _)| name == branch_remote)
    {
        return Ok(remote);
    }
    if let Some(origin) = remotes.iter().find(|(name, _)| name == "origin") {
        return Ok(origin);
    }
    if remotes.len() == 1 {
        return Ok(&remotes[0]);
    }
    bail!(
        "multiple eligible Git remotes and no branch/origin authority: {}",
        remotes
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub fn parse_git_remote_url(url: &str) -> Option<(String, String)> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix("ssh://") {
        let without_user = rest.split_once('@').map_or(rest, |(_, tail)| tail);
        let (host_port, path) = without_user.split_once('/')?;
        let host = host_port.split(':').next()?.trim();
        return normalize_remote_parts(host, path);
    }

    if let Some((scheme, rest)) = trimmed.split_once("://")
        && matches!(scheme, "http" | "https" | "git")
    {
        let (authority, path) = rest.split_once('/')?;
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, tail)| tail)
            .split(':')
            .next()?
            .trim();
        return normalize_remote_parts(host, path);
    }

    if let Some((left, path)) = trimmed.split_once(':')
        && !left.contains('/')
    {
        let host = left.rsplit_once('@').map_or(left, |(_, tail)| tail).trim();
        return normalize_remote_parts(host, path);
    }

    None
}

fn normalize_remote_parts(host: &str, path: &str) -> Option<(String, String)> {
    let host = host.trim().trim_matches('/');
    let path = path
        .trim()
        .trim_matches('/')
        .strip_suffix(".git")
        .unwrap_or(path.trim().trim_matches('/'))
        .trim_matches('/');
    if host.is_empty() || path.is_empty() || !path.contains('/') {
        return None;
    }
    Some((host.to_ascii_lowercase(), path.to_string()))
}

pub(crate) fn default_capabilities() -> BTreeMap<ForgeCapability, CapabilityState> {
    [
        (ForgeCapability::Issues, CapabilityState::Unknown),
        (ForgeCapability::IssueBoards, CapabilityState::Unknown),
        (ForgeCapability::MergeRequests, CapabilityState::Unknown),
        (ForgeCapability::Pipelines, CapabilityState::Unknown),
        (ForgeCapability::ApprovalSummary, CapabilityState::Unknown),
        (ForgeCapability::Discussions, CapabilityState::Unknown),
        (ForgeCapability::WorkItems, CapabilityState::Unknown),
    ]
    .into_iter()
    .collect()
}

pub(crate) async fn glab_api_json<T>(cwd: &str, host: &str, endpoint: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let output = run_command(
        "glab",
        &["api", "--hostname", host, endpoint],
        Some(Path::new(cwd)),
    )
    .await
    .with_context(|| format!("run glab api {endpoint}"))?;
    if !output.success {
        bail!(
            "glab api {endpoint} failed: {}",
            trim_error(&output.stderr, "unknown glab error")
        );
    }
    serde_json::from_str(&output.stdout)
        .with_context(|| format!("decode glab api response for {endpoint}"))
}

pub(crate) async fn glab_api_mutation_json<T>(
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
        args.push("-f".to_string());
        args.push(format!("{key}={value}"));
    }

    let borrowed = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = run_command("glab", &borrowed, Some(Path::new(cwd)))
        .await
        .with_context(|| format!("run glab api {method} {endpoint}"))?;
    if !output.success {
        bail!(
            "glab api {method} {endpoint} failed: {}",
            trim_error(&output.stderr, "unknown glab error")
        );
    }
    serde_json::from_str(&output.stdout)
        .with_context(|| format!("decode glab api response for {method} {endpoint}"))
}

fn json_id_to_string(value: &serde_json::Value) -> Result<String> {
    match value {
        serde_json::Value::Number(number) => Ok(number.to_string()),
        serde_json::Value::String(value) if !value.trim().is_empty() => Ok(value.clone()),
        _ => bail!("GitLab project id is missing or invalid"),
    }
}

fn percent_encode_project_path(path: &str) -> String {
    percent_encode_component(path)
}

pub(crate) fn percent_encode_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                output.push(char::from(*byte));
            }
            _ => {
                output.push('%');
                output.push(hex((*byte >> 4) & 0x0f));
                output.push(hex(*byte & 0x0f));
            }
        }
    }
    output
}

const fn hex(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        _ => (b'A' + value - 10) as char,
    }
}

#[derive(Debug)]
pub(crate) struct CommandOutput {
    pub(crate) success: bool,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) async fn run_command(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<CommandOutput> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("spawn {program}"))?;
    let stdout = child.stdout.take().context("capture stdout")?;
    let stderr = child.stderr.take().context("capture stderr")?;

    let result = timeout(COMMAND_TIMEOUT, async move {
        let stdout_task = tokio::spawn(read_capped(stdout, MAX_STDOUT_BYTES));
        let stderr_task = tokio::spawn(read_capped(stderr, MAX_STDERR_BYTES));
        let status = child.wait().await.context("wait for subprocess")?;
        let stdout = stdout_task.await.context("join stdout reader")??;
        let stderr = stderr_task.await.context("join stderr reader")??;
        Ok::<_, anyhow::Error>((status.success(), stdout, stderr))
    })
    .await
    .map_err(|_| anyhow!("{program} timed out after {}s", COMMAND_TIMEOUT.as_secs()))??;

    Ok(CommandOutput {
        success: result.0,
        stdout: String::from_utf8_lossy(&result.1).into_owned(),
        stderr: String::from_utf8_lossy(&result.2).into_owned(),
    })
}

async fn read_capped<R>(mut reader: R, cap: usize) -> Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut output = Vec::with_capacity(cap.min(16 * 1024));
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        if output.len() < cap {
            let remaining = cap - output.len();
            output.extend_from_slice(&buffer[..count.min(remaining)]);
        }
    }
    Ok(output)
}

pub(crate) fn trim_error(stderr: &str, fallback: &str) -> String {
    let value = stderr.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.chars().take(500).collect()
    }
}

pub(crate) fn first_nonempty_line(value: &str) -> Option<&str> {
    value.lines().map(str::trim).find(|line| !line.is_empty())
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
