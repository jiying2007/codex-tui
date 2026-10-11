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
    // Only NUL-delimited porcelain preserves worktree paths containing
    // newlines and trailing spaces. An incomplete inventory is never evidence
    // that a worktree/branch is absent before a destructive operation.
    let output = run_git(cwd, ["worktree", "list", "--porcelain", "-z"]).await?;
    anyhow::ensure!(
        output.success,
        "git worktree list failed: {}",
        crate::hardening::safe_external_stderr(&output.stderr, "git child")
    );
    parse_worktree_porcelain(&output.stdout)
}

pub(crate) fn parse_worktree_porcelain(value: &str) -> Result<Vec<WorktreeEntry>> {
    anyhow::ensure!(
        value.ends_with("\0\0"),
        "Git worktree porcelain inventory is incomplete or not NUL-delimited"
    );
    let mut entries = Vec::new();
    let mut path: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut has_identity = false;

    for record in value.split_terminator('\0') {
        if record.is_empty() {
            let path = path.take().context("Git worktree record is missing its path")?;
            anyhow::ensure!(has_identity, "Git worktree record is missing HEAD/bare identity");
            entries.push(WorktreeEntry {
                path,
                branch: branch.take(),
            });
            has_identity = false;
            continue;
        }
        if let Some(value) = record.strip_prefix("worktree ") {
            anyhow::ensure!(path.is_none(), "Git worktree record is missing a separator");
            anyhow::ensure!(!value.is_empty(), "Git worktree record has an empty path");
            path = Some(value.to_owned());
            continue;
        }
        anyhow::ensure!(path.is_some(), "Git worktree metadata has no worktree path");
        if record.starts_with("HEAD ") || record == "bare" {
            has_identity = true;
        } else if let Some(value) = record.strip_prefix("branch refs/heads/") {
            anyhow::ensure!(!value.is_empty(), "Git worktree branch ref is empty");
            branch = Some(value.to_owned());
        } else if record == "detached" {
            branch = None;
        } else if !(record == "locked"
            || record.starts_with("locked ")
            || record == "prunable"
            || record.starts_with("prunable "))
        {
            anyhow::bail!("unrecognized Git worktree porcelain field");
        }
    }

    anyhow::ensure!(path.is_none(), "Git worktree porcelain ended mid-record");
    Ok(entries)
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
        _ => Err(anyhow!(
            "git show-ref failed: {}",
            crate::hardening::safe_external_stderr(&output.stderr, "git child")
        )),
    }
}

pub(crate) async fn worktree_is_clean(path: &str) -> Result<bool> {
    let output = run_git(path, ["status", "--porcelain", "--untracked-files=all"]).await?;
    anyhow::ensure!(
        output.success,
        "git status failed: {}",
        crate::hardening::safe_external_stderr(&output.stderr, "git child")
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
        let (stdout, stdout_truncated) = stdout.context("read git stdout")?;
        let (stderr, _) = stderr.context("read git stderr")?;
        let status = status.context("wait for git")?;
        anyhow::ensure!(
            !stdout_truncated,
            "Git stdout exceeded {MAX_MUTATION_OUTPUT_BYTES} bytes; refusing incomplete mutation/verification evidence"
        );
        let stdout = String::from_utf8(stdout)
            .context("Git stdout is not UTF-8; refusing lossy worktree identity")?;
        Result::<_, anyhow::Error>::Ok(MutationOutput {
            success: status.success(),
            code: status.code(),
            stdout,
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    };

    tokio::time::timeout(timeout, capture)
        .await
        .with_context(|| format!("git mutation timed out after {}s", timeout.as_secs()))?
}

async fn read_capped<R>(mut reader: R, limit: usize) -> std::io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut stored = Vec::with_capacity(limit.min(8192));
    let mut truncated = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(stored.len());
        let keep = remaining.min(read);
        stored.extend_from_slice(&buffer[..keep]);
        truncated |= read > keep;
        // Continue draining even when truncated to let Git exit and be reaped.
    }
    Ok((stored, truncated))
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn capped_stdout_drains_excess_bytes_and_reports_truncation() {
        let (mut writer, reader) = tokio::io::duplex(8);
        let write = tokio::spawn(async move {
            writer.write_all(b"123456789").await.unwrap();
        });
        let (bytes, truncated) = read_capped(reader, 4).await.unwrap();
        write.await.unwrap();
        assert_eq!(bytes, b"1234");
        assert!(truncated);
    }

    #[tokio::test]
    async fn capped_stdout_accepts_exact_limit_without_truncation() {
        let (mut writer, reader) = tokio::io::duplex(8);
        writer.write_all(b"1234").await.unwrap();
        drop(writer);
        let (bytes, truncated) = read_capped(reader, 4).await.unwrap();
        assert_eq!(bytes, b"1234");
        assert!(!truncated);
    }
}
