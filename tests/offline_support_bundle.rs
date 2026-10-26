//! Real CLI/local metadata, with external executables and targets set as traps.
#![cfg(unix)]
use serde_json::Value;
use std::{
    fs,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    process::{Command, Output},
};
use tempfile::TempDir;

fn invoke(home: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codex-tui"))
        .args(args)
        .current_dir(home.path())
        .env_clear()
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("CODEX_HOME", home.path().join("codex"))
        .env("PATH", home.path().join("bin"))
        .env("TRAP_MARKER", home.path().join("external-called"))
        .env("OPENAI_API_KEY", "offline-secret-must-not-leak")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}
fn traps(home: &TempDir) -> TcpListener {
    let bin = home.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    for executable in ["codex", "gh", "glab", "git"] {
        let path = bin.join(executable);
        fs::write(
            &path,
            "#!/bin/sh\nprintf called >> \"$TRAP_MARKER\"\nexit 97\n",
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let config = home.path().join("config/codex-tui");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("config.toml"), format!(
        "[app_server]\nactive=\"trap\"\n[app_server.targets.trap]\ntransport=\"websocket\"\nurl=\"ws://{}\"\n", listener.local_addr().unwrap())).unwrap();
    listener
}
fn no_external_calls(home: &TempDir, listener: &TcpListener) {
    assert!(
        !home.path().join("external-called").exists(),
        "external executable was invoked"
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
#[test]
fn offline_bundle_never_starts_external_processes_or_connects_to_targets() {
    let home = tempfile::tempdir().unwrap();
    let listener = traps(&home);
    let output = invoke(
        &home,
        &["doctor", "bundle", "--offline", "--output", "bundle"],
    );
    assert!(output.status.success(), "{output:?}");
    no_external_calls(&home, &listener);
    let raw = fs::read(home.path().join("bundle/snapshot.json")).unwrap();
    let snapshot: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(snapshot["collectionMode"], "offline");
    assert_eq!(snapshot["realCodexQualification"], false);
    assert_eq!(snapshot["compat"]["readiness"], "blocked");
    assert_eq!(
        snapshot["build"]["sourceSha"],
        codex_tui::compat::source_sha()
    );
    assert_eq!(
        snapshot["build"]["productVersion"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(snapshot["store"]["available"], true);
    assert_eq!(snapshot["store"]["integrity"], "ok");
    assert_eq!(snapshot["forge"]["authenticated"], Value::Null);
    assert_eq!(snapshot["git"]["errorCode"], "not-probed-offline");
    for id in ["codex-app-server", "git", "terminal-pty", "gh", "glab"] {
        let component = snapshot["compat"]["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["id"] == id)
            .unwrap();
        assert_eq!(component["state"], "unavailable");
        assert_eq!(component["facts"]["probeStatus"], "not-probed-offline");
    }
    let text = String::from_utf8(raw.clone()).unwrap();
    assert!(!text.contains("offline-secret-must-not-leak"));
    assert!(!text.contains(home.path().to_str().unwrap()));
    let manifest: Value =
        serde_json::from_slice(&fs::read(home.path().join("bundle/manifest.json")).unwrap())
            .unwrap();
    for entry in manifest["files"].as_array().unwrap() {
        let bytes = fs::read(
            home.path()
                .join("bundle")
                .join(entry["file"].as_str().unwrap()),
        )
        .unwrap();
        let hash = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        assert_eq!(entry["checksum"], format!("{hash:016x}"));
        assert_eq!(entry["bytes"], bytes.len());
    }
    let duplicate = invoke(
        &home,
        &["doctor", "bundle", "--offline", "--output", "bundle"],
    );
    assert!(!duplicate.status.success());
    assert_eq!(
        fs::read(home.path().join("bundle/snapshot.json")).unwrap(),
        raw
    );
    no_external_calls(&home, &listener);
}
#[test]
fn offline_help_and_invalid_options_do_not_fall_back_to_live_probes() {
    for (args, code) in [
        (vec!["doctor", "bundle", "--offline", "--help"], 0),
        (vec!["doctor", "bundle", "--offline", "--offline"], 2),
        (vec!["doctor", "bundle", "--offline", "--target", "trap"], 2),
        (vec!["doctor", "bundle", "--offline", "--output"], 2),
        (
            vec!["doctor", "bundle", "--offline", "--output", "--offline"],
            2,
        ),
        (vec!["doctor", "bundle", "--offline", "--bogus"], 2),
    ] {
        let home = tempfile::tempdir().unwrap();
        let listener = traps(&home);
        let output = invoke(&home, &args);
        assert_eq!(output.status.code(), Some(code), "{args:?}: {output:?}");
        no_external_calls(&home, &listener);
        assert!(!home.path().join("data").exists());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn offline_bundle_reports_unavailable_local_store_without_claiming_health() {
    let home = tempfile::tempdir().unwrap();
    let listener = traps(&home);
    fs::write(home.path().join("data"), b"blocked directory").unwrap();
    let output = invoke(
        &home,
        &["doctor", "bundle", "--offline", "--output", "bundle"],
    );
    assert!(output.status.success(), "{output:?}");
    let snapshot: Value =
        serde_json::from_slice(&fs::read(home.path().join("bundle/snapshot.json")).unwrap())
            .unwrap();
    assert_eq!(snapshot["store"]["available"], false);
    assert_eq!(snapshot["store"]["schemaVersion"], Value::Null);
    assert_eq!(snapshot["store"]["integrity"], Value::Null);
    assert!(
        snapshot["compat"]["requiredFailures"]
            .as_array()
            .unwrap()
            .contains(&Value::from("sqlite"))
    );
    no_external_calls(&home, &listener);
}
