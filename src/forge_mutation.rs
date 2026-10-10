use crate::forge::{
    ForgeIdentity, ForgeProviderKind, glab_api_json, glab_api_mutation_json,
    percent_encode_component,
};
use crate::forge_github_mutation::{GitHubAppliedResult, GitHubPreflight, GitHubReconciledOutcome};
use crate::operation::{OperationState, new_operation_id, now_unix_ms};
use crate::sqlite_store::SqliteStore;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio::task::{JoinHandle, JoinSet};

const MAX_MR_TITLE_BYTES: usize = 512;
const MAX_COMMENT_BYTES: usize = 64 * 1024;
const MUTATION_COMMAND_QUEUE_CAPACITY: usize = 32;
const MUTATION_EVENT_QUEUE_CAPACITY: usize = 64;
const MUTATION_MAX_CONCURRENCY: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ForgeMutationKind {
    CreateMergeRequest,
    CommentMergeRequest,
    ApproveMergeRequest,
    MergeMergeRequest,
}

impl ForgeMutationKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::CreateMergeRequest => "create-merge-request",
            Self::CommentMergeRequest => "comment-merge-request",
            Self::ApproveMergeRequest => "approve-merge-request",
            Self::MergeMergeRequest => "merge-merge-request",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeMutationPrecondition {
    pub key: String,
    pub expected: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeMutationPlan {
    pub operation_id: String,
    pub kind: ForgeMutationKind,
    pub provider: ForgeProviderKind,
    pub cwd: String,
    pub host: String,
    pub project_id: String,
    pub project_path: String,
    pub change_request_iid: Option<u64>,
    pub source_branch: Option<String>,
    pub target_branch: Option<String>,
    pub title: Option<String>,
    pub payload_bytes: Option<usize>,
    pub expected_side_effect: String,
    pub preconditions: Vec<ForgeMutationPrecondition>,
    pub planned_at_unix_ms: u64,
}

impl ForgeMutationPlan {
    pub fn create_merge_request(
        identity: &ForgeIdentity,
        cwd: String,
        source_branch: String,
        target_branch: String,
        title: String,
        planned_at_unix_ms: u64,
    ) -> Result<Self> {
        ensure_identity(identity)?;
        let source_branch = required_text("source branch", source_branch)?;
        let target_branch = required_text("target branch", target_branch)?;
        let title = required_text("merge request title", title)?;
        anyhow::ensure!(
            title.len() <= MAX_MR_TITLE_BYTES,
            "merge request title exceeds {MAX_MR_TITLE_BYTES} bytes"
        );
        anyhow::ensure!(
            source_branch != target_branch,
            "source and target branches must differ"
        );

        Ok(Self {
            operation_id: new_operation_id(planned_at_unix_ms),
            kind: ForgeMutationKind::CreateMergeRequest,
            provider: identity.provider,
            cwd,
            host: identity.host.clone(),
            project_id: identity.project_id.clone(),
            project_path: identity.path_with_namespace.clone(),
            change_request_iid: None,
            source_branch: Some(source_branch.clone()),
            target_branch: Some(target_branch.clone()),
            title: Some(title.clone()),
            payload_bytes: None,
            expected_side_effect: format!(
                "create one {} {source_branch} -> {target_branch} titled {title:?}",
                change_request_name(identity.provider)
            ),
            preconditions: vec![
                precondition("provider", identity.provider.label()),
                precondition("project-identity-current", "true"),
                precondition("default-branch-unchanged", "true"),
                precondition("source-branch-exists", "true"),
                precondition("target-branch-exists", "true"),
                precondition("matching-open-mr-absent", "true"),
            ],
            planned_at_unix_ms,
        })
    }

    pub fn comment_merge_request(
        identity: &ForgeIdentity,
        cwd: String,
        change_request_iid: u64,
        source_branch: String,
        target_branch: String,
        payload_bytes: usize,
        planned_at_unix_ms: u64,
    ) -> Result<Self> {
        ensure_identity(identity)?;
        anyhow::ensure!(change_request_iid > 0, "merge request iid must be positive");
        anyhow::ensure!(
            payload_bytes > 0 && payload_bytes <= MAX_COMMENT_BYTES,
            "comment must be 1..={MAX_COMMENT_BYTES} bytes"
        );

        Ok(Self {
            operation_id: new_operation_id(planned_at_unix_ms),
            kind: ForgeMutationKind::CommentMergeRequest,
            provider: identity.provider,
            cwd,
            host: identity.host.clone(),
            project_id: identity.project_id.clone(),
            project_path: identity.path_with_namespace.clone(),
            change_request_iid: Some(change_request_iid),
            source_branch: Some(required_text("source branch", source_branch)?),
            target_branch: Some(required_text("target branch", target_branch)?),
            title: None,
            payload_bytes: Some(payload_bytes),
            expected_side_effect: format!(
                "post one {payload_bytes}-byte comment to {} #{change_request_iid}",
                change_request_name(identity.provider)
            ),
            preconditions: vec![
                precondition("provider", identity.provider.label()),
                precondition("project-identity-current", "true"),
                precondition("merge-request-open", "true"),
                precondition("source-target-unchanged", "true"),
            ],
            planned_at_unix_ms,
        })
    }

    pub fn approve_merge_request(
        identity: &ForgeIdentity,
        cwd: String,
        change_request_iid: u64,
        source_branch: String,
        target_branch: String,
        planned_at_unix_ms: u64,
    ) -> Result<Self> {
        mr_plan(
            identity,
            cwd,
            MrPlanSpec {
                kind: ForgeMutationKind::ApproveMergeRequest,
                change_request_iid,
                source_branch,
                target_branch,
                expected: "approve exact forge change request as the authenticated user",
                planned_at_unix_ms,
            },
        )
    }

    pub fn merge_merge_request(
        identity: &ForgeIdentity,
        cwd: String,
        change_request_iid: u64,
        source_branch: String,
        target_branch: String,
        planned_at_unix_ms: u64,
    ) -> Result<Self> {
        let mut plan = mr_plan(
            identity,
            cwd,
            MrPlanSpec {
                kind: ForgeMutationKind::MergeMergeRequest,
                change_request_iid,
                source_branch,
                target_branch,
                expected: "merge exact forge change request under standard project policy",
                planned_at_unix_ms,
            },
        )?;
        plan.preconditions.extend([
            precondition("merge-request-not-draft", "true"),
            precondition(
                "blocking-discussions-resolved",
                "true when capability available",
            ),
            precondition("approval-rules-satisfied", "true when capability available"),
            precondition("head-pipeline-not-failed", "true when pipeline available"),
        ]);
        Ok(plan)
    }

    pub fn project_lock_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.provider.label(),
            self.host.to_ascii_lowercase(),
            self.project_id
        )
    }
}

