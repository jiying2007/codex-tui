use crate::domain::{LocalRepoIdentity, ThreadId, WorktreeIdentity};
use crate::latest_read::{ReadEnvelope, ReadFence, ReadScope, next_current};
use anyhow::{Context, Result, anyhow};
use similar::{ChangeTag, TextDiff};
use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

const GIT_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const GIT_COMMAND_QUEUE_CAPACITY: usize = 128;
const GIT_EVENT_QUEUE_CAPACITY: usize = 128;
const GIT_MAX_CONCURRENCY: usize = 8;
const MAX_REVIEW_DIFF_BYTES: usize = 2 * 1024 * 1024;
const MAX_GIT_STDERR_BYTES: usize = 64 * 1024;
const MAX_GIT_PROBE_BYTES: usize = 2 * 1024 * 1024;
const MAX_GIT_IDENTITY_BYTES: usize = 64 * 1024;
const MAX_GIT_STATUS_RECORDS: usize = 8192;

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
    // Clones for same-cwd threads and planning snapshots share one immutable inventory.
    pub changes: Arc<[GitFileChange]>,
    pub observed_at_unix_ms: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitReview {
    pub thread_id: ThreadId,
    pub cwd: String,
    pub changes: Vec<GitFileChange>,
    pub staged_diff: String,
    pub unstaged_diff: String,
    pub truncated: bool,
    pub observed_at_unix_ms: u64,
    pub error: Option<String>,
}

impl GitReview {
    pub fn pending(thread_id: ThreadId, cwd: impl Into<String>) -> Self {
        Self {
            thread_id,
            cwd: cwd.into(),
            changes: vec![],
            staged_diff: String::new(),
            unstaged_diff: String::new(),
            truncated: false,
            observed_at_unix_ms: 0,
            error: None,
        }
    }

    fn failed(thread_id: ThreadId, cwd: String, error: String) -> Self {
        let mut review = Self::pending(thread_id, cwd);
        review.observed_at_unix_ms = now_unix_ms();
        review.error = Some(error);
        review
    }

