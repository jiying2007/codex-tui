//! Lossless path identity, bounded capture, and immutable inventory ownership.
use super::*;

#[test]
fn rev_parse_removes_only_the_protocol_terminator() {
    for path in [
        "/repo",
        "/repo  ",
        "/new\nline",
        "/repo\n",
        "/repo\t",
        "/仓库",
    ] {
        assert_eq!(rev_parse_path(&format!("{path}\n")).unwrap(), path);
    }
    #[cfg(unix)]
    assert_eq!(rev_parse_path("/repo\r\n").unwrap(), "/repo\r");
    #[cfg(windows)]
    assert_eq!(rev_parse_path("C:/repo\r\n").unwrap(), "C:/repo");
    for invalid in ["", "\n", "unterminated", "/repo\0\n"] {
        assert!(rev_parse_path(invalid).is_err());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn real_special_paths_and_linked_worktrees_keep_exact_identity() {
    let temp = tempfile::tempdir().unwrap();
    for (round, name) in ["repo  ", "new\nline", "tail\n", "tab\t", "仓库"]
        .iter()
        .enumerate()
    {
        let root = temp.path().join(name);
        std::fs::create_dir_all(root.join("nested")).unwrap();
        git(&root, &["init", "-q"]);
        git(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "fixture",
            ],
        );
        let context = probe_context(
            ThreadId::new("original"),
            root.join("nested").to_str().unwrap().into(),
        )
        .await
        .unwrap();
        assert_eq!(
            context.worktree.as_ref().unwrap().canonical_path,
            root.canonicalize().unwrap().to_str().unwrap()
        );
        assert_eq!(
            context.repo.as_ref().unwrap().git_common_dir,
            root.join(".git").canonicalize().unwrap().to_str().unwrap()
        );
        let linked = temp.path().join(format!("linked-{round}  \n"));
        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "fixture-linked",
                linked.to_str().unwrap(),
            ],
        );
        let other = probe_context(ThreadId::new("linked"), linked.to_str().unwrap().into())
            .await
            .unwrap();
        assert_eq!(context.repo, other.repo);
        assert_eq!(
            other.worktree.as_ref().unwrap().canonical_path,
            linked.canonicalize().unwrap().to_str().unwrap()
        );
    }
}

#[test]
fn identity_resolution_cannot_succeed_with_an_unverified_fallback_path() {
    let temp = tempfile::tempdir().unwrap();
    assert!(canonical_identity_path(temp.path().join("does-not-exist")).is_err());
}

#[test]
fn status_preserves_whitespace_and_rejects_lossy_or_incomplete_identity() {
    let parsed = parse_porcelain_v2(b"? file  \0? new\nline\0? tab\t\0").unwrap();
    assert_eq!(parsed.changes[0].path, "file  ");
    assert_eq!(parsed.changes[1].path, "new\nline");
    assert_eq!(parsed.changes[2].path, "tab\t");
    for invalid in [
        b"? invalid\xff\0".as_slice(),
        b"? partial",
        b"? \0",
        b"? ../escape\0",
        b"? /absolute\0",
        b"unknown\0",
    ] {
        assert!(
            parse_porcelain_v2(invalid).is_err(),
            "accepted invalid inventory {invalid:?}"
        );
    }
    let rename = b"2 R. N... 100644 100644 100644 a b R100 new\0";
    assert!(parse_porcelain_v2(rename).is_err());
}

#[test]
fn status_record_and_byte_budgets_are_independent() {
    let boundary = b"? a\0".repeat(MAX_GIT_STATUS_RECORDS);
    assert_eq!(
        parse_porcelain_v2(&boundary).unwrap().changes.len(),
        MAX_GIT_STATUS_RECORDS
    );
    let mut excess = boundary;
    excess.extend_from_slice(b"? b\0");
    assert!(
        parse_porcelain_v2(&excess)
            .unwrap_err()
            .to_string()
            .contains("record budget")
    );
    let excess_bytes = vec![0; MAX_GIT_PROBE_BYTES + 1];
    assert!(
        parse_porcelain_v2(&excess_bytes)
            .unwrap_err()
            .to_string()
            .contains("byte budget")
    );
}

#[tokio::test]
async fn capture_records_exact_boundary_without_confusing_eof_and_truncation() {
    for bytes in [0, 1024, 1025, 64 * 1024] {
        let data = vec![b'x'; bytes];
        let (captured, truncated) = read_capped(data.as_slice(), 1024).await.unwrap();
        assert_eq!(captured.len(), bytes.min(1024));
        assert_eq!(truncated, bytes > 1024);
    }
}

#[test]
fn git_capture_futures_do_not_embed_chunk_buffers() {
    let read = read_capped(tokio::io::empty(), MAX_GIT_PROBE_BYTES);
    let capture = capture_git(".", ["unused-unpolled"], MAX_GIT_PROBE_BYTES);
    let strict = run_git_raw(".", ["unused-unpolled"], MAX_GIT_PROBE_BYTES);
    for bytes in [
        std::mem::size_of_val(&read),
        std::mem::size_of_val(&capture),
        std::mem::size_of_val(&strict),
    ] {
        assert!(bytes <= 4096, "Git I/O future embeds {bytes} bytes");
    }
}

#[test]
fn cloned_contexts_share_one_immutable_file_inventory() {
    let mut context = GitContext::pending(ThreadId::new("source"), "/repo");
    context.changes = parse_porcelain_v2(&b"? some-file\0".repeat(2000))
        .unwrap()
        .changes
        .into();
    let copies: Vec<_> = (0..32).map(|_| context.clone()).collect();
    assert!(
        copies
            .iter()
            .all(|c| Arc::ptr_eq(&c.changes, &context.changes))
    );
    assert_eq!(Arc::strong_count(&context.changes), 33);
    let weak = Arc::downgrade(&context.changes);
    drop(copies);
    drop(context);
    assert!(
        weak.upgrade().is_none(),
        "inventory retained after all owners dropped"
    );
}

#[cfg(unix)]
fn git(root: &Path, args: &[&str]) {
    let output = super::tests::git_command(root, args).output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    if args.first() == Some(&"init") {
        super::tests::protect_fixture_repository(root);
    }
}