struct MrPlanSpec {
    kind: ForgeMutationKind,
    change_request_iid: u64,
    source_branch: String,
    target_branch: String,
    expected: &'static str,
    planned_at_unix_ms: u64,
}

fn mr_plan(identity: &ForgeIdentity, cwd: String, spec: MrPlanSpec) -> Result<ForgeMutationPlan> {
    ensure_identity(identity)?;
    anyhow::ensure!(
        spec.change_request_iid > 0,
        "merge request iid must be positive"
    );
    Ok(ForgeMutationPlan {
        operation_id: new_operation_id(spec.planned_at_unix_ms),
        kind: spec.kind,
        provider: identity.provider,
        cwd,
        host: identity.host.clone(),
        project_id: identity.project_id.clone(),
        project_path: identity.path_with_namespace.clone(),
        change_request_iid: Some(spec.change_request_iid),
        source_branch: Some(required_text("source branch", spec.source_branch)?),
        target_branch: Some(required_text("target branch", spec.target_branch)?),
        title: None,
        payload_bytes: None,
        expected_side_effect: format!("{}: !{}", spec.expected, spec.change_request_iid),
        preconditions: vec![
            precondition("provider", identity.provider.label()),
            precondition("project-identity-current", "true"),
            precondition("merge-request-open", "true"),
            precondition("source-target-unchanged", "true"),
            precondition("head-sha-revalidated", "true"),
        ],
        planned_at_unix_ms: spec.planned_at_unix_ms,
    })
}

fn ensure_identity(identity: &ForgeIdentity) -> Result<()> {
    anyhow::ensure!(!identity.host.trim().is_empty(), "forge host is empty");
    anyhow::ensure!(
        !identity.project_id.trim().is_empty()
            && identity.project_id.chars().all(|ch| ch.is_ascii_digit()),
        "forge project/repository id must be numeric"
    );
    anyhow::ensure!(
        !identity.path_with_namespace.trim().is_empty(),
        "forge project/repository path is empty"
    );
    match identity.provider {
        ForgeProviderKind::GitLab => {}
        ForgeProviderKind::GitHub => {
            let mut parts = identity.path_with_namespace.split('/');
            let owner = parts.next().unwrap_or_default().trim();
            let repo = parts.next().unwrap_or_default().trim();
            anyhow::ensure!(
                !owner.is_empty() && !repo.is_empty() && parts.next().is_none(),
                "GitHub repository path must be exactly owner/repo"
            );
        }
    }
    Ok(())
}

const fn change_request_name(provider: ForgeProviderKind) -> &'static str {
    match provider {
        ForgeProviderKind::GitLab => "GitLab merge request",
        ForgeProviderKind::GitHub => "GitHub pull request",
    }
}

fn required_text(label: &str, value: String) -> Result<String> {
    let value = value.trim().to_string();
    anyhow::ensure!(!value.is_empty(), "{label} must not be empty");
    Ok(value)
}

