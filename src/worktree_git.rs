use anyhow::{Context, Result, anyhow};
use std::{ffi::OsStr, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

const MUTATION_TIMEOUT: Duration = Duration::from_secs(15);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_MUTATION_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Debug)]
pub(crate) struct WorktreeEntry {
    pub(crate) path: String,
    pub(crate) branch: Option<String>,
}

pub(crate) async fn list_worktrees(cwd: &str) -> Result<Vec<WorktreeEntry>> {
    let output = run_git(cwd, ["worktree", "list", "--porcelain"]).await?;
    anyhow::ensure!(
        output.success,
        "git worktree list failed: {}",
        output.stderr.trim()
    );
    Ok(parse_worktree_porcelain(&output.stdout))
}

pub(crate) fn parse_worktree_porcelain(value: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut path: Option<String> = None;
    let mut branch: Option<String> = None;

    for raw_line in value.lines().chain(std::iter::once("")) {
        let line = raw_line.trim();
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

pub(crate) async fn branch_exists(cwd: &str, branch: &str) -> Result<bool> {
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

pub(crate) async fn worktree_is_clean(path: &str) -> Result<bool> {
    let output = run_git(path, ["status", "--porcelain", "--untracked-files=all"]).await?;
    anyhow::ensure!(
        output.success,
        "git status failed: {}",
        output.stderr.trim()
    );
    Ok(output.stdout.trim().is_empty())
}

#[derive(Debug)]
pub(crate) struct MutationOutput {
    pub(crate) success: bool,
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) async fn run_git_mutation(cwd: &str, argv: &[String]) -> Result<MutationOutput> {
    run_command(cwd, argv, MUTATION_TIMEOUT).await
}

pub(crate) async fn run_git<I, S>(cwd: &str, args: I) -> Result<MutationOutput>
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

pub(crate) fn canonical_path(value: &str) -> String {
    std::fs::canonicalize(value)
        .unwrap_or_else(|_| PathBuf::from(value))
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn same_path(left: &str, right: &str) -> bool {
    let left = canonical_path(left);
    let right = canonical_path(right);
    if cfg!(windows) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}
