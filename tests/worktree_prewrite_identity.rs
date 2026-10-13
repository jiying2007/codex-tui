#![cfg(unix)]

use codex_tui::domain::LocalRepoIdentity;
use codex_tui::operation::{OperationPlan, OperationState, now_unix_ms};
use codex_tui::sqlite_store::SqliteStore;
use codex_tui::worktree::{MutationEvent, MutationRequest, WorktreeMutationHandle};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// This wrapper forwards real Git output without decoding NUL porcelain. It
// changes the selected cwd only AFTER the last successful admission probe.
// Git 2.25's rejected -z probe must not trigger the switch before its fallback.
const GIT_WRAPPER: &str = r#"#!/bin/sh
set -eu
if [ "${1-}" = '-C' ] && [ "${2-}" = "$CODEX_TUI_AUDIT_SELECTED" ]; then
    if [ "${3-}" = 'worktree' ] && [ "${4-}" = 'list' ]; then
        set +e
        "$CODEX_TUI_AUDIT_GIT" "$@"
        status=$?
        set -e
        if [ "$status" -eq 0 ] && [ ! -e "$CODEX_TUI_AUDIT_ROOT/admitted" ]; then
            if [ "$CODEX_TUI_AUDIT_MODE" = 'retarget' ]; then
                rm "$CODEX_TUI_AUDIT_SELECTED"
                ln -s "$CODEX_TUI_AUDIT_ROOT/foreign" "$CODEX_TUI_AUDIT_SELECTED"
            fi
            : > "$CODEX_TUI_AUDIT_ROOT/admitted"
        fi
        exit "$status"
    fi
    if [ "${3-}" = 'branch' ] && [ "${4-}" = '-d' ]; then
        printf 'delete\n' >> "$CODEX_TUI_AUDIT_ROOT/writes"
    fi
fi
exec "$CODEX_TUI_AUDIT_GIT" "$@"
"#;

fn git(real_git: &Path, cwd: &Path, args: &[&str]) {
    let output = Command::new(real_git)
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

fn has_victim(real_git: &Path, cwd: &Path) -> bool {
    let status = Command::new(real_git)
        .arg("-C")
        .arg(cwd)
        .args(["show-ref", "--verify", "--quiet", "refs/heads/victim"])
        .status()
        .unwrap();
    assert!(matches!(status.code(), Some(0 | 1)), "{status}");
    status.success()
}

fn run_case(mode: &str) {
    let temp = tempfile::Builder::new()
        .prefix("codex-tui-worktree audit-")
        .tempdir()
        .unwrap();
    let root = temp.path().canonicalize().unwrap();
    let found = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    assert!(found.status.success());
    let real_git = PathBuf::from(String::from_utf8(found.stdout).unwrap().trim_end());
    assert!(real_git.is_absolute() && real_git.is_file());
    for name in ["original", "foreign"] {
        let repo = root.join(name);
        fs::create_dir(&repo).unwrap();
        git(&real_git, &repo, &["init"]);
        git(&real_git, &repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        git(
            &real_git,
            &repo,
            &[
                "-c",
                "user.name=Worktree Audit",
                "-c",
                "user.email=audit@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "--allow-empty",
                "-m",
                "base",
            ],
        );
        git(&real_git, &repo, &["branch", "victim"]);
    }
    let selected = root.join("selected");
    symlink(root.join("original"), &selected).unwrap();
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    let wrapper = bin.join("git");
    fs::write(&wrapper, GIT_WRAPPER).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));

    // Isolate PATH and fixture variables in a child test process, never in the
    // shared test runner. Parallel tests cannot retarget one another's Git.
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "coordinator_identity_child", "--nocapture"])
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("CODEX_TUI_AUDIT_ROOT", &root)
        .env("CODEX_TUI_AUDIT_GIT", &real_git)
        .env("CODEX_TUI_AUDIT_SELECTED", &selected)
        .env("CODEX_TUI_AUDIT_MODE", mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!("coordinator subprocess deadline exceeded: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{mode} coordinator failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn coordinator_refuses_retarget_after_successful_admission_before_any_write() {
    run_case("retarget");
}

#[test]
fn coordinator_unchanged_identity_positive_control_performs_exactly_one_write() {
    run_case("unchanged");
}

#[tokio::test]
async fn coordinator_identity_child() {
    // Subprocess fixture, deliberately inert during the ordinary full test run.
    let Some(root) = std::env::var_os("CODEX_TUI_AUDIT_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let real_git = PathBuf::from(std::env::var_os("CODEX_TUI_AUDIT_GIT").unwrap());
    let original = root.join("original");
    let foreign = root.join("foreign");
    let plan = OperationPlan::delete_branch(
        LocalRepoIdentity {
            git_common_dir: original.join(".git").to_str().unwrap().into(),
            primary_root: original.to_str().unwrap().into(),
        },
        root.join("selected").to_str().unwrap().into(),
        "victim".into(),
        now_unix_ms(),
    );
    let operation_id = plan.operation_id.clone();
    let store = SqliteStore::at(root.join("state"));
    let mut coordinator = WorktreeMutationHandle::start(store.clone());
    coordinator
        .execute(MutationRequest {
            plan,
            active_scopes: vec![],
            admitted_at: Instant::now(),
        })
        .unwrap();
    let receipt = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match coordinator.try_recv() {
                Some(MutationEvent::Receipt(receipt)) => break *receipt,
                Some(MutationEvent::Notice(notice)) => panic!("{notice}"),
                _ => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
    })
    .await
    .expect("coordinator must produce a bounded terminal receipt");
    assert!(root.join("admitted").is_file(), "admission was not exercised");
    assert!(has_victim(&real_git, &foreign), "foreign branch was modified");
    if std::env::var("CODEX_TUI_AUDIT_MODE").unwrap() == "retarget" {
        assert!(has_victim(&real_git, &original));
        assert!(!root.join("writes").exists(), "Git mutation was invoked");
        assert_eq!(receipt.state, OperationState::Failed);
        assert!(
            receipt
                .failure
                .as_deref()
                .unwrap()
                .contains("repository identity changed after admission")
        );
    } else {
        assert!(!has_victim(&real_git, &original));
        assert_eq!(fs::read_to_string(root.join("writes")).unwrap(), "delete\n");
        assert_eq!(receipt.state, OperationState::Succeeded);
    }
    assert_eq!(store.operation_receipt(&operation_id).unwrap(), Some(receipt));
}
