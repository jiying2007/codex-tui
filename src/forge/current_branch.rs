//! A single local branch lookup when a bounded Forge overview is saturated.
use super::*;
use std::path::Path;

pub(crate) async fn current_local_branch(cwd: &str) -> Option<String> {
    let output = run_command(
        "git",
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        Some(Path::new(cwd)),
    )
    .await
    .ok()?;
    if !output.success {
        return None;
    }
    let branch = output
        .stdout
        .trim_end_matches(|ch| ch == '\r' || ch == '\n');
    (!branch.is_empty() && !branch.chars().any(|ch| matches!(ch, '\r' | '\n' | '\0')))
        .then(|| branch.to_string())
}

pub(crate) fn needs_exact_branch<T>(
    recent: &[T],
    branch: &str,
    mut projected: impl FnMut(&T) -> String,
) -> bool {
    recent.len() >= DEFAULT_PAGE_SIZE && !recent.iter().any(|item| projected(item) == branch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_query_is_needed_only_for_a_saturated_unmatched_branch() {
        let recent = vec!["other".to_owned(); DEFAULT_PAGE_SIZE];
        assert!(needs_exact_branch(&recent, "topic", Clone::clone));
        assert!(!needs_exact_branch(
            &recent[..DEFAULT_PAGE_SIZE - 1],
            "topic",
            Clone::clone
        ));
        let mut matching = recent;
        matching[4] = "topic".into();
        assert!(!needs_exact_branch(&matching, "topic", Clone::clone));
    }

    #[tokio::test]
    async fn detached_or_missing_git_head_does_not_create_false_branch_identity() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(
            current_local_branch(tmp.path().to_str().unwrap())
                .await
                .is_none()
        );
    }
}
