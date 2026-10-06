use codex_tui::release::validate_evidence;
use serde_json::Value;
use std::{fs, path::PathBuf};

const COMMIT: &str = "0000000000000000000000000000000000000000";

fn example_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/release/stable-evidence.example.json")
}

fn write_mutated(value: &Value) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    fs::write(
        temp.path().join("evidence.json"),
        serde_json::to_vec_pretty(value).expect("evidence json"),
    )
    .expect("write evidence");
    temp
}

#[test]
fn documented_v5_example_is_verifier_valid() {
    validate_evidence(&example_path(), env!("CARGO_PKG_VERSION"), COMMIT)
        .expect("documented v5 evidence example must remain verifier-valid");
}

#[test]
fn terminal_receipt_digest_is_exact() {
    let mut value: Value = serde_json::from_slice(&fs::read(example_path()).expect("read example"))
        .expect("decode example");
    value["terminalRestoration"]["linux"]["receiptSha256"] = Value::String("c".repeat(63));
    let temp = write_mutated(&value);
    let error = validate_evidence(
        &temp.path().join("evidence.json"),
        env!("CARGO_PKG_VERSION"),
        COMMIT,
    )
    .expect_err("terminal receipt digest must be exact");
    assert!(format!("{error:#}").contains("terminal receipt SHA-256"));
}

#[test]
fn performance_report_digest_is_exact() {
    let mut value: Value = serde_json::from_slice(&fs::read(example_path()).expect("read example"))
        .expect("decode example");
    value["performance"]["reportSha256"] = Value::String("d".repeat(63));
    let temp = write_mutated(&value);
    let error = validate_evidence(
        &temp.path().join("evidence.json"),
        env!("CARGO_PKG_VERSION"),
        COMMIT,
    )
    .expect_err("performance report digest must be exact");
    assert!(format!("{error:#}").contains("performance report SHA-256"));
}
