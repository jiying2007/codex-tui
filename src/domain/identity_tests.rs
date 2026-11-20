use super::*;

#[test]
fn cwd_locality_detects_host_local_and_foreign_paths() {
    let current = std::env::current_dir().expect("cwd");
    assert_eq!(
        classify_cwd(&current.to_string_lossy()),
        CwdLocality::LocalDirectory
    );

    if cfg!(windows) {
        assert_eq!(classify_cwd("/home/user/repo"), CwdLocality::ForeignUnix);
    } else {
        assert_eq!(
            classify_cwd(r"C:\\Users\\jun\\repo"),
            CwdLocality::ForeignWindows
        );
        assert_eq!(
            classify_cwd(r"/vsdata/repo/C:\\Users\\jun\\repo"),
            CwdLocality::ForeignWindows
        );
        assert_eq!(
            display_cwd(r"/vsdata/repo/C:\\Users\\jun\\repo"),
            r"C:\\Users\\jun\\repo"
        );
    }

    assert_eq!(classify_cwd("relative/repo"), CwdLocality::Relative);
    assert_eq!(classify_cwd(""), CwdLocality::Empty);
}

#[test]
fn cwd_locality_without_io_only_resolves_structural_cases() {
    assert_eq!(
        classify_cwd_without_io("relative/repo"),
        Some(CwdLocality::Relative)
    );
    assert_eq!(classify_cwd_without_io(""), Some(CwdLocality::Empty));

    if cfg!(windows) {
        assert_eq!(
            classify_cwd_without_io("/home/user/repo"),
            Some(CwdLocality::ForeignUnix)
        );
        assert_eq!(classify_cwd_without_io(r"C:\repo"), None);
    } else {
        assert_eq!(
            classify_cwd_without_io(r"C:\repo"),
            Some(CwdLocality::ForeignWindows)
        );
        assert_eq!(classify_cwd_without_io("/tmp/repo"), None);
    }
}

#[cfg(unix)]
#[test]
fn exact_cwd_identity_preserves_trailing_whitespace() {
    let root = tempfile::tempdir().unwrap();
    let plain = root.path().join("repo");
    let spaced = root.path().join("repo ");
    std::fs::create_dir(&plain).unwrap();
    std::fs::create_dir(&spaced).unwrap();
    let selected = spaced.to_str().unwrap();
    assert_eq!(classify_cwd(selected), CwdLocality::LocalDirectory);
    assert_eq!(display_cwd(selected), selected);
    assert_ne!(
        std::fs::canonicalize(selected).unwrap(),
        std::fs::canonicalize(plain).unwrap()
    );
}

#[test]
fn repository_identity_is_distinct_from_worktree_identity() {
    let repo = LocalRepoIdentity {
        git_common_dir: "/repo/.git".into(),
        primary_root: "/repo".into(),
    };
    let left = WorktreeIdentity {
        repo: repo.clone(),
        canonical_path: "/repo".into(),
        branch: Some("main".into()),
        managed_by_codex_tui: false,
    };
    let right = WorktreeIdentity {
        repo,
        canonical_path: "/tmp/worktrees/feature".into(),
        branch: Some("feature".into()),
        managed_by_codex_tui: true,
    };

    assert_eq!(left.repo, right.repo);
    assert_ne!(left.canonical_path, right.canonical_path);
}
