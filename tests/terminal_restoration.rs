//! Maintained actual-binary Linux PTY regressions. No external account or approval.
#![cfg(target_os = "linux")]

use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn normal_and_failed_save_restore_the_actual_controlling_terminal() {
    let temporary = tempfile::tempdir().expect("isolated PTY evidence");
    let retained = std::env::var_os("CODEX_TUI_PTY_EVIDENCE_DIR");
    let output = retained
        .as_deref()
        .map(Path::new)
        .unwrap_or_else(|| temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/testing/terminal_restoration.py");
    let log_path = output.join("harness.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut child = Command::new("python3")
        .arg("-O")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_codex-tui"))
        .arg(output)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .expect("Python 3 is required for Linux PTY regression");
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "PTY deadline: {}",
                std::fs::read_to_string(&log_path).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "PTY failure: {}",
        std::fs::read_to_string(&log_path).unwrap_or_default()
    );
    for (scenario, exit_code, count) in [
        ("normal", 0, 6),
        ("failed-save", 1, 7),
        ("SIGTERM", 0, 6),
        ("SIGHUP", 0, 6),
        ("SIGINT", 0, 6),
        ("SIGTERM-failed-save", 1, 7),
        ("SIGHUP-failed-save", 1, 7),
        ("SIGINT-failed-save", 1, 7),
        ("raw-ctrl-c", 0, 6),
    ] {
        let receipt: serde_json::Value = serde_json::from_slice(
            &std::fs::read(output.join(scenario).join("receipt.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["schema"], "codex-tui/automated-pty-regression/v2");
        assert_eq!(receipt["status"], "passed");
        assert_eq!(receipt["exitCode"], exit_code);
        assert_eq!(receipt["manualQualification"], false);
        assert_eq!(receipt["realSshQualification"], false);
        let checks = receipt["checks"].as_object().unwrap();
        assert_eq!(checks.len(), count);
        assert!(checks.values().all(|value| value == true));
        println!("{receipt}");
    }
}

#[test]
fn drawer_signals_forge_reader_ownership_and_board_churn() {
    let temporary = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("CODEX_TUI_PTY_EVIDENCE_DIR");
    let root = retained
        .as_deref()
        .map(Path::new)
        .unwrap_or_else(|| temporary.path());
    std::fs::create_dir_all(root).unwrap();
    let output = root.join("runtime-faults");
    let log_path = root.join("runtime-faults.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut child = Command::new("python3")
        .arg("-O")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts/testing/runtime_fault_regression.py"),
        )
        .arg(env!("CARGO_BIN_EXE_codex-tui"))
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(150);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "runtime harness timeout: {}",
                std::fs::read_to_string(&log_path).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "runtime regression: {}",
        std::fs::read_to_string(&log_path).unwrap_or_default()
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["status"], "passed");
    let cases = receipt["checks"].as_array().unwrap();
    let expected = [
        "drawer-SIGTERM",
        "drawer-SIGHUP",
        "drawer-SIGINT",
        "forge-pipes",
        "board-10ms",
        "board-100ms",
    ];
    assert_eq!(cases.len(), expected.len());
    for (case, name) in cases.iter().zip(expected) {
        assert_eq!(case["mode"], name);
        assert_eq!(case["passed"], true);
    }
    assert_eq!(receipt["manualQualification"], false);
    assert_eq!(receipt["realSshQualification"], false);
    println!("{receipt}");
}

#[test]
fn exact_git_editor_targets_and_connection_readers_regressions() {
    let temporary = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("CODEX_TUI_PTY_EVIDENCE_DIR");
    let root = retained
        .as_deref()
        .map(Path::new)
        .unwrap_or_else(|| temporary.path());
    std::fs::create_dir_all(root).unwrap();
    let output = root.join("git-lifecycle");
    let log_path = root.join("git-lifecycle.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut child = Command::new("python3")
        .arg("-O")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts/testing/git_lifecycle_regression.py"),
        )
        .arg(env!("CARGO_BIN_EXE_codex-tui"))
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(180);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "Git lifecycle timeout: {}",
                std::fs::read_to_string(&log_path).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "Git lifecycle regression: {}",
        std::fs::read_to_string(&log_path).unwrap_or_default()
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["schema"], "codex-tui/git-lifecycle-regression/v1");
    assert_eq!(receipt["status"], "passed");
    assert_eq!(receipt["manualQualification"], false);
    assert_eq!(receipt["realSshQualification"], false);
    let expected = [
        "editor-control",
        "editor-spaces",
        "editor-newline",
        "stderr-control",
        "stderr-overflow",
        "status-control",
        "status-bytes",
        "status-records",
        "startup-held",
        "search-held",
        "search-eof-control",
    ];
    let cases = receipt["checks"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    for (case, name) in cases.iter().zip(expected) {
        assert_eq!(case["mode"], name);
        assert_eq!(case["passed"], true);
    }
    println!("{receipt}");
}

#[test]
fn cross_view_watch_effects_reach_the_actual_backend() {
    let temporary = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("CODEX_TUI_PTY_EVIDENCE_DIR");
    let root = retained
        .as_deref()
        .map(Path::new)
        .unwrap_or_else(|| temporary.path());
    std::fs::create_dir_all(root).unwrap();
    let output = root.join("watch-ownership");
    let log_path = root.join("watch-ownership.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let source = option_env!("CODEX_TUI_GIT_SHA")
        .or(option_env!("GITHUB_SHA"))
        .unwrap_or("unknown");
    let mut child = Command::new("python3")
        .arg("-O")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts/testing/watch_ownership_regression.py"),
        )
        .arg(env!("CARGO_BIN_EXE_codex-tui"))
        .arg(&output)
        .env("CODEX_TUI_PTY_SOURCE_SHA", source)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(180);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "watch harness timeout: {}",
                std::fs::read_to_string(&log_path).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "watch regression: {}",
        std::fs::read_to_string(&log_path).unwrap_or_default()
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["status"], "passed");
    assert_eq!(receipt["sourceSha"], source);
    assert_eq!(receipt["sourceIdentityVerified"], source != "unknown");
    let expected = [
        "idle-back",
        "search-cancel",
        "search-commit",
        "history-complete",
        "registry-complete",
        "deleted",
        "board-cancel",
        "board-commit",
        "prompt-success",
        "prompt-failure",
        "prompt-history-failure",
        "removal-provisional-keep",
        "removal-provisional-delete",
        "removal-provisional-archive",
        "removal-registered-keep",
        "removal-registered-delete",
        "removal-registered-archive",
        "return-provisional-live",
        "return-provisional-retired",
        "return-registered-live",
        "return-registered-retired",
    ];
    let checks = receipt["checks"].as_array().unwrap();
    assert_eq!(checks.len(), expected.len());
    for (check, name) in checks.iter().zip(expected) {
        assert_eq!(check["mode"], name);
        assert_eq!(check["passed"], true);
    }
    assert_eq!(receipt["realCodexQualification"], false);
    assert_eq!(receipt["realSshQualification"], false);
    println!("{receipt}");
}

#[test]
fn forge_confirmation_and_readback_remain_bound_to_the_reviewed_revision() {
    let temporary = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("CODEX_TUI_PTY_EVIDENCE_DIR");
    let root = retained
        .as_deref()
        .map(Path::new)
        .unwrap_or_else(|| temporary.path());
    std::fs::create_dir_all(root).unwrap();
    let output = root.join("forge-revision");
    let log_path = root.join("forge-revision.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let source = option_env!("CODEX_TUI_GIT_SHA")
        .or(option_env!("GITHUB_SHA"))
        .unwrap_or("unknown");
    let mut child = Command::new("python3")
        .arg("-O")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts/testing/forge_revision_regression.py"),
        )
        .arg(env!("CARGO_BIN_EXE_codex-tui"))
        .arg(&output)
        .env("CODEX_TUI_PTY_SOURCE_SHA", source)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(240);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "forge revision harness timeout: {}",
                std::fs::read_to_string(&log_path).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "forge revision regression: {}",
        std::fs::read_to_string(&log_path).unwrap_or_default()
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["status"], "passed");
    assert_eq!(receipt["sourceSha"], source);
    let expected = [
        "github-approve-control",
        "github-approve-missing",
        "github-approve-before",
        "github-approve-after",
        "github-approve-uncertain",
        "github-merge-control",
        "github-merge-missing",
        "github-merge-before",
        "github-merge-after",
        "github-merge-uncertain",
        "gitlab-approve-control",
        "gitlab-approve-missing",
        "gitlab-approve-before",
        "gitlab-approve-after",
        "gitlab-approve-uncertain",
        "gitlab-merge-control",
        "gitlab-merge-missing",
        "gitlab-merge-before",
        "gitlab-merge-after",
        "gitlab-merge-uncertain",
        "github-approve-wrong-review",
        "github-approve-stale-existing",
    ];
    let checks = receipt["checks"].as_array().unwrap();
    assert_eq!(checks.len(), expected.len());
    for (check, name) in checks.iter().zip(expected) {
        assert_eq!(check["mode"], name);
        assert_eq!(check["passed"], true);
    }
    assert_eq!(receipt["realForgeQualification"], false);
    assert_eq!(receipt["realCodexQualification"], false);
    assert_eq!(receipt["realSshQualification"], false);
    println!("{receipt}");
}

#[test]
fn subscription_release_survives_real_command_queue_pressure() {
    let temporary = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("CODEX_TUI_PTY_EVIDENCE_DIR");
    let root = retained
        .as_deref()
        .map(Path::new)
        .unwrap_or_else(|| temporary.path());
    std::fs::create_dir_all(root).unwrap();
    let output = root.join("watch-pressure");
    let log_path = root.join("watch-pressure.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let source = option_env!("CODEX_TUI_GIT_SHA")
        .or(option_env!("GITHUB_SHA"))
        .unwrap_or("unknown");
    let mut child = Command::new("python3")
        .arg("-O")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts/testing/watch_pressure_regression.py"),
        )
        .arg(env!("CARGO_BIN_EXE_codex-tui"))
        .arg(&output)
        .env("CODEX_TUI_PTY_SOURCE_SHA", source)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(180);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "watch harness timeout: {}",
                std::fs::read_to_string(&log_path).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "watch regression: {}",
        std::fs::read_to_string(&log_path).unwrap_or_default()
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["status"], "passed");
    assert_eq!(receipt["sourceSha"], source);
    let expected = [
        "healthy-close",
        "pressure-close-both",
        "pressure-close-queue",
        "pressure-keep",
        "pressure-reopen",
        "pressure-disconnect",
    ];
    let checks = receipt["checks"].as_array().unwrap();
    assert_eq!(checks.len(), expected.len());
    for (check, name) in checks.iter().zip(expected) {
        assert_eq!(check["mode"], name);
        assert_eq!(check["passed"], true);
    }
    assert_eq!(receipt["realCodexQualification"], false);
    assert_eq!(receipt["realSshQualification"], false);
    println!("{receipt}");
}

