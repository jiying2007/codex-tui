//! One bounded, non-executing Git review of staged and unstaged content.
use super::*;

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
            "--no-textconv",
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
        [
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--unified=3",
            "--",
        ],
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
