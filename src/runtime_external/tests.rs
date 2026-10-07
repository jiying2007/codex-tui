//! Linux real-process tests; do not change process-wide editor/browser variables.
use super::*;
use std::{
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

#[test]
fn child_fixture() {
    let Some(root) = std::env::var_os("CODEX_TUI_EXTERNAL_TEST_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let expected = if std::env::var("CODEX_TUI_EXTERNAL_TEST_ROLE").unwrap() == "editor" {
        open_external_editor(root.to_str().unwrap(), "a file.rs").unwrap();
        root.join("a file.rs").to_string_lossy().into_owned()
    } else {
        let url = "https://example.invalid/review?q=a%20b&x=1";
        open_external_url(url).unwrap();
        url.to_string()
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    let pid_file = root.join("pid");
    while !pid_file.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let pid: i32 = std::fs::read_to_string(pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let process = std::path::PathBuf::from(format!("/proc/{pid}"));
    while process.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let leaked = process.exists();
    if leaked {
        let _ = nix::sys::wait::waitpid(
            nix::unistd::Pid::from_raw(pid),
            Some(nix::sys::wait::WaitPidFlag::WNOHANG),
        );
    }
    assert!(!leaked, "external handoff child {pid} was not reaped");
    assert_eq!(
        std::fs::read_to_string(root.join("arg")).unwrap().trim(),
        expected
    );
}

fn exercise(role: &str) {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let script = root.path().join("helper");
    std::fs::write(&script, "#!/bin/bash\nprintf '%s\\n' \"$BASHPID\" > \"$CODEX_TUI_EXTERNAL_TEST_ROOT/pid.tmp\"\nmv \"$CODEX_TUI_EXTERNAL_TEST_ROOT/pid.tmp\" \"$CODEX_TUI_EXTERNAL_TEST_ROOT/pid\"\nprintf '%s\\n' \"$1\" > \"$CODEX_TUI_EXTERNAL_TEST_ROOT/arg\"\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(root.path().join("a file.rs"), "test").unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "runtime_external::process_tests::child_fixture"])
        .env("CODEX_TUI_EXTERNAL_TEST_ROOT", root.path())
        .env("CODEX_TUI_EXTERNAL_TEST_ROLE", role)
        .env("CODEX_TUI_EDITOR", &script)
        .env("CODEX_TUI_BROWSER", &script)
        .env_remove("RUST_BACKTRACE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{role}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn editor_handoff_child_is_reaped() {
    exercise("editor");
}
#[test]
fn browser_handoff_child_is_reaped() {
    exercise("browser");
}