fn precondition(key: &str, expected: &str) -> ForgeMutationPrecondition {
    ForgeMutationPrecondition {
        key: key.into(),
        expected: expected.into(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeMutationReceipt {
    pub operation_id: String,
    pub plan: ForgeMutationPlan,
    pub state: OperationState,
    pub started_at_unix_ms: Option<u64>,
    pub completed_at_unix_ms: Option<u64>,
    pub result_ref: Option<String>,
    pub verification: Option<String>,
    pub failure: Option<String>,
}

impl ForgeMutationReceipt {
    pub fn planned(plan: ForgeMutationPlan) -> Self {
        Self {
            operation_id: plan.operation_id.clone(),
            plan,
            state: OperationState::Planned,
            started_at_unix_ms: None,
            completed_at_unix_ms: None,
            result_ref: None,
            verification: None,
            failure: None,
        }
    }

    pub fn start(&mut self, now: u64) {
        self.state = OperationState::Executing;
        self.started_at_unix_ms = Some(now);
        self.failure = None;
    }

    pub fn succeed(&mut self, now: u64, result_ref: String, verification: String) {
        self.state = OperationState::Succeeded;
        self.completed_at_unix_ms = Some(now);
        self.result_ref = Some(result_ref);
        self.verification = Some(verification);
        self.failure = None;
    }

    pub fn fail(&mut self, now: u64, failure: String) {
        self.state = OperationState::Failed;
        self.completed_at_unix_ms = Some(now);
        self.failure = Some(failure);
    }

    pub fn outcome_unknown(&mut self, now: u64, failure: String) {
        self.state = OperationState::OutcomeUnknown;
        self.completed_at_unix_ms = Some(now);
        self.failure = Some(failure);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeMutationRequest {
    pub plan: ForgeMutationPlan,
    pub payload: Option<String>,
}

#[derive(Clone, Debug)]
pub enum ForgeMutationCommand {
    Execute(Box<ForgeMutationRequest>),
    Recover,
}

#[derive(Clone, Debug)]
pub enum ForgeMutationEvent {
    Receipt(Box<ForgeMutationReceipt>),
    Notice(String),
}

pub struct ForgeMutationHandle {
    command_tx: mpsc::Sender<ForgeMutationCommand>,
    event_rx: mpsc::Receiver<ForgeMutationEvent>,
    task: JoinHandle<()>,
}

impl ForgeMutationHandle {
    pub fn start(store: SqliteStore) -> Self {
        let (command_tx, command_rx) = mpsc::channel(MUTATION_COMMAND_QUEUE_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel(MUTATION_EVENT_QUEUE_CAPACITY);
        let locks = Arc::new(Mutex::new(BTreeMap::new()));
        let task = tokio::spawn(run_coordinator(store, locks, command_rx, event_tx));
        Self {
            command_tx,
            event_rx,
            task,
        }
    }

    pub fn execute(&self, request: ForgeMutationRequest) -> Result<()> {
        queue_mutation_command(
            &self.command_tx,
            ForgeMutationCommand::Execute(Box::new(request)),
        )
    }

    pub fn recover(&self) -> Result<()> {
        queue_mutation_command(&self.command_tx, ForgeMutationCommand::Recover)
    }

    pub fn try_recv(&mut self) -> Option<ForgeMutationEvent> {
        self.event_rx.try_recv().ok()
    }
}

fn queue_mutation_command(
    tx: &mpsc::Sender<ForgeMutationCommand>,
    command: ForgeMutationCommand,
) -> Result<()> {
    tx.try_send(command).map_err(|error| match error {
        mpsc::error::TrySendError::Full(_) => anyhow!("forge mutation coordinator queue is full"),
        mpsc::error::TrySendError::Closed(_) => {
            anyhow!("forge mutation coordinator is unavailable")
        }
    })
}

impl Drop for ForgeMutationHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

type ProjectLocks = Arc<Mutex<BTreeMap<String, Arc<Mutex<()>>>>>;

async fn run_coordinator(
    store: SqliteStore,
    locks: ProjectLocks,
    mut command_rx: mpsc::Receiver<ForgeMutationCommand>,
    event_tx: mpsc::Sender<ForgeMutationEvent>,
) {
    let mut tasks = JoinSet::new();
    let mut command_open = true;

    loop {
        tokio::select! {
            joined = tasks.join_next(), if !tasks.is_empty() => {
                let _ = joined;
            }
            command = command_rx.recv(), if command_open && tasks.len() < MUTATION_MAX_CONCURRENCY => {
                match command {
                    Some(command) => {
                        let store = store.clone();
                        let locks = locks.clone();
                        let event_tx = event_tx.clone();
                        tasks.spawn(async move {
                            match command {
                                ForgeMutationCommand::Execute(request) => {
                                    match execute_with_project_lock(&store, &locks, *request).await {
                                        Ok(receipt) => {
                                            let _ = event_tx
                                                .send(ForgeMutationEvent::Receipt(Box::new(receipt)))
                                                .await;
                                        }
                                        Err(error) => {
                                            let _ = event_tx
                                                .send(ForgeMutationEvent::Notice(format!(
                                                    "forge mutation coordinator failed: {error:#}"
                                                )))
                                                .await;
                                        }
                                    }
                                }
                                ForgeMutationCommand::Recover => {
                                    if let Err(error) =
                                        recover_incomplete(&store, &locks, &event_tx).await
                                    {
                                        let _ = event_tx
                                            .send(ForgeMutationEvent::Notice(format!(
                                                "forge mutation recovery failed: {error:#}"
                                            )))
                                            .await;
                                    }
                                }
                            }
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

async fn project_lock(locks: &ProjectLocks, plan: &ForgeMutationPlan) -> Arc<Mutex<()>> {
    let mut map = locks.lock().await;
    map.entry(plan.project_lock_key())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

async fn execute_with_project_lock(
    store: &SqliteStore,
    locks: &ProjectLocks,
    request: ForgeMutationRequest,
) -> Result<ForgeMutationReceipt> {
    let lock = project_lock(locks, &request.plan).await;
    let _guard = lock.lock().await;
    execute_request(store, request).await
}

#[derive(Clone, Debug)]
struct Preflight {
    authenticated_user_id: Option<u64>,
    merge_request_sha: Option<String>,
    github: Option<GitHubPreflight>,
    already_satisfied: Option<(String, String)>,
}

async fn execute_request(
    store: &SqliteStore,
    request: ForgeMutationRequest,
) -> Result<ForgeMutationReceipt> {
    validate_request_payload(&request)?;

    let mut receipt = match store.forge_mutation_receipt(&request.plan.operation_id)? {
        Some(existing) => {
            anyhow::ensure!(
                matches!(
                    existing.state,
                    OperationState::Planned | OperationState::OutcomeUnknown
                ),
                "forge operation {} is not executable from state {:?}",
                existing.operation_id,
                existing.state
            );
            anyhow::ensure!(
                existing.plan == request.plan,
                "forge operation plan changed after it was persisted"
            );
            if existing.state == OperationState::OutcomeUnknown {
                return reconcile_receipt(store, existing).await;
            }
            existing
        }
        None => {
            let receipt = ForgeMutationReceipt::planned(request.plan.clone());
            store.save_forge_mutation_receipt(&receipt)?;
            receipt
        }
    };

    let preflight = match validate_preconditions(&request.plan).await {
        Ok(preflight) => preflight,
        Err(error) => {
            receipt.fail(now_unix_ms(), format!("precondition failed: {error:#}"));
            store.save_forge_mutation_receipt(&receipt)?;
            return Ok(receipt);
        }
    };

    if let Some((result_ref, verification)) = preflight.already_satisfied {
        receipt.succeed(now_unix_ms(), result_ref, verification);
        store.save_forge_mutation_receipt(&receipt)?;
        return Ok(receipt);
    }

    receipt.start(now_unix_ms());
    store.save_forge_mutation_receipt(&receipt)?;

    match execute_mutation(&request, &preflight).await {
        Ok(applied) => match verify_success(&request, &preflight, applied).await {
            Ok((result_ref, verification)) => {
                receipt.succeed(now_unix_ms(), result_ref, verification);
                store.save_forge_mutation_receipt(&receipt)?;
                Ok(receipt)
            }
            Err(error) => {
                receipt.outcome_unknown(
                    now_unix_ms(),
                    format!("mutation command returned success but verification failed: {error:#}"),
                );
                store.save_forge_mutation_receipt(&receipt)?;
                reconcile_receipt(store, receipt).await
            }
        },
        Err(error) => {
            receipt.outcome_unknown(
                now_unix_ms(),
                format!("mutation command failed after execution began: {error:#}"),
            );
            store.save_forge_mutation_receipt(&receipt)?;
            reconcile_receipt(store, receipt).await
        }
    }
}

fn validate_request_payload(request: &ForgeMutationRequest) -> Result<()> {
    match request.plan.kind {
        ForgeMutationKind::CommentMergeRequest => {
            let payload = request
                .payload
                .as_deref()
                .context("comment mutation payload is missing")?;
            let bytes = payload.len();
            anyhow::ensure!(
                Some(bytes) == request.plan.payload_bytes,
                "comment payload length changed after planning"
            );
            anyhow::ensure!(
                bytes > 0 && bytes <= MAX_COMMENT_BYTES,
                "comment payload size is invalid"
            );
        }
        _ => {
            anyhow::ensure!(
                request.payload.is_none(),
                "unexpected payload for {}",
                request.plan.kind.label()
            );
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct GitLabProjectCheck {
    path_with_namespace: String,
    default_branch: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitLabMergeRequest {
    iid: u64,
    title: String,
    state: String,
    source_branch: String,
    target_branch: String,
    source_project_id: Option<u64>,
    target_project_id: Option<u64>,
    web_url: String,
    sha: Option<String>,
    #[serde(default)]
    draft: bool,
    detailed_merge_status: Option<String>,
    blocking_discussions_resolved: Option<bool>,
    head_pipeline: Option<GitLabPipelineRef>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitLabPipelineRef {
    status: String,
}

#[derive(Debug, Deserialize)]
struct GitLabBranch {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GitLabNote {
    id: u64,
    body: String,
}

#[derive(Debug, Deserialize)]
struct GitLabUser {
    id: u64,
}

#[derive(Debug, Deserialize)]
struct GitLabApprovalUser {
    user: GitLabApprovalUserRef,
}

#[derive(Debug, Deserialize)]
struct GitLabApprovalUserRef {
    id: u64,
}

#[derive(Debug, Deserialize)]
struct GitLabApprovals {
    approvals_left: Option<u64>,
    #[serde(default)]
    approved_by: Vec<GitLabApprovalUser>,
}

#[derive(Debug, Deserialize)]
struct GitLabApprovalState {
    #[serde(default)]
    rules: Vec<GitLabApprovalRuleState>,
}

#[derive(Debug, Deserialize)]
struct GitLabApprovalRuleState {
    #[serde(default)]
    approvals_required: u64,
    #[serde(default)]
    approved: bool,
}

async fn validate_preconditions(plan: &ForgeMutationPlan) -> Result<Preflight> {
    if plan.provider == ForgeProviderKind::GitHub {
        let github = crate::forge_github_mutation::validate_preconditions(plan).await?;
        return Ok(Preflight {
            authenticated_user_id: None,
            merge_request_sha: github.head_sha.clone(),
            already_satisfied: github.already_satisfied.clone(),
            github: Some(github),
        });
    }
    let project: GitLabProjectCheck =
        glab_api_json(&plan.cwd, &plan.host, &project_endpoint(plan)).await?;
    anyhow::ensure!(
        project.path_with_namespace == plan.project_path,
        "GitLab project identity changed: expected {}, observed {}",
        plan.project_path,
        project.path_with_namespace
    );

    match plan.kind {
        ForgeMutationKind::CreateMergeRequest => {
            let source = plan
                .source_branch
                .as_deref()
                .context("create MR plan missing source branch")?;
            let target = plan
                .target_branch
                .as_deref()
                .context("create MR plan missing target branch")?;
            anyhow::ensure!(
                project.default_branch.as_deref() == Some(target),
                "GitLab default branch changed: planned {target:?}, observed {:?}",
                project.default_branch
            );
            branch(plan, source).await?;
            branch(plan, target).await?;
            let existing = matching_merge_requests(plan, source, target).await?;
            anyhow::ensure!(
                existing.is_empty(),
                "matching open merge request already exists: {}",
                existing
                    .iter()
                    .map(|mr| format!("!{}", mr.iid))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            Ok(Preflight {
                authenticated_user_id: None,
                merge_request_sha: None,
                github: None,
                already_satisfied: None,
            })
        }
        ForgeMutationKind::CommentMergeRequest => {
            validate_exact_open_mr(plan).await?;
            Ok(Preflight {
                authenticated_user_id: None,
                merge_request_sha: None,
                github: None,
                already_satisfied: None,
            })
        }
        ForgeMutationKind::ApproveMergeRequest => {
            let mr = validate_exact_open_mr(plan).await?;
            let sha = required_mr_sha(&mr)?;
            let user: GitLabUser = glab_api_json(&plan.cwd, &plan.host, "/user").await?;
            let approvals = approvals(plan, mr.iid).await?;
            if approvals
                .approved_by
                .iter()
                .any(|approval| approval.user.id == user.id)
            {
                return Ok(Preflight {
                    authenticated_user_id: Some(user.id),
                    merge_request_sha: Some(sha),
                    github: None,
                    already_satisfied: Some((
                        mr_ref(plan, mr.iid),
                        "authenticated GitLab user is already present in approved_by".into(),
                    )),
                });
            }
            Ok(Preflight {
                authenticated_user_id: Some(user.id),
                merge_request_sha: Some(sha),
                github: None,
                already_satisfied: None,
            })
        }
        ForgeMutationKind::MergeMergeRequest => {
            let mr = validate_exact_open_mr(plan).await?;
            let sha = required_mr_sha(&mr)?;
            anyhow::ensure!(!mr.draft, "merge request is still a draft");
            anyhow::ensure!(
                mr.blocking_discussions_resolved == Some(true),
                "GitLab merge discussions must be explicitly resolved"
            );
            require_gitlab_merge_ready(mr.detailed_merge_status.as_deref())?;
            if let Some(pipeline) = &mr.head_pipeline {
                anyhow::ensure!(
                    pipeline.status.eq_ignore_ascii_case("success"),
                    "GitLab head pipeline is not successful: {}",
                    pipeline.status
                );
            }
            require_gitlab_approvals(plan, mr.iid).await?;

            Ok(Preflight {
                authenticated_user_id: None,
                merge_request_sha: Some(sha),
                github: None,
                already_satisfied: None,
            })
        }
    }
}

async fn branch(plan: &ForgeMutationPlan, name: &str) -> Result<GitLabBranch> {
    let endpoint = format!(
        "{}/repository/branches/{}",
        project_endpoint(plan),
        percent_encode_component(name)
    );
    let branch: GitLabBranch = glab_api_json(&plan.cwd, &plan.host, &endpoint)
        .await
        .with_context(|| format!("resolve GitLab branch {name}"))?;
    anyhow::ensure!(branch.name == name, "GitLab branch identity mismatch");
    Ok(branch)
}

fn is_own_project_mr(mr: &GitLabMergeRequest, plan: &ForgeMutationPlan) -> Result<bool> {
    let source_id = mr
        .source_project_id
        .context("GitLab MR source project identity is unavailable")?;
    let target_id = mr
        .target_project_id
        .context("GitLab MR target project identity is unavailable")?;
    anyhow::ensure!(
        target_id.to_string() == plan.project_id,
        "GitLab MR target project identity does not match the selected project"
    );
    Ok(source_id.to_string() == plan.project_id)
}

async fn validate_exact_open_mr(plan: &ForgeMutationPlan) -> Result<GitLabMergeRequest> {
    let iid = plan
        .change_request_iid
        .context("merge request mutation plan missing iid")?;
    let mr = get_mr(plan, iid).await?;
    anyhow::ensure!(mr.state == "opened", "merge request !{iid} is {}", mr.state);
    if let Some(source) = &plan.source_branch {
        // Same-named branches in fork projects are not local branch identity.
        anyhow::ensure!(
            is_own_project_mr(&mr, plan)?,
            "GitLab MR source belongs to a fork; local branch mutation refused"
        );
        anyhow::ensure!(
            mr.source_branch == *source,
            "merge request source branch changed: expected {source}, observed {}",
            mr.source_branch
        );
    }
    if let Some(target) = &plan.target_branch {
        anyhow::ensure!(
            mr.target_branch == *target,
            "merge request target branch changed: expected {target}, observed {}",
            mr.target_branch
        );
    }
    Ok(mr)
}

fn required_mr_sha(mr: &GitLabMergeRequest) -> Result<String> {
    let sha = mr
        .sha
        .as_deref()
        .map(str::trim)
        .filter(|sha| !sha.is_empty())
        .context("GitLab merge request response is missing HEAD sha")?;
    Ok(sha.to_string())
}

async fn get_mr(plan: &ForgeMutationPlan, iid: u64) -> Result<GitLabMergeRequest> {
    glab_api_json(
        &plan.cwd,
        &plan.host,
        &format!("{}/merge_requests/{iid}", project_endpoint(plan)),
    )
    .await
}

async fn approvals(plan: &ForgeMutationPlan, iid: u64) -> Result<GitLabApprovals> {
    glab_api_json(
        &plan.cwd,
        &plan.host,
        &format!("{}/merge_requests/{iid}/approvals", project_endpoint(plan)),
    )
    .await
}

async fn approval_state(plan: &ForgeMutationPlan, iid: u64) -> Result<GitLabApprovalState> {
    glab_api_json(
        &plan.cwd,
        &plan.host,
        &format!(
            "{}/merge_requests/{iid}/approval_state",
            project_endpoint(plan)
        ),
    )
    .await
}

fn require_gitlab_merge_ready(status: Option<&str>) -> Result<()> {
    anyhow::ensure!(
        status.is_some_and(|value| value.eq_ignore_ascii_case("mergeable")),
        "GitLab merge readiness is not confirmed: {status:?}"
    );
    Ok(())
}

fn require_gitlab_approval_fallback(approvals: &GitLabApprovals) -> Result<()> {
    let remaining = approvals
        .approvals_left
        .context("GitLab approval_state unavailable; fallback approvals_left is unknown")?;
    anyhow::ensure!(remaining == 0, "GitLab merge needs {remaining} approvals");
    Ok(())
}

async fn require_gitlab_approvals(plan: &ForgeMutationPlan, iid: u64) -> Result<()> {
    match approval_state(plan, iid).await {
        Ok(state) => {
            let pending = unsatisfied_required_approval_rules(&state);
            anyhow::ensure!(pending == 0, "GitLab merge has {pending} unsatisfied rules");
            Ok(())
        }
        Err(_) => {
            let fallback = approvals(plan, iid)
                .await
                .context("both GitLab approval_state and approvals fallback unavailable")?;
            require_gitlab_approval_fallback(&fallback)
        }
    }
}

fn unsatisfied_required_approval_rules(state: &GitLabApprovalState) -> usize {
    state
        .rules
        .iter()
        .filter(|rule| rule.approvals_required > 0 && !rule.approved)
        .count()
}

async fn matching_merge_requests(
    plan: &ForgeMutationPlan,
    source: &str,
    target: &str,
) -> Result<Vec<GitLabMergeRequest>> {
    let prefix = format!(
        "{}/merge_requests?scope=all&state=opened&source_branch={}&target_branch={}",
        project_endpoint(plan),
        percent_encode_component(source),
        percent_encode_component(target)
    );
    let entries: Vec<GitLabMergeRequest> = crate::forge::bounded_review_pages(|page| {
        let endpoint = format!(
            "{prefix}&per_page={}&page={page}",
            crate::forge::REVIEW_PAGE_SIZE
        );
        async move { glab_api_json(&plan.cwd, &plan.host, &endpoint).await }
    })
    .await?;
    let mut matches = Vec::new();
    for mr in entries {
        if mr.source_branch == source && mr.target_branch == target && is_own_project_mr(&mr, plan)?
        {
            matches.push(mr);
        }
    }
    Ok(matches)
}

fn project_endpoint(plan: &ForgeMutationPlan) -> String {
    format!("/projects/{}", plan.project_id)
}

#[derive(Clone, Debug)]
enum AppliedResult {
    MergeRequest(u64),
    Note(u64),
    Approval(u64),
    Merge(u64),
    GitHub(GitHubAppliedResult),
}

async fn execute_mutation(
    request: &ForgeMutationRequest,
    preflight: &Preflight,
) -> Result<AppliedResult> {
    let plan = &request.plan;
    if plan.provider == ForgeProviderKind::GitHub {
        let github = preflight
            .github
            .as_ref()
            .context("GitHub mutation preflight is missing")?;
        return crate::forge_github_mutation::execute_mutation(request, github)
            .await
            .map(AppliedResult::GitHub);
    }
    match plan.kind {
        ForgeMutationKind::CreateMergeRequest => {
            let source = plan
                .source_branch
                .as_deref()
                .context("create MR plan missing source branch")?;
            let target = plan
                .target_branch
                .as_deref()
                .context("create MR plan missing target branch")?;
            let title = plan
                .title
                .as_deref()
                .context("create MR plan missing title")?;
            let mr: GitLabMergeRequest = glab_api_mutation_json(
                &plan.cwd,
                &plan.host,
                "POST",
                &format!("{}/merge_requests", project_endpoint(plan)),
                &[
                    ("source_branch", source),
                    ("target_branch", target),
                    ("title", title),
                ],
            )
            .await?;
            Ok(AppliedResult::MergeRequest(mr.iid))
        }
        ForgeMutationKind::CommentMergeRequest => {
            let iid = plan
                .change_request_iid
                .context("comment plan missing merge request iid")?;
            let body = request
                .payload
                .as_deref()
                .context("comment payload missing")?;
            let note: GitLabNote = glab_api_mutation_json(
                &plan.cwd,
                &plan.host,
                "POST",
                &format!("{}/merge_requests/{iid}/notes", project_endpoint(plan)),
                &[("body", body)],
            )
            .await?;
            Ok(AppliedResult::Note(note.id))
        }
        ForgeMutationKind::ApproveMergeRequest => {
            let iid = plan
                .change_request_iid
                .context("approve plan missing merge request iid")?;
            let user_id = preflight
                .authenticated_user_id
                .context("approve preflight missing authenticated user")?;
            let sha = preflight
                .merge_request_sha
                .as_deref()
                .context("approve preflight missing merge request HEAD sha")?;
            let _: serde_json::Value = glab_api_mutation_json(
                &plan.cwd,
                &plan.host,
                "POST",
                &format!("{}/merge_requests/{iid}/approve", project_endpoint(plan)),
                &[("sha", sha)],
            )
            .await?;
            Ok(AppliedResult::Approval(user_id))
        }
        ForgeMutationKind::MergeMergeRequest => {
            let iid = plan
                .change_request_iid
                .context("merge plan missing merge request iid")?;
            let sha = preflight
                .merge_request_sha
                .as_deref()
                .context("merge preflight missing merge request HEAD sha")?;
            let _: serde_json::Value = glab_api_mutation_json(
                &plan.cwd,
                &plan.host,
                "PUT",
                &format!("{}/merge_requests/{iid}/merge", project_endpoint(plan)),
                &[("sha", sha)],
            )
            .await?;
            Ok(AppliedResult::Merge(iid))
        }
    }
}

async fn verify_success(
    request: &ForgeMutationRequest,
    _preflight: &Preflight,
    applied: AppliedResult,
) -> Result<(String, String)> {
    let plan = &request.plan;
    match applied {
        AppliedResult::GitHub(applied) => {
            crate::forge_github_mutation::verify_success(request, applied).await
        }
        AppliedResult::MergeRequest(iid) => {
            let mr = get_mr(plan, iid).await?;
            let source = plan.source_branch.as_deref().context("missing source")?;
            let target = plan.target_branch.as_deref().context("missing target")?;
            let title = plan.title.as_deref().context("missing title")?;
            anyhow::ensure!(mr.state == "opened", "created MR is not open");
            anyhow::ensure!(mr.source_branch == source, "created MR source mismatch");
            anyhow::ensure!(mr.target_branch == target, "created MR target mismatch");
            anyhow::ensure!(mr.title == title, "created MR title mismatch");
            Ok((
                mr_ref(plan, iid),
                format!(
                    "GitLab readback confirms open MR !{iid} {source} -> {target}: {}",
                    mr.web_url
                ),
            ))
        }
        AppliedResult::Note(note_id) => {
            let iid = plan
                .change_request_iid
                .context("comment plan missing merge request iid")?;
            let note: GitLabNote = glab_api_json(
                &plan.cwd,
                &plan.host,
                &format!(
                    "{}/merge_requests/{iid}/notes/{note_id}",
                    project_endpoint(plan)
                ),
            )
            .await?;
            let payload = request
                .payload
                .as_deref()
                .context("comment payload missing")?;
            anyhow::ensure!(note.body == payload, "GitLab note readback body mismatch");
            Ok((
                format!("{}#note-{note_id}", mr_ref(plan, iid)),
                format!("GitLab readback confirms comment note {note_id}"),
            ))
        }
        AppliedResult::Approval(user_id) => {
            let iid = plan
                .change_request_iid
                .context("approve plan missing merge request iid")?;
            let approval = approvals(plan, iid).await?;
            anyhow::ensure!(
                approval
                    .approved_by
                    .iter()
                    .any(|entry| entry.user.id == user_id),
                "authenticated user is absent from approved_by after approval"
            );
            Ok((
                mr_ref(plan, iid),
                "GitLab approval readback contains authenticated user".into(),
            ))
        }
        AppliedResult::Merge(iid) => {
            let mr = get_mr(plan, iid).await?;
            anyhow::ensure!(mr.state == "merged", "merge request state is {}", mr.state);
            Ok((
                mr_ref(plan, iid),
                format!("GitLab readback confirms MR !{iid} is merged"),
            ))
        }
    }
}

fn mr_ref(plan: &ForgeMutationPlan, iid: u64) -> String {
    format!(
        "gitlab://{}/projects/{}/merge-requests/{iid}",
        plan.host, plan.project_id
    )
}

async fn reconcile_receipt(
    store: &SqliteStore,
    mut receipt: ForgeMutationReceipt,
) -> Result<ForgeMutationReceipt> {
    let now = now_unix_ms();
    match reconcile_outcome(&receipt.plan).await {
        Ok(ReconciledOutcome::Succeeded {
            result_ref,
            verification,
        }) => receipt.succeed(now, result_ref, verification),
        Ok(ReconciledOutcome::Unknown(reason)) => receipt.outcome_unknown(now, reason),
        Err(error) => receipt.outcome_unknown(now, format!("reconciliation failed: {error:#}")),
    }
    store.save_forge_mutation_receipt(&receipt)?;
    Ok(receipt)
}

enum ReconciledOutcome {
    Succeeded {
        result_ref: String,
        verification: String,
    },
    Unknown(String),
}

async fn reconcile_outcome(plan: &ForgeMutationPlan) -> Result<ReconciledOutcome> {
    if plan.provider == ForgeProviderKind::GitHub {
        return match crate::forge_github_mutation::reconcile_outcome(plan).await? {
            GitHubReconciledOutcome::Succeeded {
                result_ref,
                verification,
            } => Ok(ReconciledOutcome::Succeeded {
                result_ref,
                verification,
            }),
            GitHubReconciledOutcome::Unknown(reason) => Ok(ReconciledOutcome::Unknown(reason)),
        };
    }
    match plan.kind {
        ForgeMutationKind::CreateMergeRequest => {
            let source = plan
                .source_branch
                .as_deref()
                .context("missing source branch")?;
            let target = plan
                .target_branch
                .as_deref()
                .context("missing target branch")?;
            let title = plan.title.as_deref().context("missing title")?;
            let matches = matching_merge_requests(plan, source, target).await?;
            let exact = matches
                .into_iter()
                .filter(|mr| mr.title == title)
                .collect::<Vec<_>>();
            match exact.as_slice() {
                [mr] => Ok(ReconciledOutcome::Succeeded {
                    result_ref: mr_ref(plan, mr.iid),
                    verification: format!(
                        "reconciled after uncertain outcome: GitLab contains exact open MR !{}",
                        mr.iid
                    ),
                }),
                [] => Ok(ReconciledOutcome::Unknown(
                    "exact matching open MR is not visible after an uncertain create outcome; never retry blindly"
                        .into(),
                )),
                _ => Ok(ReconciledOutcome::Unknown(
                    "multiple exact matching open MRs prevent safe reconciliation".into(),
                )),
            }
        }
        ForgeMutationKind::CommentMergeRequest => Ok(ReconciledOutcome::Unknown(
            "comment outcome cannot be proven after payload left memory; never retry blindly"
                .into(),
        )),
        ForgeMutationKind::ApproveMergeRequest => {
            let iid = plan
                .change_request_iid
                .context("approve plan missing iid")?;
            let user: GitLabUser = glab_api_json(&plan.cwd, &plan.host, "/user").await?;
            let approvals = approvals(plan, iid).await?;
            if approvals
                .approved_by
                .iter()
                .any(|entry| entry.user.id == user.id)
            {
                Ok(ReconciledOutcome::Succeeded {
                    result_ref: mr_ref(plan, iid),
                    verification:
                        "reconciled after uncertain outcome: authenticated user is approved".into(),
                })
            } else {
                Ok(ReconciledOutcome::Unknown(
                    "authenticated user is not visible in approved_by after an uncertain approval outcome; never retry blindly"
                        .into(),
                ))
            }
        }
        ForgeMutationKind::MergeMergeRequest => {
            let iid = plan.change_request_iid.context("merge plan missing iid")?;
            let mr = get_mr(plan, iid).await?;
            match mr.state.as_str() {
                "merged" => Ok(ReconciledOutcome::Succeeded {
                    result_ref: mr_ref(plan, iid),
                    verification: "reconciled after uncertain outcome: GitLab reports merged"
                        .into(),
                }),
                "opened" => Ok(ReconciledOutcome::Unknown(
                    "merge request remains open after an uncertain merge outcome; never retry blindly"
                        .into(),
                )),
                other => Ok(ReconciledOutcome::Unknown(format!(
                    "merge request is in unexpected state {other:?}"
                ))),
            }
        }
    }
}

async fn recover_incomplete(
    store: &SqliteStore,
    locks: &ProjectLocks,
    tx: &mpsc::Sender<ForgeMutationEvent>,
) -> Result<()> {
    let receipts = store.load_recoverable_forge_mutation_receipts()?;
    for mut receipt in receipts {
        let lock = project_lock(locks, &receipt.plan).await;
        let _guard = lock.lock().await;

        if receipt.state == OperationState::Planned {
            receipt.fail(
                now_unix_ms(),
                "planned forge operation was never confirmed/executed before restart".into(),
            );
            store.save_forge_mutation_receipt(&receipt)?;
        } else {
            if receipt.state == OperationState::Executing {
                receipt.outcome_unknown(
                    now_unix_ms(),
                    "process restarted while forge operation was executing".into(),
                );
                store.save_forge_mutation_receipt(&receipt)?;
            }
            receipt = reconcile_receipt(store, receipt).await?;
        }
        let _ = tx
            .send(ForgeMutationEvent::Receipt(Box::new(receipt)))
            .await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
        let source = include_str!("forge_mutation.rs");
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
}
