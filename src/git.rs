use crate::domain::{LocalRepoIdentity, ThreadId, WorktreeIdentity};
use anyhow::{Context, Result, anyhow};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const GIT_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitFileChange {
    pub path: String,
    pub original_path: Option<String>,
    pub index_status: Option<char>,
    pub worktree_status: Option<char>,
    pub untracked: bool,
    pub conflict: bool,
}

impl GitFileChange {
    pub fn status_label(&self) -> String {
        if self.untracked {
            return "??".into();
        }
        if self.conflict {
            return "UU".into();
        }
        format!(
            "{}{}",
            self.index_status.unwrap_or('.'),
            self.worktree_status.unwrap_or('.')
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitContext {
    pub thread_id: ThreadId,
    pub cwd: String,
    pub is_repository: bool,
    pub repo: Option<LocalRepoIdentity>,
    pub worktree: Option<WorktreeIdentity>,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub changes: Vec<GitFileChange>,
    pub observed_at_unix_ms: u64,
    pub error: Option<String>,
}

impl GitContext {
    pub fn pending(thread_id: ThreadId, cwd: impl Into<String>) -> Self {
        Self {
            thread_id,
            cwd: cwd.into(),
            is_repository: false,
            repo: None,
            worktree: None,
            head: None,
            branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            dirty: false,
            changes: vec![],
            observed_at_unix_ms: 0,
            error: None,
        }
    }

    fn not_repository(thread_id: ThreadId, cwd: String) -> Self {
        let mut context = Self::pending(thread_id, cwd);
        context.observed_at_unix_ms = now_unix_ms();
        context
    }

    fn failed(thread_id: ThreadId, cwd: String, error: String) -> Self {
        let mut context = Self::pending(thread_id, cwd);
        context.observed_at_unix_ms = now_unix_ms();
        context.error = Some(error);
        context
    }
}

#[derive(Clone, Debug)]
pub enum GitCommand {
    Probe { thread_id: ThreadId, cwd: String },
}

#[derive(Clone, Debug)]
pub enum GitEvent {
    Context(GitContext),
}

pub struct GitHandle {
    command_tx: mpsc::UnboundedSender<GitCommand>,
    event_rx: mpsc::UnboundedReceiver<GitEvent>,
    task: JoinHandle<()>,
}

impl GitHandle {
    pub fn start() -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(run_actor(command_rx, event_tx));
        Self {
            command_tx,
            event_rx,
            task,
        }
    }

    pub fn probe(&self, thread_id: ThreadId, cwd: String) -> Result<()> {
        self.command_tx
            .send(GitCommand::Probe { thread_id, cwd })
            .map_err(|_| anyhow!("Git actor is not available"))
    }

    pub fn try_recv(&mut self) -> Option<GitEvent> {
        self.event_rx.try_recv().ok()
    }
}

impl Drop for GitHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run_actor(
    mut command_rx: mpsc::UnboundedReceiver<GitCommand>,
    event_tx: mpsc::UnboundedSender<GitEvent>,
) {
    while let Some(command) = command_rx.recv().await {
        match command {
            GitCommand::Probe { thread_id, cwd } => {
                let context = match probe_context(thread_id.clone(), cwd.clone()).await {
                    Ok(context) => context,
                    Err(error) => GitContext::failed(thread_id, cwd, error.to_string()),
                };
                let _ = event_tx.send(GitEvent::Context(context));
            }
        }
    }
}

pub async fn probe_context(thread_id: ThreadId, cwd: String) -> Result<GitContext> {
    let identity = run_git(
        &cwd,
        [
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-common-dir",
        ],
    )
    .await?;

    if !identity.success {
        if looks_like_not_repository(&identity.stderr) {
            return Ok(GitContext::not_repository(thread_id, cwd));
        }
        return Err(anyhow!(
            "git repository probe failed: {}",
            identity.stderr.trim()
        ));
    }

    let mut lines = identity
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let worktree_root = lines
        .next()
        .context("git rev-parse response missing worktree root")?
        .to_string();
    let common_dir = lines
        .next()
        .context("git rev-parse response missing common directory")?
        .to_string();
    let primary_root = primary_root_from_common_dir(&common_dir, &worktree_root);

    let status = run_git_bytes(
        &cwd,
        [
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ],
    )
    .await?;
    if !status.success {
        return Err(anyhow!(
            "git status failed: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        ));
    }
    let parsed = parse_porcelain_v2(&status.stdout)?;

    let repo = LocalRepoIdentity {
        git_common_dir: common_dir,
        primary_root,
    };
    let worktree = WorktreeIdentity {
        repo: repo.clone(),
        canonical_path: worktree_root,
        branch: parsed.branch.clone(),
        managed_by_codex_tui: false,
    };

    Ok(GitContext {
        thread_id,
        cwd,
        is_repository: true,
        repo: Some(repo),
        worktree: Some(worktree),
        head: parsed.head,
        branch: parsed.branch,
        upstream: parsed.upstream,
        ahead: parsed.ahead,
        behind: parsed.behind,
        dirty: !parsed.changes.is_empty(),
        changes: parsed.changes,
        observed_at_unix_ms: now_unix_ms(),
        error: None,
    })
}

#[derive(Debug)]
struct GitOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug)]
struct GitBytesOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

async fn run_git<I, S>(cwd: &str, args: I) -> Result<GitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_git_raw(cwd, args).await?;
    Ok(GitOutput {
        success: output.success,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

async fn run_git_bytes<I, S>(cwd: &str, args: I) -> Result<GitBytesOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_git_raw(cwd, args).await?;
    Ok(GitBytesOutput {
        success: output.success,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

struct RawGitOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

async fn run_git_raw<I, S>(cwd: &str, args: I) -> Result<RawGitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let output = tokio::time::timeout(GIT_PROBE_TIMEOUT, command.output())
        .await
        .with_context(|| {
            format!(
                "git command timed out after {}s",
                GIT_PROBE_TIMEOUT.as_secs()
            )
        })?
        .context("spawn git")?;

    Ok(RawGitOutput {
        success: output.status.success(),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedStatus {
    head: Option<String>,
    branch: Option<String>,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    changes: Vec<GitFileChange>,
}

fn parse_porcelain_v2(bytes: &[u8]) -> Result<ParsedStatus> {
    let records = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut parsed = ParsedStatus::default();
    let mut index = 0;

    while index < records.len() {
        let record = records[index];
        index += 1;
        if record.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(record);

        if let Some(value) = text.strip_prefix("# branch.oid ") {
            if value != "(initial)" {
                parsed.head = Some(value.to_string());
            }
            continue;
        }
        if let Some(value) = text.strip_prefix("# branch.head ") {
            if value != "(detached)" {
                parsed.branch = Some(value.to_string());
            }
            continue;
        }
        if let Some(value) = text.strip_prefix("# branch.upstream ") {
            parsed.upstream = Some(value.to_string());
            continue;
        }
        if let Some(value) = text.strip_prefix("# branch.ab ") {
            for part in value.split_whitespace() {
                if let Some(ahead) = part.strip_prefix('+') {
                    parsed.ahead = ahead.parse().unwrap_or(0);
                } else if let Some(behind) = part.strip_prefix('-') {
                    parsed.behind = behind.parse().unwrap_or(0);
                }
            }
            continue;
        }

        if text.starts_with("1 ") {
            let fields = text.splitn(9, ' ').collect::<Vec<_>>();
            if fields.len() != 9 {
                return Err(anyhow!("invalid porcelain v2 ordinary entry: {text}"));
            }
            parsed.changes.push(change_from_xy(
                fields[1],
                fields[8].to_string(),
                None,
                false,
                false,
            )?);
            continue;
        }

        if text.starts_with("2 ") {
            let fields = text.splitn(10, ' ').collect::<Vec<_>>();
            if fields.len() != 10 {
                return Err(anyhow!("invalid porcelain v2 rename entry: {text}"));
            }
            let original = records
                .get(index)
                .context("porcelain v2 rename entry missing original path")?;
            index += 1;
            parsed.changes.push(change_from_xy(
                fields[1],
                fields[9].to_string(),
                Some(String::from_utf8_lossy(original).into_owned()),
                false,
                false,
            )?);
            continue;
        }

        if text.starts_with("u ") {
            let fields = text.splitn(11, ' ').collect::<Vec<_>>();
            if fields.len() != 11 {
                return Err(anyhow!("invalid porcelain v2 unmerged entry: {text}"));
            }
            parsed.changes.push(change_from_xy(
                fields[1],
                fields[10].to_string(),
                None,
                false,
                true,
            )?);
            continue;
        }

        if let Some(path) = text.strip_prefix("? ") {
            parsed.changes.push(GitFileChange {
                path: path.to_string(),
                original_path: None,
                index_status: None,
                worktree_status: None,
                untracked: true,
                conflict: false,
            });
            continue;
        }
    }

    Ok(parsed)
}

fn change_from_xy(
    xy: &str,
    path: String,
    original_path: Option<String>,
    untracked: bool,
    conflict: bool,
) -> Result<GitFileChange> {
    let mut chars = xy.chars();
    let index = chars.next().context("status missing index code")?;
    let worktree = chars.next().context("status missing worktree code")?;
    Ok(GitFileChange {
        path,
        original_path,
        index_status: (index != '.').then_some(index),
        worktree_status: (worktree != '.').then_some(worktree),
        untracked,
        conflict,
    })
}

fn primary_root_from_common_dir(common_dir: &str, worktree_root: &str) -> String {
    let path = Path::new(common_dir);
    if path.file_name().is_some_and(|name| name == ".git") {
        return path
            .parent()
            .unwrap_or(Path::new(worktree_root))
            .to_string_lossy()
            .into_owned();
    }
    common_dir.to_string()
}

fn looks_like_not_repository(stderr: &str) -> bool {
    stderr.to_ascii_lowercase().contains("not a git repository")
}

fn now_unix_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_v2_preserves_branch_dirty_kinds_and_rename_identity() {
        let input = b"# branch.oid abcdef0123456789\0# branch.head feature/test\0# branch.upstream origin/feature/test\0# branch.ab +2 -1\0\
1 M. N... 100644 100644 100644 aaaaaaa bbbbbbb src/lib.rs\0\
1 .M N... 100644 100644 100644 aaaaaaa bbbbbbb README.md\0\
2 R. N... 100644 100644 100644 aaaaaaa bbbbbbb R100 new name.rs\0old name.rs\0\
? scratch file.txt\0";
        let parsed = parse_porcelain_v2(input).expect("parse");
        assert_eq!(parsed.branch.as_deref(), Some("feature/test"));
        assert_eq!(parsed.upstream.as_deref(), Some("origin/feature/test"));
        assert_eq!(parsed.ahead, 2);
        assert_eq!(parsed.behind, 1);
        assert_eq!(parsed.changes.len(), 4);
        assert_eq!(parsed.changes[0].index_status, Some('M'));
        assert_eq!(parsed.changes[1].worktree_status, Some('M'));
        assert_eq!(parsed.changes[2].path, "new name.rs");
        assert_eq!(
            parsed.changes[2].original_path.as_deref(),
            Some("old name.rs")
        );
        assert!(parsed.changes[3].untracked);
    }

    #[test]
    fn detached_head_does_not_invent_a_branch() {
        let parsed = parse_porcelain_v2(b"# branch.oid deadbeef\0# branch.head (detached)\0")
            .expect("parse");
        assert_eq!(parsed.head.as_deref(), Some("deadbeef"));
        assert!(parsed.branch.is_none());
    }

    #[test]
    fn linked_worktrees_share_primary_repository_identity() {
        assert_eq!(
            primary_root_from_common_dir("/repo/.git", "/tmp/worktree"),
            "/repo"
        );
        assert_eq!(
            primary_root_from_common_dir("/srv/bare.git", "/tmp/worktree"),
            "/srv/bare.git"
        );
    }
}
