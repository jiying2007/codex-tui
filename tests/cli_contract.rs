//! Process-level tests: usage validation must precede all state/backend/TTY work.
use std::process::{Command, Output};
use tempfile::TempDir;

fn invoke(args: &[&str], home: &TempDir) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codex-tui"))
        .args(args)
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("APPDATA", home.path().join("AppData/Roaming"))
        .env("LOCALAPPDATA", home.path().join("AppData/Local"))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("CODEX_HOME", home.path().join("codex"))
        .env("PATH", home.path())
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run CLI")
}

#[test]
fn help_is_side_effect_free_without_a_tty_or_backend() {
    for args in [
        vec!["--help"],
        vec!["-h"],
        vec!["help"],
        vec!["doctor", "--help"],
        vec!["doctor", "codex", "--help"],
        vec!["doctor", "compat", "--help"],
        vec!["headless", "--help"],
        vec!["thread", "list", "--help"],
        vec!["release", "verify", "--help"],
        vec!["soak", "--help"],
    ] {
        let home = TempDir::new().unwrap();
        let output = invoke(&args, &home);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
        assert_eq!(
            std::fs::read_dir(home.path()).unwrap().count(),
            0,
            "{args:?} wrote state"
        );
    }
}

#[test]
fn invalid_usage_is_side_effect_free_and_exits_two() {
    for args in [
        vec!["--no-such-option"],
        vec!["doctor", "no-such-scope"],
        vec!["doctor", "store", "--bogus"],
        vec!["doctor", "store", "extra"],
        vec!["doctor", "codex", "--target", "--help"],
        vec!["--target"],
        vec!["--target", "a", "--target", "b"],
        vec!["--fake", "--target", "a"],
        vec!["headless", "threads", "--json", "--json"],
        vec!["doctor", "bundle", "--output", "a", "--output", "b"],
        vec!["release", "benchmark", "--iterations", "0"],
        vec!["soak", "--cycles", "bad"],
        vec!["doctor", "bad", "--help"],
        vec!["version", "extra"],
    ] {
        let home = TempDir::new().unwrap();
        let output = invoke(&args, &home);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert_eq!(
            std::fs::read_dir(home.path()).unwrap().count(),
            0,
            "{args:?} wrote state"
        );
    }
}

#[test]
fn requested_missing_backend_is_not_reported_as_success() {
    let home = TempDir::new().unwrap();
    // Known Folder API verifies directories before returning a Windows path.
    // Only this explicit Doctor probe gets an initialized synthetic profile;
    // help/invalid-usage tests above still start with a completely empty home.
    #[cfg(windows)]
    for directory in ["AppData/Roaming", "AppData/Local"] {
        std::fs::create_dir_all(home.path().join(directory)).unwrap();
    }
    let output = invoke(&["doctor", "codex"], &home);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("connected: false"),
        "{output:?}"
    );
}
