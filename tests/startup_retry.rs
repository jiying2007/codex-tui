//! Linux controlling-PTY fixture; no real Codex binary, account or remote endpoint.
#![cfg(target_os = "linux")]
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn startup_retry_is_explicit_single_flight_and_not_established_reconnect() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let output = root.join("startup-retry");
    let log_path = root.join("harness.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut child = Command::new("python3")
        .arg("-O")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts/testing/startup_retry_regression.py"),
        )
        .arg(env!("CARGO_BIN_EXE_codex-tui"))
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("startup retry harness exceeded its deadline");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "startup retry: {}\n{}",
        std::fs::read_to_string(&log_path).unwrap_or_default(),
        std::fs::read_to_string(output.join("screen.txt")).unwrap_or_default()
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["status"], "passed");
    assert_eq!(receipt["starts"], 2);
    assert_eq!(receipt["writeRpcCount"], 0);
    for name in [
        "singleFlight",
        "modalFocusPreserved",
        "noAutomaticRetry",
        "noEstablishedReconnect",
        "terminalRestored",
    ] {
        assert_eq!(receipt[name], true, "{name}");
    }
    assert_eq!(receipt["realCodexQualification"], false);
    assert_eq!(receipt["realSshQualification"], false);
    println!("{receipt}");
}
