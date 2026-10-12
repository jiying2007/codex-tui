//! Linux actual-binary regressions; isolated mock App Server, never real qualification.
#![cfg(target_os = "linux")]
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[test]
fn fragmented_stdio_eof_and_cursor_cycles_are_truthful_and_bounded() {
    let temporary = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("CODEX_TUI_PTY_EVIDENCE_DIR");
    let root = retained
        .as_deref()
        .map(Path::new)
        .unwrap_or_else(|| temporary.path());
    std::fs::create_dir_all(root).unwrap();
    let output = root.join("app-server");
    let log_path = root.join("app-server-harness.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut child = Command::new("python3")
        .arg("-O")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts/testing/app_server_transport_regression.py"),
        )
        .arg(env!("CARGO_BIN_EXE_codex-tui"))
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "transport harness timeout: {}",
                std::fs::read_to_string(&log_path).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "transport regression: {}",
        std::fs::read_to_string(&log_path).unwrap_or_default()
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["status"], "passed");
    assert_eq!(receipt["checks"].as_array().unwrap().len(), 7);
    assert_eq!(receipt["realCodexQualification"], false);
    assert_eq!(receipt["realSshQualification"], false);
    println!("{receipt}");
}
