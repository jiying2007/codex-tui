//! Linux/macOS CLI smoke: typed GitLab failures survive Doctor rendering without secrets.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn doctor_forge_shows_safe_gitlab_http_reason_with_real_child_process() {
    let dir = tempfile::tempdir().expect("temporary GitLab fixture");
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let run_git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .expect("git");
        assert!(output.status.success(), "git failed: {output:?}");
    };
    run_git(&["init", "-q"]);
    run_git(&[
        "remote",
        "add",
        "origin",
        "git@gitlab.example.invalid:team/repo.git",
    ]);

    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let glab = bin.join("glab");
    std::fs::write(
        &glab,
        r#"#!/bin/sh
case "$1" in
  auth) exit 0 ;;
  version) printf 'glab 1.0.0 fixture\n'; exit 0 ;;
  api)
    case "$*" in
      *"/version"*) printf '{"version":"14.6.1","enterprise":false}\n'; exit 0 ;;
      *)
        printf 'glab: %s Forbidden token=secret-from-fake-glab\n' "$MOCK_HTTP_STATUS" >&2
        exit 1
        ;;
    esac
    ;;
  *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&glab).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&glab, permissions).unwrap();

    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![bin.clone()];
    paths.extend(std::env::split_paths(&inherited_path));
    let path = std::env::join_paths(paths).unwrap();

    for (status, reason) in [
        ("403", "access denied (HTTP 403)"),
        ("404", "resource not found (HTTP 404)"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_codex-tui"))
            .args(["doctor", "forge"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("MOCK_HTTP_STATUS", status)
            .env("HOME", dir.path())
            .output()
            .expect("doctor forge");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stdout.contains(reason),
            "missing safe HTTP status, exit={}: stdout={stdout}; stderr={stderr}",
            output.status
        );
        assert!(!stdout.contains("secret-from-fake-glab"));
        assert!(!stderr.contains("secret-from-fake-glab"));
    }

    // A glab diagnostic ending in ": 404" is not an HTTP 404 response.
    let ambiguous = Command::new(env!("CARGO_BIN_EXE_codex-tui"))
        .args(["doctor", "forge"])
        .current_dir(&repo)
        .env("PATH", &path)
        .env("MOCK_HTTP_STATUS", "network failed: 404")
        .env("HOME", dir.path())
        .output()
        .expect("doctor forge");
    let shown = String::from_utf8_lossy(&ambiguous.stdout);
    assert!(!shown.contains("HTTP 404"), "{shown}");
    assert!(!shown.contains("secret-from-fake-glab"));
    assert!(!String::from_utf8_lossy(&ambiguous.stderr).contains("secret-from-fake-glab"));
}
