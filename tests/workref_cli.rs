//! End-to-end G2/G3 CLI tests: no App Server, external API, Git write or TTY.
use std::process::{Command, Output};
use tempfile::TempDir;

fn invoke(home: &TempDir, args: &[&str]) -> Output {
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
        .env("PATH", home.path())
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

fn succeeds(home: &TempDir, args: &[&str]) -> String {
    // Windows Known Folder APIs require these target directories to exist.
    // The pure invalid-usage test invokes without this setup and stays side-effect free.
    #[cfg(windows)]
    for path in ["AppData/Roaming", "AppData/Local"] {
        std::fs::create_dir_all(home.path().join(path)).unwrap();
    }
    let output = invoke(home, args);
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn g2_bug_requirement_and_g3_two_target_cross_repo_handoff_roundtrip() {
    let home = TempDir::new().unwrap();
    let url = "https://www.tapd.cn/";
    let sha = "a".repeat(40);
    succeeds(
        &home,
        &[
            "workref",
            "add",
            "--key",
            "BUG17",
            "--provider",
            "tapd",
            "--space",
            "robot",
            "--type",
            "bug",
            "--id",
            "17",
            "--url",
            url,
        ],
    );
    succeeds(
        &home,
        &[
            "workref",
            "add",
            "--key",
            "REQ18",
            "--provider",
            "feishu-project",
            "--space",
            "robot",
            "--type",
            "requirement",
            "--id",
            "18",
            "--url",
            "https://project.feishu.cn/",
        ],
    );
    succeeds(
        &home,
        &[
            "workref",
            "session",
            "--key",
            "BUG17",
            "--alias",
            "soc",
            "--target",
            "ubuntu-A",
            "--home",
            "/home/a/.codex",
            "--thread",
            "same-thread-id",
            "--repo",
            "robot-app",
            "--role",
            "implementation",
        ],
    );
    succeeds(
        &home,
        &[
            "workref",
            "session",
            "--key",
            "BUG17",
            "--alias",
            "mcu",
            "--target",
            "windows-B",
            "--home",
            "C:\\Users\\b\\.codex",
            "--thread",
            "same-thread-id",
            "--repo",
            "mcu-fw",
            "--role",
            "firmware",
        ],
    );
    // A shared ThreadId does not authorize a link on the wrong Target/Home.
    let found_soc: serde_json::Value = serde_json::from_str(&succeeds(
        &home,
        &[
            "workref", "find", "--target", "ubuntu-A",
            "--home", "/home/a/.codex", "--thread", "same-thread-id", "--json",
        ],
    ))
    .unwrap();
    assert_eq!(found_soc.as_array().unwrap().len(), 1);
    assert_eq!(found_soc[0]["key"], "BUG17");
    assert_eq!(found_soc[0]["alias"], "soc");
    assert_eq!(found_soc[0]["manualUnverified"], true);
    let found_mcu: serde_json::Value = serde_json::from_str(&succeeds(
        &home,
        &[
            "workref", "find", "--target", "windows-B",
            "--home", "C:\\Users\\b\\.codex", "--thread", "same-thread-id", "--json",
        ],
    ))
    .unwrap();
    assert_eq!(found_mcu.as_array().unwrap().len(), 1);
    assert_eq!(found_mcu[0]["alias"], "mcu");
    let wrong_home: serde_json::Value = serde_json::from_str(&succeeds(
        &home,
        &[
            "workref", "find", "--target", "ubuntu-A",
            "--home", "C:\\Users\\b\\.codex", "--thread", "same-thread-id", "--json",
        ],
    ))
    .unwrap();
    assert!(wrong_home.as_array().unwrap().is_empty());
    assert!(succeeds(
        &home,
        &[
            "workref", "find", "--target", "ubuntu-A",
            "--home", "/home/a/.codex", "--thread", "missing-thread",
        ],
    ).contains("No matching local manual reference"));

    succeeds(
        &home,
        &[
            "workref",
            "contract",
            "--key",
            "BUG17",
            "--kind",
            "protocol",
            "--source",
            "robot-app/docs/proto.md",
            "--revision",
            "rev-3",
            "--git-sha",
            &sha,
        ],
    );
    succeeds(
        &home,
        &[
            "workref",
            "evidence",
            "--key",
            "BUG17",
            "--kind",
            "unit",
            "--source",
            "https://github.com/acme/run/7",
            "--revision",
            "run-7",
            "--result",
            "passed",
            "--environment",
            "CI",
        ],
    );
    succeeds(
        &home,
        &[
            "workref",
            "evidence",
            "--key",
            "BUG17",
            "--kind",
            "integration",
            "--source",
            "joint-bench/report-8",
            "--revision",
            "fwA+appB",
            "--result",
            "unknown",
            "--environment",
            "robot-bench",
        ],
    );
    let prepared = succeeds(
        &home,
        &[
            "workref",
            "handoff",
            "--key",
            "BUG17",
            "--from",
            "soc",
            "--to",
            "mcu",
            "--summary",
            "Check the shared protocol",
        ],
    );
    assert!(prepared.contains("no delivery attempted"));
    let handoff = succeeds(
        &home,
        &["workref", "handoff-text", "--key", "BUG17", "--index", "0"],
    );
    assert!(handoff.contains("HANDOFF STATUS: prepared"));
    assert!(handoff.contains("FROM: soc · repo robot-app"));
    assert!(handoff.contains("TO: mcu · repo mcu-fw"));
    assert!(handoff.contains("TARGET: ubuntu-A"));
    assert!(handoff.contains("TARGET: windows-B"));
    assert!(handoff.contains("THREAD: same-thread-id"));
    assert!(handoff.contains("CONTRACT: protocol"));
    assert!(handoff.contains("EVIDENCE: unit"));
    assert!(handoff.contains("EVIDENCE: integration"));
    assert!(handoff.contains("result=passed"));
    assert!(handoff.contains("result=unknown"));
    assert!(handoff.contains("manual copy is not delivery"));
    let json: serde_json::Value =
        serde_json::from_str(&succeeds(&home, &["workref", "list", "--json"])).unwrap();
    assert_eq!(
        json["items"]["BUG17"]["sessions"]["soc"]["threadId"],
        "same-thread-id"
    );
    assert_eq!(
        json["items"]["BUG17"]["sessions"]["mcu"]["threadId"],
        "same-thread-id"
    );
    assert_ne!(
        json["items"]["BUG17"]["sessions"]["soc"]["targetIdentity"],
        json["items"]["BUG17"]["sessions"]["mcu"]["targetIdentity"]
    );
    assert_eq!(json["items"]["BUG17"]["contracts"][0]["gitSha"], sha);
    assert_eq!(json["items"]["BUG17"]["evidence"][1]["result"], "unknown");
    assert_eq!(json["items"]["BUG17"]["handoffs"][0]["status"], "prepared");
    assert!(
        json["items"]["REQ18"]["sessions"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    succeeds(
        &home,
        &[
            "workref",
            "ack",
            "--key",
            "BUG17",
            "--index",
            "0",
            "--receipt",
            "manual-receiver-ack",
        ],
    );
    let after: serde_json::Value =
        serde_json::from_str(&succeeds(&home, &["workref", "show", "--key", "BUG17"])).unwrap();
    assert_eq!(after["handoffs"][0]["status"], "acknowledged");
    assert_eq!(after["handoffs"][0]["receipt"], "manual-receiver-ack");
    assert_eq!(after["evidence"][1]["result"], "unknown");
}

#[test]
fn invalid_or_conflicting_cli_operations_never_replace_saved_authority() {
    let home = TempDir::new().unwrap();
    let invalid = invoke(
        &home,
        &[
            "workref",
            "add",
            "--key",
            "X",
            "--provider",
            "tapd",
            "--space",
            "robot",
            "--type",
            "bug",
            "--id",
            "1",
        ],
    );
    assert_eq!(invalid.status.code(), Some(2));
    let invalid_find = invoke(
        &home,
        &[
            "workref", "find", "--target", "ubuntu-A",
            "--thread", "same-thread-id", "--json",
        ],
    );
    assert_eq!(invalid_find.status.code(), Some(2));
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
    succeeds(
        &home,
        &[
            "workref",
            "add",
            "--key",
            "X",
            "--provider",
            "local-draft",
            "--space",
            "robot",
            "--type",
            "technical",
            "--id",
            "local-1",
        ],
    );
    let before = succeeds(&home, &["workref", "show", "--key", "X"]);
    let conflict = invoke(
        &home,
        &[
            "workref",
            "add",
            "--key",
            "X",
            "--provider",
            "local-draft",
            "--space",
            "robot",
            "--type",
            "technical",
            "--id",
            "local-2",
        ],
    );
    assert!(!conflict.status.success());
    let after = succeeds(&home, &["workref", "show", "--key", "X"]);
    assert_eq!(before, after);
    assert!(
        !invoke(
            &home,
            &[
                "workref",
                "handoff",
                "--key",
                "X",
                "--from",
                "unlinked",
                "--to",
                "also-unlinked",
                "--summary",
                "No"
            ]
        )
        .status
        .success()
    );
}
