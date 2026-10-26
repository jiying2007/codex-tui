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

pub(super) fn isolate_git_environment(command: &mut std::process::Command) {
    // Per-child only: parallel tests must not rewrite the test runner's env.
    for (key, _) in std::env::vars_os() {
        if key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(key);
        }
    }
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", null)
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_COUNT", "0")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env("GIT_TEMPLATE_DIR", "")
        .args([
            "-c",
            &format!("core.hooksPath={null}"),
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
        ]);
}

pub(super) fn git_command(cwd: &Path, args: &[&str]) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    isolate_git_environment(&mut command);
    command.arg("-C").arg(cwd).args(args);
    command
}

pub(super) fn protect_fixture_repository(cwd: &Path) {
    // Production read/mutation code subsequently uses this temporary repo.
    // Pin these local values (including LF checkout) as well, without changing
    // production Git policy or depending on a platform global config.
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    for (key, value) in [
        ("core.hooksPath", null),
        ("commit.gpgsign", "false"),
        ("tag.gpgsign", "false"),
        ("core.fsmonitor", "false"),
        ("core.autocrlf", "false"),
        ("core.eol", "lf"),
    ] {
        assert!(
            git_command(cwd, &["config", "--local", key, value])
                .status()
                .unwrap()
                .success()
        );
    }
}

fn git(cwd: &Path, args: &[&str]) {
    let status = git_command(cwd, args).status().expect("spawn git");
    assert!(status.success(), "git command failed: {args:?}");
    if args.first() == Some(&"init") {
        protect_fixture_repository(cwd);
    }
}

#[cfg(unix)]
#[test]
fn fixture_git_ignores_hostile_inherited_config_and_hook_programs() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let hooks = root.path().join("hostile-hooks");
    std::fs::create_dir(&hooks).unwrap();
    let marker = root.path().join("unexpected-hook");
    let hook = hooks.join("pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf called > \"$FIXTURE_HOOK_MARKER\"\nexit 97\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let hostile = root.path().join("hostile.config");
    std::fs::write(&hostile, format!("[core]\n hooksPath = {}\n[commit]\n gpgsign = true\n[gpg]\n program = {}\n[safety]\n inherited = forbidden\n", hooks.display(), hook.display())).unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init"]);
    let mut command = std::process::Command::new("git");
    command
        .env("FIXTURE_HOOK_MARKER", &marker)
        .env("GIT_CONFIG_GLOBAL", &hostile)
        .env("GIT_CONFIG_SYSTEM", &hostile)
        .env("GIT_CONFIG_PARAMETERS", "'commit.gpgsign=true'")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.hooksPath")
        .env("GIT_CONFIG_VALUE_0", &hooks);
    isolate_git_environment(&mut command);
    let output = command
        .arg("-C")
        .arg(&repo)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "isolated fixture",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !marker.exists(),
        "fixture invoked an inherited hook/signing program"
    );
    let mut query = std::process::Command::new("git");
    query
        .env("GIT_CONFIG_GLOBAL", &hostile)
        .env("GIT_CONFIG_SYSTEM", &hostile);
    isolate_git_environment(&mut query);
    let queried = query
        .arg("-C")
        .arg(&repo)
        .args(["config", "--get", "safety.inherited"])
        .output()
        .unwrap();
    assert_eq!(queried.status.code(), Some(1));
    assert!(queried.stdout.is_empty());
    assert_eq!(
        String::from_utf8(
            git_command(&repo, &["config", "--local", "--get", "core.hooksPath"])
                .output()
                .unwrap()
                .stdout
        )
        .unwrap()
        .trim(),
        "/dev/null"
    );
}

#[test]
fn relative_identity_paths_resolve_from_the_probed_cwd() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("repo");
    let git_dir = root.join(".git");
    std::fs::create_dir_all(&git_dir).expect("create git dir");

    let cwd = root.to_string_lossy().into_owned();
    assert_eq!(
        canonical_identity_path_from(&cwd, ".git").unwrap(),
        canonical_identity_path(&git_dir).unwrap()
    );
    assert_eq!(
        canonical_identity_path_from(&cwd, git_dir.to_string_lossy().as_ref()).unwrap(),
        canonical_identity_path(&git_dir).unwrap()
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
        context
            .changes
            .iter()
            .any(|change| { change.path == "tracked.txt" && change.worktree_status == Some('M') })
    );
}

#[test]
fn detached_head_does_not_invent_a_branch() {
    let parsed =
        parse_porcelain_v2(b"# branch.oid deadbeef\0# branch.head (detached)\0").expect("parse");
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
