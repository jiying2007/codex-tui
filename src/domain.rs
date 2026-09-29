use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadId(pub String);

impl ThreadId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Display for ThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub type ThreadIdentity = ThreadId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexTarget {
    pub codex_home: Option<String>,
    pub backend_fingerprint: String,
    pub platform: String,
    #[serde(default)]
    pub capabilities: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceSource {
    CodexProject,
    GitRepository,
    WorkingDirectory,
    ExplicitRegistration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceIdentity {
    pub stable_key: String,
    pub source: WorkspaceSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LocalRepoIdentity {
    pub git_common_dir: String,
    pub primary_root: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeIdentity {
    pub repo: LocalRepoIdentity,
    pub canonical_path: String,
    pub branch: Option<String>,
    pub managed_by_codex_tui: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeStatus {
    Working,
    WaitingHuman,
    Ready,
    Inactive,
}

impl RuntimeStatus {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Working => "WORKING",
            Self::WaitingHuman => "WAITING",
            Self::Ready => "READY",
            Self::Inactive => "IDLE",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttentionReason {
    ApprovalRequired,
    UserInputRequired,
    ReadyForReview,
    MarkedUnread,
}

impl AttentionReason {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ApprovalRequired => "approval",
            Self::UserInputRequired => "input",
            Self::ReadyForReview => "review",
            Self::MarkedUnread => "unread",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub id: ThreadId,
    pub workspace: String,
    pub title: String,
    pub runtime: RuntimeStatus,
    pub attention: Vec<AttentionReason>,
    pub pinned: bool,
    pub alias: Option<String>,
}

impl ThreadSummary {
    pub fn display_title(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.title)
    }

    pub fn needs_attention(&self) -> bool {
        !self.attention.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadUiState {
    pub draft: String,
    pub scroll: u16,
    pub follow: bool,
}

impl Default for ThreadUiState {
    fn default() -> Self {
        Self {
            draft: String::new(),
            scroll: 0,
            follow: true,
        }
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn repository_identity_is_distinct_from_worktree_identity() {
        let repo = LocalRepoIdentity {
            git_common_dir: "/repo/.git".into(),
            primary_root: "/repo".into(),
        };
        let left = WorktreeIdentity {
            repo: repo.clone(),
            canonical_path: "/repo".into(),
            branch: Some("main".into()),
            managed_by_codex_tui: false,
        };
        let right = WorktreeIdentity {
            repo,
            canonical_path: "/tmp/worktrees/feature".into(),
            branch: Some("feature".into()),
            managed_by_codex_tui: true,
        };

        assert_eq!(left.repo, right.repo);
        assert_ne!(left.canonical_path, right.canonical_path);
    }
}
