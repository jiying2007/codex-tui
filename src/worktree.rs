use crate::domain::LocalRepoIdentity;
use crate::operation::{
    ManagedWorktreeRecord, MutationScope, OperationKind, OperationPlan, OperationReceipt,
    OperationState, now_unix_ms, scopes_overlap,
};
use crate::sqlite_store::SqliteStore;
use anyhow::{Context, Result, anyhow};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

const MUTATION_TIMEOUT: Duration = Duration::from_secs(15);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_MUTATION_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug)]
pub struct MutationRequest {
    pub plan: OperationPlan,
    pub active_scopes: Vec<MutationScope>,
}

#[derive(Clone, Debug)]
pub enum MutationCommand {
    Execute(Box<MutationRequest>),
    Recover,
}

#[derive(Clone, Debug)]
pub enum MutationEvent {
    Receipt(Box<OperationReceipt>),
    ManagedWorktrees(Vec<ManagedWorktreeRecord>),
    Notice(String),
}

pub struct WorktreeMutationHandle {
    command_tx: mpsc::UnboundedSender<MutationCommand>,
    event_rx: mpsc::UnboundedReceiver<MutationEvent>,
    task: JoinHandle<()>,
}

impl WorktreeMutationHandle {
    pub fn start(store: SqliteStore) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let locks = Arc::new(Mutex::new(BTreeMap::new()));
        let task = tokio::spawn(run_coordinator(store, locks, command_rx, event_tx));
        Self {
            command_tx,
            event_rx,
            task,
        }
    }

    pub fn execute(&self, request: MutationRequest) -> Result<()> {
        self.command_tx
            .send(MutationCommand::Execute(Box::new(request)))
            .map_err(|_| anyhow!("worktree mutation coordinator is unavailable"))
    }

    pub fn recover(&self) -> Result<()> {
        self.command_tx
            .send(MutationCommand::Recover)
            .map_err(|_| anyhow!("worktree mutation coordinator is unavailable"))
    }

    pub fn try_recv(&mut self) -> Option<MutationEvent> {
        self.event_rx.try_recv().ok()
    }
}

impl Drop for WorktreeMutationHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

type RepoLocks = Arc<Mutex<BTreeMap<String, Arc<Mutex<()>>>>>;

async fn run_coordinator(
    store: SqliteStore,
    locks: RepoLocks,
    mut command_rx: mpsc::UnboundedReceiver<MutationCommand>,
    event_tx: mpsc::UnboundedSender<MutationEvent>,
) {
    while let Some(command) = command_rx.recv().await {
        match command {
            MutationCommand::Execute(request) => {
                let request = *request;
                let store = store.clone();
                let locks = locks.clone();
                let event_tx = event_tx.clone();
                tokio::spawn(async move {
                    let receipt = execute_with_repo_lock(&store, &locks, request).await;
                    match receipt {
                        Ok(receipt) => {
                            let _ = event_tx.send(MutationEvent::Receipt(Box::new(receipt)));
                            emit_inventory(&store, &event_tx);
                        }
                        Err(error) => {
                            let _ = event_tx.send(MutationEvent::Notice(format!(
                                "worktree mutation coordinator failed: {error:#}"
                            )));
                        }
                    }
                });
            }
            MutationCommand::Recover => {
                let store = store.clone();
                let locks = locks.clone();
                let event_tx = event_tx.clone();
                tokio::spawn(async move {
                    if let Err(error) = recover_incomplete(&store, &locks, &event_tx).await {
                        let _ = event_tx.send(MutationEvent::Notice(format!(
                            "worktree recovery failed: {error:#}"
                        )));
                    }
                    emit_inventory(&store, &event_tx);
                });
            }
        }
    }
}

fn emit_inventory(store: &SqliteStore, tx: &mpsc::UnboundedSender<MutationEvent>) {
    match store.load_managed_worktrees() {
        Ok(records) => {
            let _ = tx.send(MutationEvent::ManagedWorktrees(records));
        }
        Err(error) => {
            let _ = tx.send(MutationEvent::Notice(format!(
                "managed worktree inventory unavailable: {error:#}"
            )));
        }
    }
}

