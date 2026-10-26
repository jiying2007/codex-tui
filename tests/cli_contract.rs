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
        vec!["headless", "threads", "--target"],
        vec!["headless", "work", "--target", "a", "--target", "b"],
        vec!["headless", "status", "--fake", "--target", "a"],
        vec!["headless", "board", "--fixture-10k", "--target", "a"],
        vec!["headless", "forge", "--target", "a"],
        vec!["worktree", "list", "--target", "a"],
        vec!["status", "--fake", "--fixture-10k"],
        vec!["doctor", "bundle", "--output", "a", "--output", "b"],
        vec!["release", "benchmark", "--iterations", "0"],
        vec!["soak", "--cycles", "bad"],
        vec!["soak", "--rows", "50001"],
        vec!["soak", "--cycles", "10001"],
        vec!["soak", "--duration-seconds", "3601"],
        vec!["release", "scale", "--rows", "100001"],
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
    // Empty PATH is not a no-exec boundary on Windows: Rust also searches
    // executable/system directories and the parent PATH. Select an absolute,
    // nonexistent fixture program instead of ever resolving the name `codex`.
    #[cfg(target_os = "windows")]
    let config_path = home
        .path()
        .join("AppData/Roaming/jiying2007/codex-tui/config/config.toml");
    #[cfg(target_os = "macos")]
    let config_path = home
        .path()
        .join("Library/Application Support/dev.jiying2007.codex-tui/config.toml");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let config_path = home.path().join("config/codex-tui/config.toml");
    let missing = home.path().join("missing-backend-fixture.exe");
    assert!(missing.is_absolute() && !missing.exists());
    let target_name = format!(
        "missing-fixture-{}",
        home.path().file_name().unwrap().to_string_lossy()
    );
    let mut config = codex_tui::store::AppConfig::default();
    config.app_server.active = target_name.clone();
    config.app_server.targets.insert(
        target_name.clone(),
        codex_tui::app_server_target::AppServerTargetConfig::Stdio {
            codex_bin: Some(missing.to_str().unwrap().into()),
        },
    );
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    let text = toml::to_string(&config).unwrap();
    std::fs::write(&config_path, &text).unwrap();
    let parsed: codex_tui::store::AppConfig = toml::from_str(&text).unwrap();
    let target = codex_tui::app_server_target::ResolvedAppServerTarget::resolve(
        &parsed.app_server,
        Some(&target_name),
    )
    .unwrap();
    assert!(
        matches!(target.endpoint, codex_tui::app_server_target::ResolvedAppServerEndpoint::Stdio { codex_bin: Some(path) } if path == missing.as_os_str())
    );
    let output = invoke(&["doctor", "codex", "--target", &target_name], &home);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("connected: false"),
        "{output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(&format!("app-server-target: {target_name}")),
        "{output:?}"
    );
    assert!(!missing.exists());
}

#[test]
fn archive_identity_sampling_respects_real_cli_exit_contract_without_state() {
    for (warmup, iterations, exit, qualified) in [("1", "1", 5, false), ("20", "200", 0, true)] {
        let home = TempDir::new().unwrap();
        let output = invoke(
            &[
                "release",
                "benchmark",
                "--warmup",
                warmup,
                "--iterations",
                iterations,
                "--source",
                "archive-identity-smoke",
                "--json",
            ],
            &home,
        );
        assert_eq!(output.status.code(), Some(exit), "{output:?}");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["schema"], "codex-tui/performance/v2");
        assert_eq!(report["sampleQualified"], qualified);
        assert_eq!(report["warmupIterations"], warmup.parse::<u64>().unwrap());
        assert_eq!(report["iterations"], iterations.parse::<u64>().unwrap());
        assert_eq!(report["sourceSha"], codex_tui::compat::source_sha());
        assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
    }
}

#[test]
fn version_is_side_effect_free_without_entering_the_interactive_runtime() {
    for args in [vec!["--version"], vec!["version"]] {
        let home = TempDir::new().unwrap();
        let output = invoke(&args, &home);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            concat!("codex-tui ", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
    }
}