    pub fn combined_diff(&self) -> String {
        let mut out = String::new();
        if !self.staged_diff.is_empty() {
            out.push_str("### staged\n");
            out.push_str(&self.staged_diff);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        if !self.unstaged_diff.is_empty() {
            out.push_str("### unstaged\n");
            out.push_str(&self.unstaged_diff);
        }
        if self.truncated {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("[diff output truncated]\n");
        }
        out
    }
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
            changes: Vec::new().into(),
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
    LoadReview { thread_id: ThreadId, cwd: String },
}

#[derive(Clone, Debug)]
pub enum GitEvent {
    Context(GitContext),
    Review(GitReview),
}

pub struct GitHandle {
    command_tx: mpsc::Sender<ReadEnvelope<GitCommand>>,
    event_rx: mpsc::Receiver<ReadEnvelope<GitEvent>>,
    task: JoinHandle<()>,
    reads: ReadFence,
}

impl GitHandle {
    pub fn start() -> Self {
        let (command_tx, command_rx) = mpsc::channel(GIT_COMMAND_QUEUE_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel(GIT_EVENT_QUEUE_CAPACITY);
        let task = tokio::spawn(run_actor(command_rx, event_tx));
        Self {
            command_tx,
            event_rx,
            task,
            reads: ReadFence::default(),
        }
    }

    pub fn probe(&self, thread_id: ThreadId, cwd: String) -> Result<()> {
        self.queue_command(GitCommand::Probe { thread_id, cwd })
    }

    pub fn load_review(&self, thread_id: ThreadId, cwd: String) -> Result<()> {
        self.queue_command(GitCommand::LoadReview { thread_id, cwd })
    }

    fn queue_command(&self, command: GitCommand) -> Result<()> {
        let scope = match &command {
            GitCommand::Probe { cwd, .. } => ReadScope::Snapshot(cwd.clone()),
            GitCommand::LoadReview { thread_id, .. } => ReadScope::Review(thread_id.0.clone()),
        };
        self.reads.submit(&self.command_tx, scope, command, "Git")
    }

    pub fn try_recv(&mut self) -> Option<GitEvent> {
        next_current(&mut self.event_rx, GIT_EVENT_QUEUE_CAPACITY)
    }
}

impl Drop for GitHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run_actor(
    mut command_rx: mpsc::Receiver<ReadEnvelope<GitCommand>>,
    event_tx: mpsc::Sender<ReadEnvelope<GitEvent>>,
) {
    let mut tasks = JoinSet::new();
    let mut command_open = true;

    loop {
        tokio::select! {
            joined = tasks.join_next(), if !tasks.is_empty() => {
                let _ = joined;
            }
            command = command_rx.recv(), if command_open && tasks.len() < GIT_MAX_CONCURRENCY => {
                match command {
                    Some(ReadEnvelope { value: command, ticket }) => {
                        let event_tx = event_tx.clone();
                        tasks.spawn(async move {
                            let event = match command {
                                GitCommand::Probe { thread_id, cwd } => {
                                    let context = match probe_context(thread_id.clone(), cwd.clone()).await {
                                        Ok(context) => context,
                                        Err(error) => GitContext::failed(thread_id, cwd, error.to_string()),
                                    };
                                    GitEvent::Context(context)
                                }
                                GitCommand::LoadReview { thread_id, cwd } => {
                                    let review = match load_review(thread_id.clone(), cwd.clone()).await {
                                        Ok(review) => match tokio::task::spawn_blocking(move || {
                                            crate::syntax_highlight::prewarm_review_diff(&review);
                                            review
                                        }).await {
                                            Ok(review) => review,
                                            Err(error) => GitReview::failed(thread_id, cwd,
                                                format!("review presentation worker failed: {error}")),
                                        },
                                        Err(error) => GitReview::failed(thread_id, cwd, error.to_string()),
                                    };
                                    GitEvent::Review(review)
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

pub async fn probe_context(thread_id: ThreadId, cwd: String) -> Result<GitContext> {
    let worktree_probe = run_git(&cwd, ["rev-parse", "--show-toplevel"]).await?;

    if !worktree_probe.success {
        if looks_like_not_repository(&worktree_probe.stderr) {
            return Ok(GitContext::not_repository(thread_id, cwd));
        }
        return Err(anyhow!(
            "git worktree-root probe failed: {}",
            crate::hardening::safe_external_stderr(&worktree_probe.stderr, "git root probe")
        ));
    }

    let common_dir_probe = run_git(&cwd, ["rev-parse", "--git-common-dir"]).await?;
    if !common_dir_probe.success {
        return Err(anyhow!(
            "git common-dir probe failed: {}",
            crate::hardening::safe_external_stderr(&common_dir_probe.stderr, "git common-dir")
        ));
    }

    // Each invocation emits one path plus one LF, not a list of trimmed lines.
    let worktree_root =
        canonical_identity_path_from(&cwd, rev_parse_path(&worktree_probe.stdout)?)?;
    let common_dir = canonical_identity_path_from(&cwd, rev_parse_path(&common_dir_probe.stdout)?)?;
    let canonical_cwd = std::fs::canonicalize(&cwd).context("resolve Git probe cwd")?;
    anyhow::ensure!(
        canonical_cwd.is_dir()
            && Path::new(&worktree_root).is_dir()
            && canonical_cwd.starts_with(&worktree_root)
            && Path::new(&common_dir).is_dir(),
        "Git path identity does not match the probed cwd"
    );
    let primary_root =
        canonical_identity_path(primary_root_from_common_dir(&common_dir, &worktree_root))?;

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
            crate::hardening::safe_external_stderr(
                &String::from_utf8_lossy(&status.stderr),
                "git status"
            )
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
        changes: parsed.changes.into(),
        observed_at_unix_ms: now_unix_ms(),
        error: None,
    })
}

pub async fn load_review(thread_id: ThreadId, cwd: String) -> Result<GitReview> {
    let context = probe_context(thread_id.clone(), cwd.clone()).await?;
    if !context.is_repository {
        return Err(anyhow!("cwd is not inside a Git repository"));
    }

    let staged = run_git_capped(
        &cwd,
        [
            "diff",
            "--cached",
            "--no-ext-diff",
            "--no-color",
            "--unified=3",
            "--",
        ],
        MAX_REVIEW_DIFF_BYTES,
    )
    .await?;
    if !staged.success {
        return Err(anyhow!(
            "git staged diff failed: {}",
            crate::hardening::safe_external_stderr(&staged.stderr, "git staged diff")
        ));
    }

    let unstaged = run_git_capped(
        &cwd,
        ["diff", "--no-ext-diff", "--no-color", "--unified=3", "--"],
        MAX_REVIEW_DIFF_BYTES,
    )
    .await?;
    if !unstaged.success {
        return Err(anyhow!(
            "git unstaged diff failed: {}",
            crate::hardening::safe_external_stderr(&unstaged.stderr, "git unstaged diff")
        ));
    }

    Ok(GitReview {
        thread_id,
        cwd,
        changes: context.changes.as_ref().to_vec(),
        staged_diff: staged.stdout,
        unstaged_diff: unstaged.stdout,
        truncated: staged.truncated || unstaged.truncated,
        observed_at_unix_ms: now_unix_ms(),
        error: None,
    })
}

#[derive(Debug)]
struct CappedGitOutput {
    success: bool,
    stdout: String,
    stderr: String,
    truncated: bool,
}

async fn run_git_capped<I, S>(cwd: &str, args: I, stdout_limit: usize) -> Result<CappedGitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = capture_git(cwd, args, stdout_limit).await?;
    Ok(CappedGitOutput {
        success: output.success,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        truncated: output.stdout_truncated,
    })
}

// Streams, buffers and child wait belong to this future and die on cancellation.
// Diff may retain a bounded display prefix; identity/status callers reject overflow.
async fn capture_git<I, S>(cwd: &str, args: I, stdout_limit: usize) -> Result<CapturedGitOutput>
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
    let mut child = command.spawn().context("spawn git")?;
    let stdout = child.stdout.take().context("git stdout unavailable")?;
    let stderr = child.stderr.take().context("git stderr unavailable")?;
    let capture = async {
        let ((stdout, stdout_truncated), (stderr, stderr_truncated), status) = tokio::try_join!(
            read_capped(stdout, stdout_limit),
            read_capped(stderr, MAX_GIT_STDERR_BYTES),
            child.wait(),
        )
        .context("capture Git output")?;
        Ok(CapturedGitOutput {
            success: status.success(),
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
        })
    };
    tokio::time::timeout(GIT_PROBE_TIMEOUT, capture)
        .await
        .with_context(|| {
            format!(
                "git command timed out after {}s",
                GIT_PROBE_TIMEOUT.as_secs()
            )
        })?
}

async fn read_capped<R>(mut reader: R, limit: usize) -> std::io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut stored = Vec::with_capacity(limit.min(8192));
    // Avoid multiplying inline arrays through nested Doctor/Forge async states.
    let mut buffer = vec![0_u8; 8192];
    let mut truncated = false;

    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(stored.len());
        let keep = remaining.min(read);
        stored.extend_from_slice(&buffer[..keep]);
        truncated |= keep < read;
    }

    Ok((stored, truncated))
}

pub fn presentation_diff_lines(review: &GitReview, word_diff: bool) -> Vec<String> {
    let raw = review.combined_diff();
    if !word_diff {
        return raw.lines().map(crate::text::sanitize_inline).collect();
    }

    let source = raw.lines().collect::<Vec<_>>();
    let mut lines = Vec::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        let current = source[index];
        if current.starts_with('-')
            && !current.starts_with("---")
            && let Some(next) = source.get(index + 1)
            && next.starts_with('+')
            && !next.starts_with("+++")
        {
            let (old, new) = inline_word_pair(&current[1..], &next[1..]);
            lines.push(format!("-{old}"));
            lines.push(format!("+{new}"));
            index += 2;
            continue;
        }
        lines.push(current.to_string());
        index += 1;
    }
    lines
        .into_iter()
        .map(|line| crate::text::sanitize_inline(&line))
        .collect()
}

fn inline_word_pair(old: &str, new: &str) -> (String, String) {
    let diff = TextDiff::from_words(old, new);
    let mut old_out = String::new();
    let mut new_out = String::new();
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                old_out.push_str(change.value());
                new_out.push_str(change.value());
            }
            ChangeTag::Delete => {
                old_out.push_str("[-");
                old_out.push_str(change.value());
                old_out.push_str("-]");
            }
            ChangeTag::Insert => {
                new_out.push_str("{+");
                new_out.push_str(change.value());
                new_out.push_str("+}");
            }
        }
    }
    (old_out, new_out)
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
    let output = run_git_raw(cwd, args, MAX_GIT_IDENTITY_BYTES).await?;
    Ok(GitOutput {
        success: output.success,
        stdout: String::from_utf8(output.stdout).context("Git identity is not valid UTF-8")?,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

async fn run_git_bytes<I, S>(cwd: &str, args: I) -> Result<GitBytesOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_git_raw(cwd, args, MAX_GIT_PROBE_BYTES).await?;
    Ok(GitBytesOutput {
        success: output.success,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

#[derive(Debug)]
struct CapturedGitOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

async fn run_git_raw<I, S>(cwd: &str, args: I, stdout_limit: usize) -> Result<CapturedGitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = capture_git(cwd, args, stdout_limit).await?;
    anyhow::ensure!(
        !output.stdout_truncated && !output.stderr_truncated,
        "Git probe exceeds output budget (stdout {stdout_limit} bytes, stderr {MAX_GIT_STDERR_BYTES} bytes); inventory unavailable"
    );
    Ok(output)
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
    anyhow::ensure!(
        bytes.len() <= MAX_GIT_PROBE_BYTES,
        "Git status exceeds byte budget"
    );
    anyhow::ensure!(
        bytes.is_empty() || bytes.ends_with(&[0]),
        "Git status record is incomplete"
    );
    let mut records = bytes.split(|byte| *byte == 0);
    let mut parsed = ParsedStatus::default();
    let mut count = 0;
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        count += 1;
        anyhow::ensure!(
            count <= MAX_GIT_STATUS_RECORDS,
            "Git status exceeds record budget"
        );
        let text = std::str::from_utf8(record).context("Git status identity is not valid UTF-8")?;
        if let Some(value) = text.strip_prefix("# branch.oid ") {
            if value != "(initial)" {
                parsed.head = Some(value.to_string());
            }
        } else if let Some(value) = text.strip_prefix("# branch.head ") {
            if value != "(detached)" {
                parsed.branch = Some(value.to_string());
            }
        } else if let Some(value) = text.strip_prefix("# branch.upstream ") {
            parsed.upstream = Some(value.to_string());
        } else if let Some(value) = text.strip_prefix("# branch.ab ") {
            for part in value.split_whitespace() {
                if let Some(ahead) = part.strip_prefix('+') {
                    parsed.ahead = ahead.parse().context("invalid Git ahead count")?;
                } else if let Some(behind) = part.strip_prefix('-') {
                    parsed.behind = behind.parse().context("invalid Git behind count")?;
                }
            }
        } else if text.starts_with("1 ") {
            let fields = text.splitn(9, ' ').collect::<Vec<_>>();
            anyhow::ensure!(fields.len() == 9, "invalid Git ordinary status record");
            parsed.changes.push(change_from_xy(
                fields[1],
                fields[8].into(),
                None,
                false,
                false,
            )?);
        } else if text.starts_with("2 ") {
            let fields = text.splitn(10, ' ').collect::<Vec<_>>();
            anyhow::ensure!(fields.len() == 10, "invalid Git rename status record");
            let original = records
                .next()
                .context("Git rename is missing its original path")?;
            count += 1;
            anyhow::ensure!(
                count <= MAX_GIT_STATUS_RECORDS,
                "Git status exceeds record budget"
            );
            anyhow::ensure!(
                !original.is_empty(),
                "Git rename has an empty original path"
            );
            let original =
                std::str::from_utf8(original).context("Git rename identity is not valid UTF-8")?;
            validate_status_path(original)?;
            parsed.changes.push(change_from_xy(
                fields[1],
                fields[9].into(),
                Some(original.into()),
                false,
                false,
            )?);
        } else if text.starts_with("u ") {
            let fields = text.splitn(11, ' ').collect::<Vec<_>>();
            anyhow::ensure!(fields.len() == 11, "invalid Git unmerged status record");
            parsed.changes.push(change_from_xy(
                fields[1],
                fields[10].into(),
                None,
                false,
                true,
            )?);
        } else if let Some(path) = text.strip_prefix("? ") {
            validate_status_path(path)?;
            parsed.changes.push(GitFileChange {
                path: path.into(),
                original_path: None,
                index_status: None,
                worktree_status: None,
                untracked: true,
                conflict: false,
            });
        } else {
            // Porcelain permits extensible headers and explicitly ignored files.
            anyhow::ensure!(
                text.starts_with("# ") || text.starts_with("! "),
                "unknown Git status record"
            );
        }
    }
    Ok(parsed)
}

fn validate_status_path(path: &str) -> Result<()> {
    anyhow::ensure!(
        !path.is_empty()
            && Path::new(path)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
        "Git status path is not worktree-relative"
    );
    Ok(())
}

fn change_from_xy(
    xy: &str,
    path: String,
    original_path: Option<String>,
    untracked: bool,
    conflict: bool,
) -> Result<GitFileChange> {
    validate_status_path(&path)?;
    anyhow::ensure!(xy.len() == 2, "invalid Git status code");
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

fn rev_parse_path(output: &str) -> Result<&str> {
    let path = output
        .strip_suffix('\n')
        .context("Git path response is missing its terminator")?;
    // On Unix CR and extra LF bytes may be part of the path itself.
    #[cfg(windows)]
    let path = path.strip_suffix('\r').unwrap_or(path);
    anyhow::ensure!(
        !path.is_empty() && !path.contains('\0'),
        "Git path response is invalid"
    );
    Ok(path)
}

fn canonical_identity_path(value: impl AsRef<Path>) -> Result<String> {
    let canonical = std::fs::canonicalize(value).context("resolve Git path identity")?;
    canonical
        .into_os_string()
        .into_string()
        .map_err(|_| anyhow!("Git path identity is not valid UTF-8"))
}

fn canonical_identity_path_from(cwd: &str, value: &str) -> Result<String> {
    let path = Path::new(value);
    if path.is_absolute() {
        canonical_identity_path(path)
    } else {
        canonical_identity_path(Path::new(cwd).join(path))
    }
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
mod tests;

#[cfg(test)]
mod read_order_tests;

#[cfg(test)]
mod audit_tests;
