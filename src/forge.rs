use crate::domain::ThreadId;
use crate::forge_github::{GitHubProvider, probe_github_with_remote};
pub(crate) use crate::forge_types::default_capabilities;
pub use crate::forge_types::{
    CapabilityState, ChangeRequestSummary, ForgeCapability, ForgeDoctorSnapshot, ForgeFreshness,
    ForgeFuture, ForgeIdentity, ForgeIssueSummary, ForgeObservation, ForgeProvider,
    ForgeProviderKind, ForgeReviewResult, ForgeReviewSummary, ForgeReviewTarget, IssueBoardSummary,
    PipelineSummary, RemoteIdentity,
};
use crate::latest_read::{ReadEnvelope, ReadFence, ReadScope, next_current};
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const FORGE_COMMAND_QUEUE_CAPACITY: usize = 128;
const FORGE_EVENT_QUEUE_CAPACITY: usize = 128;
const FORGE_MAX_CONCURRENCY: usize = 4;
const MAX_STDOUT_BYTES: usize = 1024 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;
const DEFAULT_PAGE_SIZE: usize = 20;

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
        (true, true) => {
            bail!("forge provider is ambiguous for host {host}: both gh and glab are authenticated")
        }
        (false, false) => {
            bail!("forge provider not configured for host {host}; authenticate with gh or glab")
        }
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
    Review(Box<ForgeReviewResult>),
}

pub struct ForgeHandle {
    command_tx: mpsc::Sender<ReadEnvelope<ForgeCommand>>,
    event_rx: mpsc::Receiver<ReadEnvelope<ForgeEvent>>,
    task: JoinHandle<()>,
    reads: ReadFence,
}

impl ForgeHandle {
    pub fn start() -> Self {
        Self::start_with_provider(Arc::new(RoutingForgeProvider))
    }

    pub fn start_with_provider(provider: Arc<dyn ForgeProvider>) -> Self {
        let (command_tx, command_rx) = mpsc::channel(FORGE_COMMAND_QUEUE_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel(FORGE_EVENT_QUEUE_CAPACITY);
        let task = tokio::spawn(run_actor(provider, command_rx, event_tx));
        Self {
            command_tx,
            event_rx,
            task,
            reads: ReadFence::default(),
        }
    }

    pub fn probe(&self, thread_id: ThreadId, cwd: String) -> Result<()> {
        self.queue_command(ForgeCommand::Probe { thread_id, cwd })
    }

    pub fn probe_review(&self, target: ForgeReviewTarget) -> Result<()> {
        self.queue_command(ForgeCommand::ProbeReview(target))
    }

    fn queue_command(&self, command: ForgeCommand) -> Result<()> {
        let scope = match &command {
            ForgeCommand::Probe { cwd, .. } => ReadScope::Snapshot(cwd.clone()),
            ForgeCommand::ProbeReview(target) => ReadScope::Review(target.thread_id.0.clone()),
        };
        self.reads.submit(&self.command_tx, scope, command, "Forge")
    }

    pub fn try_recv(&mut self) -> Option<ForgeEvent> {
        next_current(&mut self.event_rx, FORGE_EVENT_QUEUE_CAPACITY)
    }
}

impl Drop for ForgeHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run_actor(
    provider: Arc<dyn ForgeProvider>,
    mut command_rx: mpsc::Receiver<ReadEnvelope<ForgeCommand>>,
    event_tx: mpsc::Sender<ReadEnvelope<ForgeEvent>>,
) {
    let mut tasks = JoinSet::new();
    let mut command_open = true;

    loop {
        tokio::select! {
            joined = tasks.join_next(), if !tasks.is_empty() => {
                let _ = joined;
            }
            command = command_rx.recv(), if command_open && tasks.len() < FORGE_MAX_CONCURRENCY => {
                match command {
                    Some(ReadEnvelope { value: command, ticket }) => {
                        let provider = Arc::clone(&provider);
                        let event_tx = event_tx.clone();
                        tasks.spawn(async move {
                            let event = match command {
                                ForgeCommand::Probe { thread_id, cwd } => {
                                    ForgeEvent::Observation(Box::new(provider.probe(thread_id, cwd).await))
                                }
                                ForgeCommand::ProbeReview(target) => {
                                    let summary = provider.probe_review(target.clone()).await;
                                    ForgeEvent::Review(Box::new(ForgeReviewResult { target, summary }))
                                }
                            };
                            let _ = event_tx.send(ReadEnvelope { value: event, ticket }).await;
                        });
                    }
                    None => command_open = false,
                }
            }
            else => {
                if !command_open && tasks.is_empty() {
                    break;
                }
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
        .ok_or_else(|| anyhow!("unsupported Git remote URL (redacted)"))?;
    let remote_url = redact_git_remote_url(&selected.1)?;

    Ok(RemoteIdentity {
        remote_name: selected.0.clone(),
        remote_url,
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

fn redact_git_remote_url(raw: &str) -> Result<String> {
    let raw = raw.trim();
    // Display/diagnostic identity is not a Git command authority. Never retain
    // embedded URL userinfo, query secrets or fragments in Forge observations.
    if raw.contains("://") {
        let mut url = url::Url::parse(raw).context("parse Git remote URL (redacted)")?;
        url.set_username("")
            .map_err(|_| anyhow!("cannot redact Git remote username"))?;
        url.set_password(None)
            .map_err(|_| anyhow!("cannot redact Git remote password"))?;
        url.set_query(None);
        url.set_fragment(None);
        return Ok(url.to_string());
    }
    // SCP-style remotes have login@host:path. Keep the hostname/path without
    // retaining the username, which could be a credential-like identifier.
    let (authority, path) = raw
        .split_once(':')
        .ok_or_else(|| anyhow!("unsupported Git remote URL (redacted)"))?;
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    anyhow::ensure!(
        !host.is_empty() && !path.is_empty(),
        "invalid Git remote identity"
    );
    Ok(format!("{host}:{path}"))
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

async fn with_command_deadline<T>(
    program: &str,
    deadline: Duration,
    future: impl Future<Output = Result<T>>,
) -> Result<T> {
    timeout(deadline, future)
        .await
        .map_err(|_| anyhow!("{program} timed out after {}ms", deadline.as_millis()))?
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

    let result = with_command_deadline(program, COMMAND_TIMEOUT, async move {
        let stdout_task = tokio::spawn(read_capped(stdout, MAX_STDOUT_BYTES));
        let stderr_task = tokio::spawn(read_capped(stderr, MAX_STDERR_BYTES));
        let status = child.wait().await.context("wait for subprocess")?;
        let stdout = stdout_task.await.context("join stdout reader")??;
        let stderr = stderr_task.await.context("join stderr reader")??;
        Ok::<_, anyhow::Error>((status.success(), stdout, stderr))
    })
    .await?;

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
mod tests;

#[cfg(test)]
mod read_order_tests;