async fn repo_lock(locks: &RepoLocks, repo: &LocalRepoIdentity) -> Arc<Mutex<()>> {
    let key = repo.git_common_dir.clone();
    let mut map = locks.lock().await;
    map.entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

async fn execute_with_repo_lock(
    store: &SqliteStore,
    locks: &RepoLocks,
    request: MutationRequest,
) -> Result<OperationReceipt> {
    let lock = repo_lock(locks, &request.plan.repo).await;
    let _guard = lock.lock().await;
    execute_request(store, request).await
}

async fn execute_request(
    store: &SqliteStore,
    request: MutationRequest,
) -> Result<OperationReceipt> {
    let mut receipt = match store.operation_receipt(&request.plan.operation_id)? {
        Some(existing) => {
            anyhow::ensure!(
                matches!(
                    existing.state,
                    OperationState::Planned | OperationState::OutcomeUnknown
                ),
                "operation {} is not executable from state {:?}",
                existing.operation_id,
                existing.state
            );
            if existing.state == OperationState::OutcomeUnknown {
                return reconcile_receipt(store, existing).await;
            }
            anyhow::ensure!(
                existing.plan == request.plan,
                "operation {} plan does not match persisted receipt",
                existing.operation_id
            );
            existing
        }
        None => {
            let receipt = OperationReceipt::planned(request.plan);
            store.save_operation_receipt(&receipt)?;
            receipt
        }
    };

    if let Err(error) = check_preconditions(store, &receipt.plan, &request.active_scopes).await {
        receipt.fail(now_unix_ms(), error.to_string());
        store.save_operation_receipt(&receipt)?;
        return Ok(receipt);
    }

    receipt.start(now_unix_ms());
    store.save_operation_receipt(&receipt)?;

    if receipt.plan.kind == OperationKind::AdoptWorktree {
        match verify_success(store, &receipt.plan).await {
            Ok((result_ref, verification)) => {
                receipt.succeed(now_unix_ms(), result_ref, verification);
            }
            Err(error) => {
                receipt.fail(
                    now_unix_ms(),
                    format!("worktree adoption failed: {error:#}"),
                );
            }
        }
        store.save_operation_receipt(&receipt)?;
        return Ok(receipt);
    }

    let command = run_git_mutation(&receipt.plan.cwd, &receipt.plan.argv).await;
    match command {
        Ok(output) if output.success => match verify_success(store, &receipt.plan).await {
            Ok((result_ref, verification)) => {
                receipt.succeed(now_unix_ms(), result_ref, verification);
            }
            Err(error) => {
                receipt.outcome_unknown(
                    now_unix_ms(),
                    format!("mutation exited successfully but verification failed: {error:#}"),
                );
            }
        },
        Ok(output) => {
            receipt.fail(
                now_unix_ms(),
                format!(
                    "git mutation failed with exit status {}: {}",
                    output
                        .code
                        .map_or_else(|| "unknown".into(), |code| code.to_string()),
                    output.stderr.trim()
                ),
            );
        }
        Err(error) => {
            receipt.outcome_unknown(now_unix_ms(), format!("{error:#}"));
        }
    }

    store.save_operation_receipt(&receipt)?;

    if receipt.state == OperationState::OutcomeUnknown {
        return reconcile_receipt(store, receipt).await;
    }

    Ok(receipt)
}

async fn check_preconditions(
    store: &SqliteStore,
    plan: &OperationPlan,
    active_scopes: &[MutationScope],
) -> Result<()> {
    match plan.kind {
        OperationKind::CreateWorktree => {
            let target = plan
                .target_worktree
                .as_deref()
                .context("create plan missing target worktree")?;
            anyhow::ensure!(
                !Path::new(target).exists(),
                "target worktree path already exists: {target}"
            );
            let branch = plan
                .target_branch
                .as_deref()
                .context("create plan missing target branch")?;
            anyhow::ensure!(
                !branch_exists(&plan.cwd, branch).await?,
                "target branch already exists: {branch}"
            );
        }
        OperationKind::RemoveWorktree => {
            let target = plan
                .target_worktree
                .as_deref()
                .context("remove plan missing target worktree")?;
            let managed = store
                .managed_worktree(&plan.repo.git_common_dir, target)?
                .context("refusing to remove a worktree not managed/adopted by codex-tui")?;
            anyhow::ensure!(
                managed.canonical_path == target,
                "managed worktree identity mismatch"
            );
            anyhow::ensure!(
                worktree_is_clean(target).await?,
                "refusing to remove dirty worktree: {target}"
            );
            let target_scope = MutationScope {
                repo: Some(plan.repo.clone()),
                writable_roots: vec![target.to_string()],
                confidence: crate::operation::MutationScopeConfidence::ExactWorktree,
            };
            anyhow::ensure!(
                !active_scopes
                    .iter()
                    .any(|scope| scopes_overlap(&target_scope, scope)),
                "refusing to remove worktree while an active mutation scope overlaps it"
            );
        }
        OperationKind::DeleteBranch => {
            let branch = plan
                .target_branch
                .as_deref()
                .context("delete-branch plan missing branch")?;
            anyhow::ensure!(
                branch_exists(&plan.cwd, branch).await?,
                "branch does not exist: {branch}"
            );
            let inventory = list_worktrees(&plan.cwd).await?;
            anyhow::ensure!(
                !inventory
                    .iter()
                    .any(|worktree| worktree.branch.as_deref() == Some(branch)),
                "branch is checked out in a worktree: {branch}"
            );
        }
        OperationKind::AdoptWorktree => {
            let target = plan
                .target_worktree
                .as_deref()
                .context("adopt plan missing target worktree")?;
            let inventory = list_worktrees(&plan.cwd).await?;
            anyhow::ensure!(
                inventory
                    .iter()
                    .any(|worktree| same_path(&worktree.path, target)),
                "cannot adopt a path that is not a Git worktree: {target}"
            );
        }
    }
    Ok(())
}

async fn verify_success(store: &SqliteStore, plan: &OperationPlan) -> Result<(String, String)> {
    match plan.kind {
        OperationKind::CreateWorktree => {
            let target = plan
                .target_worktree
                .as_deref()
                .context("create plan missing target worktree")?;
            let branch = plan
                .target_branch
                .as_deref()
                .context("create plan missing target branch")?;
            let inventory = list_worktrees(&plan.cwd).await?;
            let worktree = inventory
                .iter()
                .find(|worktree| same_path(&worktree.path, target))
                .context("created worktree not present in git worktree list")?;
            anyhow::ensure!(
                worktree.branch.as_deref() == Some(branch),
                "created worktree branch mismatch"
            );
            let canonical = canonical_path(target);
            let now = now_unix_ms();
            store.upsert_managed_worktree(&ManagedWorktreeRecord {
                repo: plan.repo.clone(),
                canonical_path: canonical.clone(),
                branch: Some(branch.to_string()),
                created_by_operation_id: plan.operation_id.clone(),
                adopted: false,
                created_at_unix_ms: now,
                last_verified_at_unix_ms: now,
            })?;
            Ok((
                canonical,
                format!("git worktree list confirms branch {branch}"),
            ))
        }
        OperationKind::RemoveWorktree => {
            let target = plan
                .target_worktree
                .as_deref()
                .context("remove plan missing target worktree")?;
            let inventory = list_worktrees(&plan.cwd).await?;
            anyhow::ensure!(
                !inventory
                    .iter()
                    .any(|worktree| same_path(&worktree.path, target)),
                "removed worktree still appears in git worktree list"
            );
            store.remove_managed_worktree(&plan.repo.git_common_dir, target)?;
            Ok((
                target.to_string(),
                "git worktree list confirms worktree is absent; branch was not deleted".into(),
            ))
        }
        OperationKind::DeleteBranch => {
            let branch = plan
                .target_branch
                .as_deref()
                .context("delete-branch plan missing branch")?;
            anyhow::ensure!(
                !branch_exists(&plan.cwd, branch).await?,
                "branch still exists after delete"
            );
            Ok((
                branch.to_string(),
                "git show-ref confirms local branch is absent".into(),
            ))
        }
        OperationKind::AdoptWorktree => {
            let target = plan
                .target_worktree
                .as_deref()
                .context("adopt plan missing target worktree")?;
            let inventory = list_worktrees(&plan.cwd).await?;
            let worktree = inventory
                .iter()
                .find(|worktree| same_path(&worktree.path, target))
                .context("adopted worktree disappeared")?;
            let canonical = canonical_path(target);
            let now = now_unix_ms();
            store.upsert_managed_worktree(&ManagedWorktreeRecord {
                repo: plan.repo.clone(),
                canonical_path: canonical.clone(),
                branch: worktree.branch.clone(),
                created_by_operation_id: plan.operation_id.clone(),
                adopted: true,
                created_at_unix_ms: now,
                last_verified_at_unix_ms: now,
            })?;
            Ok((canonical, "existing worktree explicitly adopted".into()))
        }
    }
}

async fn reconcile_receipt(
    store: &SqliteStore,
    mut receipt: OperationReceipt,
) -> Result<OperationReceipt> {
    let now = now_unix_ms();
    match reconcile_outcome(store, &receipt.plan).await {
        Ok(ReconciledOutcome::Succeeded {
            result_ref,
            verification,
        }) => {
            receipt.succeed(now, result_ref, verification);
        }
        Ok(ReconciledOutcome::Failed(reason)) => {
            receipt.fail(now, reason);
        }
        Ok(ReconciledOutcome::Unknown(reason)) => {
            receipt.outcome_unknown(now, reason);
        }
        Err(error) => {
            receipt.outcome_unknown(now, format!("reconciliation failed: {error:#}"));
        }
    }
    store.save_operation_receipt(&receipt)?;
    Ok(receipt)
}

enum ReconciledOutcome {
    Succeeded {
        result_ref: String,
        verification: String,
    },
    Failed(String),
    Unknown(String),
}

async fn reconcile_outcome(store: &SqliteStore, plan: &OperationPlan) -> Result<ReconciledOutcome> {
    match plan.kind {
        OperationKind::CreateWorktree => {
            let target = plan
                .target_worktree
                .as_deref()
                .context("create plan missing target")?;
            let branch = plan
                .target_branch
                .as_deref()
                .context("create plan missing branch")?;
            let inventory = list_worktrees(&plan.cwd).await?;
            if let Some(worktree) = inventory
                .iter()
                .find(|worktree| same_path(&worktree.path, target))
            {
                if worktree.branch.as_deref() == Some(branch) {
                    return verify_success(store, plan)
                        .await
                        .map(|(result_ref, verification)| ReconciledOutcome::Succeeded {
                            result_ref,
                            verification: format!(
                                "reconciled after uncertain outcome: {verification}"
                            ),
                        });
                }
                return Ok(ReconciledOutcome::Unknown(format!(
                    "target path exists as a worktree on unexpected branch {:?}",
                    worktree.branch
                )));
            }
            if Path::new(target).exists() || branch_exists(&plan.cwd, branch).await? {
                return Ok(ReconciledOutcome::Unknown(
                    "partial create side effects exist but target worktree is not fully registered"
                        .into(),
                ));
            }
            Ok(ReconciledOutcome::Failed(
                "reconciliation confirms create side effects are absent".into(),
            ))
        }
        OperationKind::RemoveWorktree => {
            let target = plan
                .target_worktree
                .as_deref()
                .context("remove plan missing target")?;
            let inventory = list_worktrees(&plan.cwd).await?;
            if inventory
                .iter()
                .any(|worktree| same_path(&worktree.path, target))
            {
                return Ok(ReconciledOutcome::Failed(
                    "reconciliation confirms worktree still exists".into(),
                ));
            }
            verify_success(store, plan)
                .await
                .map(|(result_ref, verification)| ReconciledOutcome::Succeeded {
                    result_ref,
                    verification: format!("reconciled after uncertain outcome: {verification}"),
                })
        }
        OperationKind::DeleteBranch => {
            let branch = plan
                .target_branch
                .as_deref()
                .context("delete plan missing branch")?;
            if branch_exists(&plan.cwd, branch).await? {
                return Ok(ReconciledOutcome::Failed(
                    "reconciliation confirms branch still exists".into(),
                ));
            }
            verify_success(store, plan)
                .await
                .map(|(result_ref, verification)| ReconciledOutcome::Succeeded {
                    result_ref,
                    verification: format!("reconciled after uncertain outcome: {verification}"),
                })
        }
        OperationKind::AdoptWorktree => {
            verify_success(store, plan)
                .await
                .map(|(result_ref, verification)| ReconciledOutcome::Succeeded {
                    result_ref,
                    verification,
                })
        }
    }
}

async fn recover_incomplete(
    store: &SqliteStore,
    locks: &RepoLocks,
    tx: &mpsc::UnboundedSender<MutationEvent>,
) -> Result<()> {
    let receipts = store.load_recoverable_operation_receipts()?;
    for mut receipt in receipts {
        let lock = repo_lock(locks, &receipt.plan.repo).await;
        let _guard = lock.lock().await;

        if receipt.state == OperationState::Planned {
            receipt.fail(
                now_unix_ms(),
                "planned operation was never confirmed/executed before restart".into(),
            );
            store.save_operation_receipt(&receipt)?;
        } else {
            if receipt.state == OperationState::Executing {
                receipt.outcome_unknown(
                    now_unix_ms(),
                    "process restarted while operation was executing".into(),
                );
                store.save_operation_receipt(&receipt)?;
            }
            receipt = reconcile_receipt(store, receipt).await?;
        }
        let _ = tx.send(MutationEvent::Receipt(Box::new(receipt)));
    }
    Ok(())
}

#[derive(Debug)]
struct WorktreeEntry {
    path: String,
    branch: Option<String>,
}

async fn list_worktrees(cwd: &str) -> Result<Vec<WorktreeEntry>> {
    let output = run_git(cwd, ["worktree", "list", "--porcelain"]).await?;
    anyhow::ensure!(
        output.success,
        "git worktree list failed: {}",
        output.stderr.trim()
    );
    Ok(parse_worktree_porcelain(&output.stdout))
}

fn parse_worktree_porcelain(value: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut path: Option<String> = None;
    let mut branch: Option<String> = None;

    for line in value.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let Some(path) = path.take() {
                entries.push(WorktreeEntry {
                    path,
                    branch: branch.take(),
                });
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("worktree ") {
            path = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("branch refs/heads/") {
            branch = Some(value.to_string());
        } else if line == "detached" {
            branch = None;
        }
    }

    entries
}

async fn branch_exists(cwd: &str, branch: &str) -> Result<bool> {
    let output = run_git(
        cwd,
        [
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .await?;
    match output.code {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(anyhow!("git show-ref failed: {}", output.stderr.trim())),
    }
}

async fn worktree_is_clean(path: &str) -> Result<bool> {
    let output = run_git(path, ["status", "--porcelain", "--untracked-files=all"]).await?;
    anyhow::ensure!(
        output.success,
        "git status failed: {}",
        output.stderr.trim()
    );
    Ok(output.stdout.trim().is_empty())
}

#[derive(Debug)]
struct MutationOutput {
    success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

async fn run_git_mutation(cwd: &str, argv: &[String]) -> Result<MutationOutput> {
    run_command(cwd, argv, MUTATION_TIMEOUT).await
}

async fn run_git<I, S>(cwd: &str, args: I) -> Result<MutationOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let argv = args
        .into_iter()
        .map(|value| value.as_ref().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    run_command(cwd, &argv, VERIFY_TIMEOUT).await
}

async fn run_command(cwd: &str, argv: &[String], timeout: Duration) -> Result<MutationOutput> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(cwd)
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().context("spawn git mutation")?;
    let stdout = child.stdout.take().context("git stdout unavailable")?;
    let stderr = child.stderr.take().context("git stderr unavailable")?;

    let capture = async {
        let (stdout, stderr, status) = tokio::join!(
            read_capped(stdout, MAX_MUTATION_OUTPUT_BYTES),
            read_capped(stderr, MAX_MUTATION_OUTPUT_BYTES),
            child.wait(),
        );
        let stdout = stdout.context("read git stdout")?;
        let stderr = stderr.context("read git stderr")?;
        let status = status.context("wait for git")?;
        Result::<_, anyhow::Error>::Ok(MutationOutput {
            success: status.success(),
            code: status.code(),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    };

    tokio::time::timeout(timeout, capture)
        .await
        .with_context(|| format!("git mutation timed out after {}s", timeout.as_secs()))?
}

async fn read_capped<R>(mut reader: R, limit: usize) -> std::io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut stored = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(stored.len());
        let keep = remaining.min(read);
        stored.extend_from_slice(&buffer[..keep]);
    }
    Ok(stored)
}

fn canonical_path(value: &str) -> String {
    std::fs::canonicalize(value)
        .unwrap_or_else(|_| PathBuf::from(value))
        .to_string_lossy()
        .into_owned()
}

fn same_path(left: &str, right: &str) -> bool {
    canonical_path(left).eq_ignore_ascii_case(&canonical_path(right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::OperationPlan;
    use tempfile::tempdir;

    fn git(cwd: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .status()
            .expect("spawn git");
        assert!(status.success(), "git command failed: {args:?}");
    }

    fn init_repo(root: &Path) -> LocalRepoIdentity {
        std::fs::create_dir_all(root).expect("create root");
        git(root, &["init"]);
        git(root, &["config", "user.email", "ci@example.invalid"]);
        git(root, &["config", "user.name", "CI"]);
        std::fs::write(root.join("tracked.txt"), "base\n").expect("write");
        git(root, &["add", "tracked.txt"]);
        git(root, &["commit", "-m", "base"]);
        futures_lite_probe(root)
    }

    fn futures_lite_probe(root: &Path) -> LocalRepoIdentity {
        let common = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .output()
            .expect("git rev-parse");
        assert!(common.status.success());
        let common = String::from_utf8_lossy(&common.stdout).trim().to_string();
        LocalRepoIdentity {
            git_common_dir: canonical_path(&common),
            primary_root: canonical_path(root.to_string_lossy().as_ref()),
        }
    }

    #[test]
    fn porcelain_parser_preserves_worktree_branch() {
        let parsed = parse_worktree_porcelain(
            "worktree /repo
HEAD deadbeef
branch refs/heads/main

             worktree /repo-feature
HEAD cafe
branch refs/heads/feature

",
        );
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].path, "/repo-feature");
        assert_eq!(parsed[1].branch.as_deref(), Some("feature"));
    }

    #[tokio::test]
    async fn create_and_remove_managed_worktree_preserve_branch_separation() {
        let temp = tempdir().expect("tempdir");
        let repo_root = temp.path().join("repo");
        let repo = init_repo(&repo_root);
        let store = SqliteStore::at(temp.path().join("store"));
        let target = temp.path().join("feature-wt");
        let target_text = target.to_string_lossy().into_owned();

        let create = OperationPlan::create_worktree(
            repo.clone(),
            repo_root.to_string_lossy().into_owned(),
            target_text.clone(),
            "feature".into(),
            "HEAD".into(),
            1,
        );
        let receipt = execute_request(
            &store,
            MutationRequest {
                plan: create,
                active_scopes: vec![],
            },
        )
        .await
        .expect("create receipt");
        assert_eq!(receipt.state, OperationState::Succeeded);
        assert!(target.exists());
        assert!(
            branch_exists(repo_root.to_string_lossy().as_ref(), "feature")
                .await
                .expect("branch")
        );

        let remove = OperationPlan::remove_worktree(
            repo.clone(),
            repo_root.to_string_lossy().into_owned(),
            canonical_path(&target_text),
            2,
        );
        let receipt = execute_request(
            &store,
            MutationRequest {
                plan: remove,
                active_scopes: vec![],
            },
        )
        .await
        .expect("remove receipt");
        assert_eq!(receipt.state, OperationState::Succeeded);
        assert!(!target.exists());
        assert!(
            branch_exists(repo_root.to_string_lossy().as_ref(), "feature")
                .await
                .expect("branch preserved")
        );
    }

    #[tokio::test]
    async fn dirty_managed_worktree_is_refused_without_force() {
        let temp = tempdir().expect("tempdir");
        let repo_root = temp.path().join("repo");
        let repo = init_repo(&repo_root);
        let store = SqliteStore::at(temp.path().join("store"));
        let target = temp.path().join("dirty-wt");
        let target_text = target.to_string_lossy().into_owned();

        let create = OperationPlan::create_worktree(
            repo.clone(),
            repo_root.to_string_lossy().into_owned(),
            target_text.clone(),
            "dirty".into(),
            "HEAD".into(),
            1,
        );
        let create = execute_request(
            &store,
            MutationRequest {
                plan: create,
                active_scopes: vec![],
            },
        )
        .await
        .expect("create");
        assert_eq!(create.state, OperationState::Succeeded);
        std::fs::write(target.join("tracked.txt"), "dirty\n").expect("dirty");

        let remove = OperationPlan::remove_worktree(
            repo,
            repo_root.to_string_lossy().into_owned(),
            canonical_path(&target_text),
            2,
        );
        let remove = execute_request(
            &store,
            MutationRequest {
                plan: remove,
                active_scopes: vec![],
            },
        )
        .await
        .expect("remove");
        assert_eq!(remove.state, OperationState::Failed);
        assert!(
            remove
                .failure
                .as_deref()
                .is_some_and(|failure| failure.contains("dirty"))
        );
        assert!(target.exists());
    }

    #[tokio::test]
    async fn overlapping_active_scope_blocks_removal() {
        let temp = tempdir().expect("tempdir");
        let repo_root = temp.path().join("repo");
        let repo = init_repo(&repo_root);
        let store = SqliteStore::at(temp.path().join("store"));
        let target = temp.path().join("active-wt");
        let target_text = target.to_string_lossy().into_owned();

        let create = OperationPlan::create_worktree(
            repo.clone(),
            repo_root.to_string_lossy().into_owned(),
            target_text.clone(),
            "active".into(),
            "HEAD".into(),
            1,
        );
        let create = execute_request(
            &store,
            MutationRequest {
                plan: create,
                active_scopes: vec![],
            },
        )
        .await
        .expect("create");
        assert_eq!(create.state, OperationState::Succeeded);

        let remove = OperationPlan::remove_worktree(
            repo.clone(),
            repo_root.to_string_lossy().into_owned(),
            canonical_path(&target_text),
            2,
        );
        let remove = execute_request(
            &store,
            MutationRequest {
                plan: remove,
                active_scopes: vec![MutationScope {
                    repo: Some(repo),
                    writable_roots: vec![canonical_path(&target_text)],
                    confidence: crate::operation::MutationScopeConfidence::ExactWorktree,
                }],
            },
        )
        .await
        .expect("remove");
        assert_eq!(remove.state, OperationState::Failed);
        assert!(
            remove
                .failure
                .as_deref()
                .is_some_and(|failure| failure.contains("active mutation scope"))
        );
    }

    #[tokio::test]
    async fn adopt_records_existing_worktree_without_running_git_mutation() {
        let temp = tempdir().expect("tempdir");
        let repo_root = temp.path().join("repo");
        let repo = init_repo(&repo_root);
        let store = SqliteStore::at(temp.path().join("store"));
        let target = temp.path().join("existing-wt");
        let target_text = target.to_string_lossy().into_owned();
        git(
            &repo_root,
            &["worktree", "add", "-b", "existing", &target_text],
        );

        let plan = OperationPlan::adopt_worktree(
            repo.clone(),
            repo_root.to_string_lossy().into_owned(),
            canonical_path(&target_text),
            1,
        );
        let receipt = execute_request(
            &store,
            MutationRequest {
                plan,
                active_scopes: vec![],
            },
        )
        .await
        .expect("adopt");
        assert_eq!(receipt.state, OperationState::Succeeded);

        let managed = store
            .managed_worktree(&repo.git_common_dir, &canonical_path(&target_text))
            .expect("lookup")
            .expect("managed");
        assert!(managed.adopted);
        assert_eq!(managed.branch.as_deref(), Some("existing"));
    }

    #[tokio::test]
    async fn recovery_reconciles_executing_create_instead_of_retrying() {
        let temp = tempdir().expect("tempdir");
        let repo_root = temp.path().join("repo");
        let repo = init_repo(&repo_root);
        let store = SqliteStore::at(temp.path().join("store"));
        let target = temp.path().join("recovered-wt");
        let target_text = target.to_string_lossy().into_owned();

        git(
            &repo_root,
            &["worktree", "add", "-b", "recovered", &target_text, "HEAD"],
        );
        let plan = OperationPlan::create_worktree(
            repo,
            repo_root.to_string_lossy().into_owned(),
            canonical_path(&target_text),
            "recovered".into(),
            "HEAD".into(),
            1,
        );
        let mut receipt = OperationReceipt::planned(plan);
        receipt.start(2);
        store
            .save_operation_receipt(&receipt)
            .expect("save executing");

        let (tx, mut rx) = mpsc::unbounded_channel();
        recover_incomplete(&store, &Arc::new(Mutex::new(BTreeMap::new())), &tx)
            .await
            .expect("recover");
        let recovered = match rx.recv().await.expect("event") {
            MutationEvent::Receipt(receipt) => *receipt,
            other => panic!("unexpected event: {other:?}"),
        };
        assert_eq!(recovered.state, OperationState::Succeeded);
        assert!(
            recovered
                .verification
                .as_deref()
                .is_some_and(|value| value.contains("reconciled"))
        );
    }
}