#[test]
fn evicted_returns_and_multi_session_workloads_preserve_ownership() {
    let temporary = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("CODEX_TUI_PTY_EVIDENCE_DIR");
    let root = retained
        .as_deref()
        .map(Path::new)
        .unwrap_or_else(|| temporary.path());
    std::fs::create_dir_all(root).unwrap();
    let output = root.join("watch-cache");
    let log_path = root.join("watch-cache.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let source = option_env!("CODEX_TUI_GIT_SHA")
        .or(option_env!("GITHUB_SHA"))
        .unwrap_or("unknown");
    let mut child = Command::new("python3")
        .arg("-O")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/testing/watch_cache_regression.py"),
        )
        .arg(env!("CARGO_BIN_EXE_codex-tui"))
        .arg(&output)
        .env("CODEX_TUI_PTY_SOURCE_SHA", source)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(240);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "watch harness timeout: {}",
                std::fs::read_to_string(&log_path).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "watch regression: {}",
        std::fs::read_to_string(&log_path).unwrap_or_default()
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["status"], "passed");
    assert_eq!(receipt["sourceSha"], source);
    let expected = [
        "registered-search-resident",
        "provisional-search-resident",
        "registered-search-evicted",
        "provisional-search-evicted",
        "registered-board-evicted",
        "provisional-board-evicted",
        "registered-search-retired",
        "provisional-search-retired",
        "provisional-search-read-error",
        "multi-session-workload",
    ];
    let checks = receipt["checks"].as_array().unwrap();
    assert_eq!(checks.len(), expected.len());
    for (check, name) in checks.iter().zip(expected) {
        assert_eq!(check["mode"], name);
        assert_eq!(check["passed"], true);
    }
    assert_eq!(receipt["realCodexQualification"], false);
    assert_eq!(receipt["realSshQualification"], false);
    println!("{receipt}");
}
