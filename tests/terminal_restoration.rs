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
