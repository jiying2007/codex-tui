//! Path identity and resource boundaries are independent from presentation.
use super::*;

#[test]
fn rev_parse_consumes_one_terminator_not_identity_whitespace() {
    for path in ["/repo", "/repo  ", "/repo\nchild", "/repo\n", "/repo\r"] {
        assert_eq!(rev_parse_path(&format!("{path}\n")).unwrap(), path);
    }
    for invalid in ["", "\n", "/repo", "/repo\0\n"] {
        assert!(rev_parse_path(invalid).is_err());
    }
}

#[test]
fn canonical_identity_must_exist_and_remain_lossless() {
    let temp = tempfile::tempdir().unwrap();
    assert!(canonical_identity_path(temp.path().join("absent")).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let invalid = temp.path().join(std::ffi::OsString::from_vec(b"bad-\xff".to_vec()));
        std::fs::create_dir(&invalid).unwrap();
        assert!(canonical_identity_path(invalid).is_err());
    }
}

#[tokio::test]
async fn capture_accepts_exact_limit_and_refuses_one_more_byte() {
    for (size, pass) in [(0, true), (64, true), (65, false), (8192, false)] {
        let bytes = vec![b'x'; size];
        let result = read_probe_limited(bytes.as_slice(), 64).await;
        assert_eq!(result.is_ok(), pass);
        if let Ok(value) = result { assert_eq!(value, bytes); }
    }
    let future = read_probe_limited(tokio::io::empty(), MAX_GIT_PROBE_BYTES);
    assert!(std::mem::size_of_val(&future) <= 4096);
}

#[test]
fn status_refuses_truncation_lossy_paths_unknown_records_and_overflow() {
    for invalid in [b"? unfinished".as_slice(), b"? \xff\0", b"? \0", b"x unknown\0",
        b"2 R. N... 100644 100644 100644 a b R100 new\0",
        b"2 R. N... 100644 100644 100644 a b R100 new\0\xff\0"] {
        assert!(parse_porcelain_v2(invalid).is_err(), "{invalid:?}");
    }
    let at_limit = b"? x\0".repeat(MAX_GIT_STATUS_ENTRIES);
    assert_eq!(parse_porcelain_v2(&at_limit).unwrap().changes.len(), MAX_GIT_STATUS_ENTRIES);
    assert!(parse_porcelain_v2(&b"? x\0".repeat(MAX_GIT_STATUS_ENTRIES + 1)).is_err());
    assert!(parse_porcelain_v2(&vec![0; MAX_GIT_PROBE_BYTES + 1]).is_err());
    assert!(parse_porcelain_v2(b"").unwrap().changes.is_empty());
    assert!(parse_porcelain_v2(b"# future metadata\0").unwrap().changes.is_empty());
    assert_eq!(parse_porcelain_v2(b"? new\nline  \0").unwrap().changes[0].path, "new\nline  ");
}

#[cfg(unix)]
#[tokio::test]
async fn real_git_preserves_unusual_roots_and_linked_worktree_identity() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["ordinary", "trailing  ", "embedded\nline", "last\n", "last\r"] {
        let root = temp.path().join(name);
        std::fs::create_dir(&root).unwrap();
        tests::git(&root, &["init", "-q"]);
        tests::git(&root, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--allow-empty", "-qm", "fixture"]);
        std::fs::write(root.join("changed\nfile  "), "fixture").unwrap();
        let context = probe_context(ThreadId::new(name), root.to_str().unwrap().into()).await.unwrap();
        assert_eq!(context.worktree.as_ref().unwrap().canonical_path, canonical_identity_path(&root).unwrap());
        assert_eq!(context.changes[0].path, "changed\nfile  ");
        let linked = temp.path().join(format!("linked-{name}"));
        tests::git(&root, &["worktree", "add", "--detach", linked.to_str().unwrap()]);
        let other = probe_context(ThreadId::new("linked"), linked.to_str().unwrap().into()).await.unwrap();
        assert_eq!(context.repo, other.repo);
        assert_eq!(other.worktree.unwrap().canonical_path, canonical_identity_path(&linked).unwrap());
    }
}
