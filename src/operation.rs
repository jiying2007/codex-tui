use crate::domain::{LocalRepoIdentity, ThreadSummary};
use crate::git::GitContext;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static OPERATION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MutationScopeConfidence {
    ExactWorktree,
    ConservativeCwd,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationScope {
    pub repo: Option<LocalRepoIdentity>,
    pub writable_roots: Vec<String>,
    pub confidence: MutationScopeConfidence,
}

pub fn mutation_scope_for_thread(
    thread: &ThreadSummary,
    git: Option<&GitContext>,
) -> MutationScope {
    if let Some(worktree) = git.and_then(|context| context.worktree.as_ref()) {
        return MutationScope {
            repo: Some(worktree.repo.clone()),
            writable_roots: vec![worktree.canonical_path.clone()],
            confidence: MutationScopeConfidence::ExactWorktree,
        };
    }

    MutationScope {
        repo: git.and_then(|context| context.repo.clone()),
        writable_roots: if thread.metadata.cwd.trim().is_empty() {
            vec![]
        } else {
            vec![thread.metadata.cwd.clone()]
        },
        confidence: MutationScopeConfidence::ConservativeCwd,
    }
}

pub fn scopes_overlap(left: &MutationScope, right: &MutationScope) -> bool {
    if let (Some(left_repo), Some(right_repo)) = (&left.repo, &right.repo)
        && left_repo != right_repo
    {
        return false;
    }

    left.writable_roots.iter().any(|left_root| {
        right
            .writable_roots
            .iter()
            .any(|right_root| paths_overlap(left_root, right_root))
    })
}

fn paths_overlap(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        value
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_ascii_lowercase()
    };
    let left = normalize(left);
    let right = normalize(right);
    left == right
        || left
            .strip_prefix(&right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(&left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationKind {
    CreateWorktree,
    RemoveWorktree,
    DeleteBranch,
    AdoptWorktree,
}

impl OperationKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::CreateWorktree => "create-worktree",
            Self::RemoveWorktree => "remove-worktree",
            Self::DeleteBranch => "delete-branch",
            Self::AdoptWorktree => "adopt-worktree",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationState {
    Planned,
    Executing,
    Succeeded,
    Failed,
    OutcomeUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationPrecondition {
    pub key: String,
    pub expected: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationPlan {
    pub operation_id: String,
    pub kind: OperationKind,
    pub repo: LocalRepoIdentity,
    pub cwd: String,
    pub argv: Vec<String>,
    pub target_worktree: Option<String>,
    pub target_branch: Option<String>,
    pub expected_side_effect: String,
    pub preconditions: Vec<OperationPrecondition>,
    pub planned_at_unix_ms: u64,
}

impl OperationPlan {
    pub fn create_worktree(
        repo: LocalRepoIdentity,
        cwd: String,
        worktree_path: String,
        branch: String,
        start_point: String,
        planned_at_unix_ms: u64,
    ) -> Self {
        Self {
            operation_id: new_operation_id(planned_at_unix_ms),
            kind: OperationKind::CreateWorktree,
            repo,
            cwd,
            argv: vec![
                "worktree".into(),
                "add".into(),
                "-b".into(),
                branch.clone(),
                worktree_path.clone(),
                start_point.clone(),
            ],
            target_worktree: Some(worktree_path),
            target_branch: Some(branch),
            expected_side_effect: format!(
                "new managed worktree and branch created from {start_point}"
            ),
            preconditions: vec![
                OperationPrecondition {
                    key: "target-path-absent".into(),
                    expected: "true".into(),
                },
                OperationPrecondition {
                    key: "branch-absent".into(),
                    expected: "true".into(),
                },
            ],
            planned_at_unix_ms,
        }
    }

    pub fn remove_worktree(
        repo: LocalRepoIdentity,
        cwd: String,
        worktree_path: String,
        planned_at_unix_ms: u64,
    ) -> Self {
        Self {
            operation_id: new_operation_id(planned_at_unix_ms),
            kind: OperationKind::RemoveWorktree,
            repo,
            cwd,
            argv: vec!["worktree".into(), "remove".into(), worktree_path.clone()],
            target_worktree: Some(worktree_path),
            target_branch: None,
            expected_side_effect: "managed worktree removed; branch preserved".into(),
            preconditions: vec![
                OperationPrecondition {
                    key: "managed-by-codex-tui".into(),
                    expected: "true".into(),
                },
                OperationPrecondition {
                    key: "worktree-clean".into(),
                    expected: "true".into(),
                },
                OperationPrecondition {
                    key: "no-active-mutation-scope-overlap".into(),
                    expected: "true".into(),
                },
            ],
            planned_at_unix_ms,
        }
    }

    pub fn delete_branch(
        repo: LocalRepoIdentity,
        cwd: String,
        branch: String,
        planned_at_unix_ms: u64,
    ) -> Self {
        Self {
            operation_id: new_operation_id(planned_at_unix_ms),
            kind: OperationKind::DeleteBranch,
            repo,
            cwd,
            argv: vec!["branch".into(), "-d".into(), branch.clone()],
            target_worktree: None,
            target_branch: Some(branch),
            expected_side_effect: "local branch deleted; no worktree removal implied".into(),
            preconditions: vec![OperationPrecondition {
                key: "branch-not-checked-out".into(),
                expected: "true".into(),
            }],
            planned_at_unix_ms,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationReceipt {
    pub operation_id: String,
    pub plan: OperationPlan,
    pub state: OperationState,
    pub started_at_unix_ms: Option<u64>,
    pub completed_at_unix_ms: Option<u64>,
    pub result_ref: Option<String>,
    pub verification: Option<String>,
    pub failure: Option<String>,
}

impl OperationReceipt {
    pub fn planned(plan: OperationPlan) -> Self {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedWorktreeRecord {
    pub repo: LocalRepoIdentity,
    pub canonical_path: String,
    pub branch: Option<String>,
    pub created_by_operation_id: String,
    pub adopted: bool,
    pub created_at_unix_ms: u64,
    pub last_verified_at_unix_ms: u64,
}

pub fn new_operation_id(now_unix_ms: u64) -> String {
    let sequence = OPERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("op-{now_unix_ms}-{}-{sequence}", std::process::id())
}

pub fn now_unix_ms() -> u64 {
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
    use crate::backend::{CodexBackend, FakeBackend};

    #[test]
    fn conservative_scope_falls_back_to_thread_cwd() {
        let thread = FakeBackend::seeded().snapshot().threads.remove(0);
        let mut thread = thread;
        thread.metadata.cwd = "/repo".into();
        let scope = mutation_scope_for_thread(&thread, None);
        assert_eq!(scope.confidence, MutationScopeConfidence::ConservativeCwd);
        assert_eq!(scope.writable_roots, vec!["/repo"]);
    }

    #[test]
    fn scope_overlap_is_conservative_for_nested_roots() {
        let left = MutationScope {
            repo: None,
            writable_roots: vec!["/repo".into()],
            confidence: MutationScopeConfidence::ConservativeCwd,
        };
        let right = MutationScope {
            repo: None,
            writable_roots: vec!["/repo/sub".into()],
            confidence: MutationScopeConfidence::ConservativeCwd,
        };
        assert!(scopes_overlap(&left, &right));
    }

    #[test]
    fn removal_plan_never_deletes_branch_implicitly() {
        let repo = LocalRepoIdentity {
            git_common_dir: "/repo/.git".into(),
            primary_root: "/repo".into(),
        };
        let plan = OperationPlan::remove_worktree(repo, "/repo".into(), "/repo-wt".into(), 1);
        assert_eq!(plan.argv, vec!["worktree", "remove", "/repo-wt"]);
        assert!(plan.target_branch.is_none());
    }

    #[test]
    fn receipt_distinguishes_unknown_outcome_from_failure() {
        let repo = LocalRepoIdentity {
            git_common_dir: "/repo/.git".into(),
            primary_root: "/repo".into(),
        };
        let plan = OperationPlan::delete_branch(repo, "/repo".into(), "feature".into(), 1);
        let mut receipt = OperationReceipt::planned(plan);
        receipt.start(2);
        receipt.outcome_unknown(
            3,
            "process timed out after mutation may have applied".into(),
        );
        assert_eq!(receipt.state, OperationState::OutcomeUnknown);
        assert!(
            receipt
                .failure
                .as_deref()
                .is_some_and(|value| value.contains("timed out"))
        );
    }
}
