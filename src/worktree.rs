use crate::domain::LocalRepoIdentity;
use crate::operation::{
    ManagedWorktreeRecord, MutationScope, OperationKind, OperationPlan, OperationReceipt,
    OperationState, now_unix_ms, scopes_overlap,
};
use crate::sqlite_store::SqliteStore;
use crate::worktree_git::{
    branch_exists, canonical_path, list_worktrees, run_git_mutation, same_path, worktree_is_clean,
};
use anyhow::{Context, Result, anyhow};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};
use tokio::task::{JoinHandle, JoinSet};

const MUTATION_COMMAND_QUEUE_CAPACITY: usize = 32;
const MUTATION_EVENT_QUEUE_CAPACITY: usize = 64;
const MUTATION_MAX_CONCURRENCY: usize = 4;
const SCOPE_ADMISSION_MAX_AGE: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct MutationRequest {
    pub plan: OperationPlan,
    pub active_scopes: Vec<MutationScope>,
    /// Age of the UI-projected activity proof, not a lease from upstream Codex.
    pub admitted_at: Instant,
}

#[derive(Clone, Debug)]
pub enum MutationCommand {
    Execute(Box<MutationRequest>),
    Recover,
    RefreshInventory,
}

#[derive(Clone, Debug)]
pub enum MutationEvent {
    Receipt(Box<OperationReceipt>),
    ManagedWorktrees(Vec<ManagedWorktreeRecord>),
    Notice(String),
}

pub struct WorktreeMutationHandle {
    command_tx: mpsc::Sender<MutationCommand>,
    event_rx: mpsc::Receiver<MutationEvent>,
    task: JoinHandle<()>,
}

impl WorktreeMutationHandle {
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

    pub fn execute(&self, request: MutationRequest) -> Result<()> {
        queue_mutation_command(
            &self.command_tx,
            MutationCommand::Execute(Box::new(request)),
        )
    }

    pub fn recover(&self) -> Result<()> {
        queue_mutation_command(&self.command_tx, MutationCommand::Recover)
    }

    pub fn refresh_inventory(&self) -> Result<()> {
        queue_mutation_command(&self.command_tx, MutationCommand::RefreshInventory)
    }

    pub fn try_recv(&mut self) -> Option<MutationEvent> {
        self.event_rx.try_recv().ok()
    }
}

