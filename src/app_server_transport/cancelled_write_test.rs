//! Native pipe reproduction shared by the old-source negative and repaired-source run.
use super::*;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn cancelled_partial_transport_write_cannot_accept_later_message() {
    let temporary = tempfile::tempdir().unwrap();
    let resume = temporary.path().join("resume");
    let observed = temporary.path().join("observed");
    let script = temporary.path().join("codex");
    std::fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env python3
import os, pathlib, time
resume = pathlib.Path({resume:?})
observed = pathlib.Path({observed:?})
while not resume.exists():
    time.sleep(0.001)
first = os.read(0, 65536)
observed.with_suffix(".tmp").write_text(str(len(first)))
os.replace(observed.with_suffix(".tmp"), observed)
while os.read(0, 65536):
    pass
"#,
            resume = resume.to_str().unwrap(),
            observed = observed.to_str().unwrap()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let target = ResolvedAppServerTarget::implicit_local(Some(script.into_os_string()));
    let mut transport = AppServerTransport::connect(&target).await.unwrap();
    let value = json!({"id":1,"method":"fixture/write","params":"x".repeat(2 * 1024 * 1024)});
    assert!(
        tokio::time::timeout(Duration::from_millis(100), transport.write_json(&value))
            .await
            .is_err()
    );
    std::fs::write(&resume, b"read now").unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !observed.exists() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    // A real pipe prefix was delivered, not merely an unpolled future.
    let count: usize = std::fs::read_to_string(&observed).unwrap().parse().unwrap();
    assert!(count > 0 && count < 2 * 1024 * 1024);
    let later = tokio::time::timeout(
        Duration::from_secs(3),
        transport.write_json(&json!({"id":2,"method":"fixture/must-not-send"})),
    )
    .await
    .unwrap();
    assert!(
        later.is_err(),
        "post-cancellation write unexpectedly accepted"
    );
}
