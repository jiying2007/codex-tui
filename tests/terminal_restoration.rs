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
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts/testing/terminal_restoration.py");
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
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("PTY deadline: {}", std::fs::read_to_string(&log_path).unwrap_or_default());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "PTY failure: {}", std::fs::read_to_string(&log_path).unwrap_or_default());
    for (scenario, exit_code, count) in [("normal", 0, 6), ("failed-save", 1, 7)] {
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
