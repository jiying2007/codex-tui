use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CwdLocality {
    LocalDirectory,
    NativeMissing,
    ForeignWindows,
    ForeignUnix,
    Relative,
    Empty,
}

impl CwdLocality {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalDirectory => "local",
            Self::NativeMissing => "stale",
            Self::ForeignWindows => "foreign-windows",
            Self::ForeignUnix => "foreign-unix",
            Self::Relative => "relative",
            Self::Empty => "empty",
        }
    }

    pub const fn terminal_usable(self) -> bool {
        matches!(self, Self::LocalDirectory)
    }

    pub const fn is_foreign(self) -> bool {
        matches!(self, Self::ForeignWindows | Self::ForeignUnix)
    }
}

pub fn classify_cwd_without_io(cwd: &str) -> Option<CwdLocality> {
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return Some(CwdLocality::Empty);
    }

    let direct_windows = windows_absolute_path(cwd);
    let embedded_windows = embedded_windows_absolute(cwd);

    if cfg!(windows) {
        if direct_windows {
            return None;
        }
        if embedded_windows.is_some() {
            return Some(CwdLocality::ForeignWindows);
        }
        if cwd.starts_with('/') {
            return Some(CwdLocality::ForeignUnix);
        }
    } else {
        if embedded_windows.is_some() {
            return Some(CwdLocality::ForeignWindows);
        }
        if cwd.starts_with('/') {
            return None;
        }
    }

    Some(CwdLocality::Relative)
}

pub fn classify_cwd(cwd: &str) -> CwdLocality {
    let cwd = cwd.trim();
    if let Some(locality) = classify_cwd_without_io(cwd) {
        return locality;
    }

    if Path::new(cwd).is_dir() {
        CwdLocality::LocalDirectory
    } else {
        CwdLocality::NativeMissing
    }
}

pub fn display_cwd(cwd: &str) -> &str {
    let cwd = cwd.trim();
    if !cfg!(windows)
        && let Some(foreign) = embedded_windows_absolute(cwd)
    {
        return foreign;
    }
    cwd
}

fn embedded_windows_absolute(path: &str) -> Option<&str> {
    if windows_absolute_path(path) {
        return Some(path);
    }
    for (index, character) in path.char_indices() {
        if matches!(character, '/' | '\\') {
            let candidate = &path[index + character.len_utf8()..];
            if windows_absolute_path(candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn windows_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
        || path.starts_with("\\\\")
}

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
    SystemError,
    Inactive,
}

impl RuntimeStatus {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Working => "WORKING",
            Self::WaitingHuman => "WAITING",
            Self::Ready => "READY",
            Self::SystemError => "ERROR",
            Self::Inactive => "IDLE",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttentionReason {
    ApprovalRequired,
    UserInputRequired,
    ReadyForReview,
    SystemError,
    MarkedUnread,
}

impl AttentionReason {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ApprovalRequired => "approval",
            Self::UserInputRequired => "input",
            Self::ReadyForReview => "review",
            Self::SystemError => "error",
            Self::MarkedUnread => "unread",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadMetadata {
    pub cwd: String,
    pub model: Option<String>,
    pub project_id: Option<String>,
    pub source: String,
    pub updated_at: i64,
    pub loaded: Option<bool>,
    pub workspace_key: String,
    pub workspace_basis: String,
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
    #[serde(default)]
    pub metadata: ThreadMetadata,
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
    fn cwd_locality_detects_host_local_and_foreign_paths() {
        let current = std::env::current_dir().expect("cwd");
        assert_eq!(
            classify_cwd(&current.to_string_lossy()),
            CwdLocality::LocalDirectory
        );

        if cfg!(windows) {
            assert_eq!(classify_cwd("/home/user/repo"), CwdLocality::ForeignUnix);
        } else {
            assert_eq!(
                classify_cwd(r"C:\\Users\\jun\\repo"),
                CwdLocality::ForeignWindows
            );
            assert_eq!(
                classify_cwd(r"/vsdata/repo/C:\\Users\\jun\\repo"),
                CwdLocality::ForeignWindows
            );
            assert_eq!(
                display_cwd(r"/vsdata/repo/C:\\Users\\jun\\repo"),
                r"C:\\Users\\jun\\repo"
            );
        }

        assert_eq!(classify_cwd("relative/repo"), CwdLocality::Relative);
        assert_eq!(classify_cwd(""), CwdLocality::Empty);
    }

    #[test]
    fn cwd_locality_without_io_only_resolves_structural_cases() {
        assert_eq!(
            classify_cwd_without_io("relative/repo"),
            Some(CwdLocality::Relative)
        );
        assert_eq!(classify_cwd_without_io(""), Some(CwdLocality::Empty));

        if cfg!(windows) {
            assert_eq!(classify_cwd_without_io("/home/user/repo"), Some(CwdLocality::ForeignUnix));
            assert_eq!(classify_cwd_without_io(r"C:\repo"), None);
        } else {
            assert_eq!(
                classify_cwd_without_io(r"C:\repo"),
                Some(CwdLocality::ForeignWindows)
            );
            assert_eq!(classify_cwd_without_io("/tmp/repo"), None);
        }
    }

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
