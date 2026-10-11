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
fn presentation_word_diff_only_changes_display_layer() {
    let review = GitReview {
        thread_id: ThreadId::new("t"),
        cwd: "/repo".into(),
        changes: vec![],
        staged_diff: String::new(),
        unstaged_diff: "-hello old world\n+hello new world\n".into(),
        truncated: false,
        observed_at_unix_ms: 1,
        error: None,
    };
    let plain = presentation_diff_lines(&review, false);
    assert_eq!(plain[1], "-hello old world");
    let word = presentation_diff_lines(&review, true);
    assert!(word.iter().any(|line| line.contains("[-old-]")));
    assert!(word.iter().any(|line| line.contains("{+new+}")));
}

#[test]
fn git_diff_body_does_not_reach_terminal_with_control_or_bidi_sequences() {
    let mut review = GitReview::pending(ThreadId::new("hostile"), "/repo");
    review.staged_diff = format!(
        "diff --git a/file b/file\n-old{}[31m\n+new{}concealed{}next\n",
        '\u{001b}', '\u{202e}', '\u{2029}'
    );
    // Review authority retains source bytes; only the terminal projection is cleaned.
    assert!(review.staged_diff.contains('\u{001b}'));
    for word_diff in [false, true] {
        let shown = presentation_diff_lines(&review, word_diff);
        assert!(shown.iter().any(|line| line.contains("new")));
        for line in shown {
            assert!(!line.contains('\u{001b}'));
            assert!(!line.contains('\u{202e}'));
            assert!(!line.contains('\u{2029}'));
            assert!(!line.contains('\n'));
            assert!(!line.contains('\r'));
        }
    }
}

#[tokio::test]
async fn capped_reader_bounds_memory_even_at_utf8_boundary() {
    let payload = "é".repeat(10_000).into_bytes();
    let (mut writer, reader) = tokio::io::duplex(payload.len() + 16);
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        writer.write_all(&payload).await.expect("write payload");
    });
    let (captured, truncated) = read_capped(reader, 1025).await.expect("read capped");
    assert!(truncated);
    assert!(captured.len() <= 1025);
    let display = String::from_utf8_lossy(&captured);
    assert!(!display.is_empty());
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

#[test]
fn relative_identity_paths_resolve_from_the_probed_cwd() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("repo");
    let git_dir = root.join(".git");
    std::fs::create_dir_all(&git_dir).expect("create git dir");

    let cwd = root.to_string_lossy().into_owned();
    assert_eq!(
        canonical_identity_path_from(&cwd, ".git"),
        canonical_identity_path(&git_dir)
    );
    assert_eq!(
        canonical_identity_path_from(&cwd, git_dir.to_string_lossy().as_ref()),
        canonical_identity_path(&git_dir)
    );
}

#[tokio::test]
async fn real_repo_and_linked_worktree_share_repo_identity() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("repo");
    std::fs::create_dir_all(&root).expect("create repo");
    git(&root, &["init"]);
    git(&root, &["config", "user.email", "ci@example.invalid"]);
    git(&root, &["config", "user.name", "CI"]);
    std::fs::write(root.join("tracked.txt"), "base\n").expect("write");
    git(&root, &["add", "tracked.txt"]);
    git(&root, &["commit", "-m", "base"]);

    let linked = temp.path().join("linked");
    let linked_text = linked.to_string_lossy().into_owned();
    git(&root, &["worktree", "add", "-b", "feature", &linked_text]);

    let main = probe_context(
        ThreadId::new("main-thread"),
        root.to_string_lossy().into_owned(),
    )
    .await
    .expect("main context");
    let other = probe_context(
        ThreadId::new("linked-thread"),
        linked.to_string_lossy().into_owned(),
    )
    .await
    .expect("linked context");

    assert_eq!(main.repo, other.repo);
    assert_ne!(
        main.worktree
            .as_ref()
            .expect("main worktree")
            .canonical_path,
        other
            .worktree
            .as_ref()
            .expect("linked worktree")
            .canonical_path
    );
    assert_eq!(other.branch.as_deref(), Some("feature"));
}

#[tokio::test]
async fn real_dirty_file_is_projected_from_porcelain_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    git(root, &["init"]);
    git(root, &["config", "user.email", "ci@example.invalid"]);
    git(root, &["config", "user.name", "CI"]);
    std::fs::write(root.join("tracked.txt"), "base\n").expect("write");
    git(root, &["add", "tracked.txt"]);
    git(root, &["commit", "-m", "base"]);
    std::fs::write(root.join("tracked.txt"), "changed\n").expect("change");

    let context = probe_context(
        ThreadId::new("dirty-thread"),
        root.to_string_lossy().into_owned(),
    )
    .await
    .expect("context");
    assert!(context.dirty);
    assert!(
        context.changes.iter().any(|change| {
            change.path == "tracked.txt" && change.worktree_status == Some('M')
        })
    );
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
