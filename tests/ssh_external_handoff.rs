//! Isolated subprocesses: never mutate the parallel test runner's environment.
#![cfg(unix)]
#[path = "../src/runtime_external.rs"]
mod runtime_external;

use std::process::Command;

#[test]
fn ssh_handoff_child_fixture() {
    let Some(root) = std::env::var_os("CODEX_TUI_SSH_TEST_ROOT") else {
        return;
    };
    let root = std::path::Path::new(&root);
    let url = "https://example.invalid/review/1";
    for result in [
        runtime_external::open_external_url(url),
        runtime_external::open_external_editor(
            root.to_str().unwrap(),
            root.to_str().unwrap(),
            "a file.rs",
        ),
    ] {
        let message = format!("{:#}", result.expect_err("SSH must not launch a default"));
        assert!(message.contains("SSH: no process started"), "{message}");
        assert!(message.contains("TUI host"));
        assert!(!message.contains('\n'));
    }
    assert!(!root.join("called").exists());
    let invalid = runtime_external::open_external_url("https://user:secret@example.invalid")
        .expect_err("unsafe target must be rejected before handoff");
    assert!(!format!("{invalid:#}").contains("user:secret"));
}

#[test]
fn ssh_defaults_are_blocked_for_both_standard_session_markers() {
    use std::os::unix::fs::PermissionsExt;
    for marker in ["SSH_TTY", "SSH_CONNECTION"] {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(root.path().join("a file.rs"), "fixture").unwrap();
        for name in ["code", "xdg-open", "open"] {
            let script = bin.join(name);
            std::fs::write(
                &script,
                "#!/bin/sh\nprintf called > \"$CODEX_TUI_SSH_TEST_ROOT/called\"\n",
            )
            .unwrap();
            std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "ssh_handoff_child_fixture"])
            .env("CODEX_TUI_SSH_TEST_ROOT", root.path())
            .env_remove("CODEX_TUI_EDITOR")
            .env_remove("CODEX_TUI_BROWSER")
            .env_remove("SSH_TTY")
            .env_remove("SSH_CONNECTION")
            .env(marker, "fixture")
            .env("PATH", bin)
            .output()
            .unwrap();
        assert!(output.status.success(), "{marker}: {output:?}");
        assert!(!root.path().join("called").exists());
    }
}
