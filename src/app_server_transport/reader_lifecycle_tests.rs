//! Real pipe ownership, not fd-count heuristics or termination of unrelated children.
use super::*;
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

struct HelperGuard(i32);
impl Drop for HelperGuard {
    fn drop(&mut self) {
        let _ = kill(Pid::from_raw(self.0), Signal::SIGKILL);
    }
}

fn fixture(dir: &Path, mode: &str) -> (ResolvedAppServerTarget, PathBuf) {
    let receipt = dir.join(format!("{mode}.json"));
    let script = dir.join(format!("codex-{mode}"));
    std::fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env python3
import json, os, pathlib, subprocess, sys
mode = {mode:?}
receipt = pathlib.Path({receipt:?})
child = subprocess.Popen(['python3', '-c', 'import time;time.sleep(30)'],
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
value = {{'child':child.pid, 'pipe':os.readlink('/proc/self/fd/2')}}
partial = receipt.with_suffix('.tmp')
partial.write_text(json.dumps(value))
os.replace(partial, receipt)
if mode == 'healthy':
    os.write(2, b'x' * 200000)
    print('{{"ok":true}}', flush=True)
elif mode == 'malformed':
    print('{{invalid-json}}', flush=True)
for line in sys.stdin:
    pass
"#,
            receipt = receipt.to_str().unwrap()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    (
        ResolvedAppServerTarget::implicit_local(Some(script.into_os_string())),
        receipt,
    )
}

async fn receipt(path: &Path) -> (HelperGuard, String) {
    let value = timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(bytes) = std::fs::read(path)
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
            {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture did not publish stderr identity");
    (
        HelperGuard(value["child"].as_i64().unwrap() as i32),
        value["pipe"].as_str().unwrap().into(),
    )
}

fn owns_pipe(pipe: &str) -> bool {
    std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|e| std::fs::read_link(e.ok()?.path()).ok())
        .any(|path| path.as_os_str() == std::ffi::OsStr::new(pipe))
}

async fn released(pipe: &str, helper: &HelperGuard) {
    timeout(Duration::from_secs(2), async {
        while owns_pipe(pipe) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("dropped App Server still owns exact stderr pipe");
    assert!(
        Path::new(&format!("/proc/{}", helper.0)).exists(),
        "writer exited before cleanup assertion"
    );
}

#[tokio::test]
async fn repeated_successful_connections_release_their_owned_stderr_readers() {
    let mut held = Vec::new();
    for _ in 0..5 {
        let temp = tempfile::tempdir().unwrap();
        let (target, path) = fixture(temp.path(), "healthy");
        let mut transport = AppServerTransport::connect(&target).await.unwrap();
        let (helper, pipe) = receipt(&path).await;
        assert!(owns_pipe(&pipe));
        assert_eq!(
            transport.read_json().await.unwrap(),
            Some(serde_json::json!({"ok":true}))
        );
        drop(transport);
        released(&pipe, &helper).await;
        held.push((helper, pipe));
    }
    assert!(held.iter().all(|(_, pipe)| !owns_pipe(pipe)));
}

#[tokio::test]
async fn malformed_initial_response_drop_reclaims_stderr() {
    let temp = tempfile::tempdir().unwrap();
    let (target, path) = fixture(temp.path(), "malformed");
    let mut transport = AppServerTransport::connect(&target).await.unwrap();
    let (helper, pipe) = receipt(&path).await;
    assert!(transport.read_json().await.is_err());
    assert!(transport.is_quarantined());
    drop(transport);
    released(&pipe, &helper).await;
}

#[tokio::test]
async fn cancelled_transport_owner_does_not_detach_stderr_reader() {
    let temp = tempfile::tempdir().unwrap();
    let (target, path) = fixture(temp.path(), "stalled");
    let owner = tokio::spawn(async move {
        let mut transport = AppServerTransport::connect(&target).await.unwrap();
        transport.read_json().await
    });
    let (helper, pipe) = receipt(&path).await;
    assert!(owns_pipe(&pipe));
    owner.abort();
    assert!(owner.await.unwrap_err().is_cancelled());
    released(&pipe, &helper).await;
}
