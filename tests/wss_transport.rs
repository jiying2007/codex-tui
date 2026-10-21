//! Exercise production TLS features through the actual binary, not a dev TLS provider.
#![cfg(target_os = "linux")]

use std::{path::Path, process::Command};

#[test]
fn wss_fixture_preserves_failed_process_diagnostics() {
    let result = Command::new("python3")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts/testing/test_wss_transport_regression.py"),
        )
        .output()
        .expect("run WSS diagnostic regressions");
    assert!(
        result.status.success(),
        "WSS diagnostics failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn wss_uses_native_trust_and_reports_certificate_and_connection_errors() {
    let output = tempfile::tempdir().unwrap();
    let result = Command::new("python3")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts/testing/wss_transport_regression.py"),
        )
        .arg(env!("CARGO_BIN_EXE_codex-tui"))
        .arg(output.path())
        .output()
        .expect("run Python/OpenSSL TLS fixture");
    assert!(
        result.status.success(),
        "WSS smoke failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.path().join("receipt.json")).unwrap())
            .unwrap();
    assert_eq!(receipt["status"], "passed");
    assert_eq!(receipt["cases"].as_array().unwrap().len(), 4);
    assert_eq!(receipt["realCodexQualification"], false);
}