fn queue_mutation_command(
    tx: &mpsc::Sender<MutationCommand>,
    command: MutationCommand,
) -> Result<()> {
    tx.try_send(command).map_err(|error| match error {
        mpsc::error::TrySendError::Full(_) => {
            anyhow!("worktree mutation coordinator queue is full")
        }
        mpsc::error::TrySendError::Closed(_) => {
            anyhow!("worktree mutation coordinator is unavailable")
        }
    })
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
    mut command_rx: mpsc::Receiver<MutationCommand>,
    event_tx: mpsc::Sender<MutationEvent>,
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
                                MutationCommand::Execute(request) => {
                                    match execute_with_repo_lock(&store, &locks, *request).await {
                                        Ok(receipt) => {
                                            let _ = event_tx
                                                .send(MutationEvent::Receipt(Box::new(receipt)))
                                                .await;
                                            emit_inventory(&store, &event_tx).await;
                                        }
                                        Err(error) => {
                                            let _ = event_tx
                                                .send(MutationEvent::Notice(format!(
                                                    "worktree mutation coordinator failed: {error:#}"
                                                )))
                                                .await;
                                        }
                                    }
                                }
                                MutationCommand::Recover => {
                                    if let Err(error) =
                                        recover_incomplete(&store, &locks, &event_tx).await
                                    {
                                        let _ = event_tx
                                            .send(MutationEvent::Notice(format!(
                                                "worktree recovery failed: {error:#}"
                                            )))
                                            .await;
                                    }
                                    emit_inventory(&store, &event_tx).await;
                                }
                                MutationCommand::RefreshInventory => {
                                    emit_inventory(&store, &event_tx).await;
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

async fn emit_inventory(store: &SqliteStore, tx: &mpsc::Sender<MutationEvent>) {
    match store.load_managed_worktrees() {
        Ok(records) => {
            let _ = tx.send(MutationEvent::ManagedWorktrees(records)).await;
        }
        Err(error) => {
            let _ = tx
                .send(MutationEvent::Notice(format!(
                    "managed worktree inventory unavailable: {error:#}"
                )))
                .await;
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

    if let Err(error) = check_preconditions(
        store,
        &receipt.plan,
        &request.active_scopes,
        request.admitted_at,
    )
    .await
    {
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
        Ok(output) => record_nonzero_mutation(&mut receipt, &output),
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

// A nonzero exit can occur after Git already created, moved or removed state.
// Durable reconciliation must decide the actual result; raw hook stderr is never stored.
fn record_nonzero_mutation(
    receipt: &mut OperationReceipt,
    output: &crate::worktree_git::MutationOutput,
) {
    receipt.outcome_unknown(
        now_unix_ms(),
        format!(
            "git mutation returned status {}: {}",
            output
                .code
                .map_or_else(|| "unknown".into(), |code| code.to_string()),
            crate::hardening::safe_external_stderr(&output.stderr, "git mutation")
        ),
    );
}

async fn check_preconditions(
    store: &SqliteStore,
    plan: &OperationPlan,
    active_scopes: &[MutationScope],
    admitted_at: Instant,
) -> Result<()> {
    if matches!(
        plan.kind,
        OperationKind::RemoveWorktree | OperationKind::DeleteBranch
    ) {
        anyhow::ensure!(
            admitted_at.elapsed() <= SCOPE_ADMISSION_MAX_AGE,
            "active operation scope snapshot expired; review and confirm again"
        );
    }
    match plan.kind {
        OperationKind::CreateWorktree => {
            let target = plan
                .target_worktree
                .as_deref()
                .context("create plan missing target worktree")?;
            anyhow::ensure!(
                Path::new(target).is_absolute(),
                "target worktree path must be absolute: {target}"
            );
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
            let inventory = list_worktrees(&plan.cwd).await?;
            anyhow::ensure!(
                inventory
                    .iter()
                    .any(|worktree| same_path(&worktree.path, target)),
                "managed worktree is no longer registered with Git"
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
            anyhow::ensure!(
                Path::new(target).is_absolute(),
                "adopted worktree path must be absolute: {target}"
            );
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
            anyhow::ensure!(
                worktree_path_absent(target)?,
                "removed worktree filesystem entry still exists"
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

// Check the directory entry, including dangling symlinks, rather than just
// Path::exists(), which returns false for broken symlinks.
fn worktree_path_absent(target: &str) -> Result<bool> {
    match std::fs::symlink_metadata(target) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error).with_context(|| format!("stat worktree target {target}")),
    }
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
                return Ok(ReconciledOutcome::Unknown(
                    "Git still registers the worktree; partial remove effects are not excluded"
                        .into(),
                ));
            }
            if !worktree_path_absent(target)? {
                return Ok(ReconciledOutcome::Unknown(
                    "Git unregistered the worktree but its filesystem entry remains".into(),
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
    tx: &mpsc::Sender<MutationEvent>,
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
        let _ = tx.send(MutationEvent::Receipt(Box::new(receipt))).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::OperationPlan;
    use crate::worktree_git::parse_worktree_porcelain;
    use tempfile::tempdir;

    #[test]
    fn nonzero_git_exit_is_reconciled_without_persisting_hook_secrets() {
        let repo = LocalRepoIdentity {
            git_common_dir: "/repo/.git".into(),
            primary_root: "/repo".into(),
        };
        let plan = OperationPlan::delete_branch(repo, "/repo".into(), "feature".into(), 1);
        let mut receipt = OperationReceipt::planned(plan);
        record_nonzero_mutation(
            &mut receipt,
            &crate::worktree_git::MutationOutput {
                success: false,
                code: Some(1),
                stdout: String::new(),
                stderr: "Bearer sk-private https://alice:secret@internal.invalid/".into(),
            },
        );
        assert_eq!(receipt.state, OperationState::OutcomeUnknown);
        let failure = receipt.failure.expect("classified failure");
        for secret in ["sk-private", "alice", "secret", "internal.invalid"] {
            assert!(!failure.contains(secret));
        }
    }

    #[tokio::test]
    async fn queued_destructive_mutation_requires_fresh_admission_proof() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::at(temp.path());
        let repo = LocalRepoIdentity {
            git_common_dir: "/repo/.git".into(),
            primary_root: "/repo".into(),
        };
        let plan =
            OperationPlan::remove_worktree(repo, "/repo".into(), "/repo/stale-target".into(), 1);
        let error = check_preconditions(
            &store,
            &plan,
            &[],
            Instant::now() - SCOPE_ADMISSION_MAX_AGE - Duration::from_secs(1),
        )
        .await
        .expect_err("expired scope proof must fail before git access");
        assert!(error.to_string().contains("snapshot expired"));
    }

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

    #[tokio::test]
    async fn same_repo_reuses_one_mutation_lock_while_other_repo_does_not() {
        let locks: RepoLocks = Arc::new(Mutex::new(BTreeMap::new()));
        let repo_a = LocalRepoIdentity {
            git_common_dir: "/repo-a/.git".into(),
            primary_root: "/repo-a".into(),
        };
        let repo_a_alias = repo_a.clone();
        let repo_b = LocalRepoIdentity {
            git_common_dir: "/repo-b/.git".into(),
            primary_root: "/repo-b".into(),
        };

        let first = repo_lock(&locks, &repo_a).await;
        let same = repo_lock(&locks, &repo_a_alias).await;
        let other = repo_lock(&locks, &repo_b).await;
        assert!(Arc::ptr_eq(&first, &same));
        assert!(!Arc::ptr_eq(&first, &other));
    }

    #[test]
    fn mutation_coordinator_channels_are_bounded() {
        let source = include_str!("worktree.rs");
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
        queue_mutation_command(&tx, MutationCommand::RefreshInventory).expect("first command");
        let error = queue_mutation_command(&tx, MutationCommand::RefreshInventory)
            .expect_err("second command must hit bounded queue");
        assert!(error.to_string().contains("queue is full"));
    }

    #[test]
    fn porcelain_parser_tolerates_crlf_and_incidental_whitespace() {
        let parsed = parse_worktree_porcelain(
            "worktree C:/repo\r\nHEAD deadbeef\r\nbranch refs/heads/main\r\n\r\n  worktree C:/repo-feature  \r\nHEAD cafe\r\n  branch refs/heads/feature\r\n\r\n",
        );
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].path, "C:/repo-feature");
        assert_eq!(parsed[1].branch.as_deref(), Some("feature"));
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
                admitted_at: Instant::now(),
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
                admitted_at: Instant::now(),
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
    async fn uncertain_remove_requires_both_git_and_filesystem_absence() {
        let temp = tempdir().expect("tempdir");
        let repo_root = temp.path().join("repo");
        let repo = init_repo(&repo_root);
        let store = SqliteStore::at(temp.path().join("store"));
        let target = temp.path().join("uncertain-wt");
        let target_text = target.to_string_lossy().into_owned();
        git(&repo_root, &["worktree", "add", "-b", "uncertain", &target_text]);
        let remove = OperationPlan::remove_worktree(
            repo,
            repo_root.to_string_lossy().into_owned(),
            canonical_path(&target_text),
            1,
        );

        // Even when Git still registers the target, an attempted removal
        // could have partially deleted files: do not claim side-effect-free failure.
        let registered = reconcile_outcome(&store, &remove).await.expect("registered");
        assert!(matches!(registered, ReconciledOutcome::Unknown(_)));

        git(&repo_root, &["worktree", "remove", &target_text]);
        // Simulate a failed cleanup after the metadata record was removed.
        std::fs::create_dir_all(&target).expect("restore orphan directory");
        std::fs::write(target.join("orphan.txt"), "not deleted").expect("orphan file");
        let leftover = reconcile_outcome(&store, &remove).await.expect("leftover");
        assert!(matches!(leftover, ReconciledOutcome::Unknown(_)));
        let error = verify_success(&store, &remove)
            .await
            .expect_err("existing target cannot be a verified removal");
        assert!(error.to_string().contains("filesystem entry still exists"));
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_is_not_treated_as_absent_worktree() {
        let temp = tempdir().expect("tempdir");
        let link = temp.path().join("orphan-link");
        std::os::unix::fs::symlink(temp.path().join("missing"), &link)
            .expect("dangling test link");
        assert!(!worktree_path_absent(link.to_str().expect("utf8 path"))
            .expect("stat link"));
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
                admitted_at: Instant::now(),
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
                admitted_at: Instant::now(),
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
                admitted_at: Instant::now(),
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
                admitted_at: Instant::now(),
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
    async fn stale_managed_record_does_not_authorize_unregistered_worktree_removal() {
        let temp = tempdir().expect("tempdir");
        let repo_root = temp.path().join("repo");
        let repo = init_repo(&repo_root);
        let store = SqliteStore::at(temp.path().join("store"));
        let target = temp.path().join("stale-wt");
        let target_text = target.to_string_lossy().into_owned();

        let create = OperationPlan::create_worktree(
            repo.clone(),
            repo_root.to_string_lossy().into_owned(),
            target_text.clone(),
            "stale-wt-branch".into(),
            "HEAD".into(),
            1,
        );
        let created = execute_request(
            &store,
            MutationRequest {
                admitted_at: Instant::now(),
                plan: create,
                active_scopes: vec![],
            },
        )
        .await
        .expect("create");
        assert_eq!(created.state, OperationState::Succeeded);
        // macOS may canonicalize /var to /private/var. Retain the exact
        // persisted path before deleting its target on disk.
        let managed_path = created.result_ref.expect("canonical managed path");
        git(&repo_root, &["worktree", "remove", &target_text]);

        let remove = OperationPlan::remove_worktree(
            repo,
            repo_root.to_string_lossy().into_owned(),
            managed_path,
            2,
        );
        let rejected = execute_request(
            &store,
            MutationRequest {
                admitted_at: Instant::now(),
                plan: remove,
                active_scopes: vec![],
            },
        )
        .await
        .expect("conservative reject");
        assert_eq!(rejected.state, OperationState::Failed);
        assert!(
            rejected
                .failure
                .as_deref()
                .is_some_and(|message| { message.contains("no longer registered") })
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
                admitted_at: Instant::now(),
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

        let (tx, mut rx) = mpsc::channel(MUTATION_EVENT_QUEUE_CAPACITY);
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
