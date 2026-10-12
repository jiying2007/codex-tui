use super::*;
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};

const HELPER: &str = r#"
import json, os, subprocess, sys
child = subprocess.Popen(['python3', '-c', 'import time; time.sleep(30)'])
with open(sys.argv[1], 'w') as out:
    json.dump({'pid': child.pid, 'stdout': os.readlink('/proc/self/fd/1'),
               'stderr': os.readlink('/proc/self/fd/2')}, out)
print('{}', flush=True)
"#;
struct HelperGuard(i32);
impl Drop for HelperGuard {
    fn drop(&mut self) {
        let _ = kill(Pid::from_raw(self.0), Signal::SIGKILL);
    }
}
async fn receipt(path: &Path) -> (HelperGuard, serde_json::Value) {
    let value = timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(bytes) = std::fs::read(path)
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("helper did not publish pipe identities");
    (HelperGuard(value["pid"].as_i64().unwrap() as i32), value)
}
fn assert_no_pipe(value: &serde_json::Value) {
    let owned = std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|entry| std::fs::read_link(entry.ok()?.path()).ok())
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    for name in ["stdout", "stderr"] {
        assert!(
            !owned
                .iter()
                .any(|path| path == value[name].as_str().unwrap()),
            "cancelled Forge reader still owns {name}: {value}"
        );
    }
}
#[tokio::test]
async fn repeated_deadlines_drop_descendant_held_readers() {
    let dir = tempfile::tempdir().unwrap();
    for round in 0..3 {
        let path = dir.path().join(format!("{round}.json"));
        let task_path = path.clone();
        let task = tokio::spawn(async move {
            run_command(
                "python3",
                &["-c", HELPER, task_path.to_str().unwrap()],
                None,
            )
            .await
        });
        let (_guard, value) = receipt(&path).await;
        let error = task.await.unwrap().unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert_no_pipe(&value);
    }
}
#[tokio::test]
async fn caller_cancellation_drops_both_live_readers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cancel.json");
    let task_path = path.clone();
    let task = tokio::spawn(async move {
        run_command(
            "python3",
            &["-c", HELPER, task_path.to_str().unwrap()],
            None,
        )
        .await
    });
    let (_guard, value) = receipt(&path).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_no_pipe(&value);
}
#[tokio::test]
async fn concurrent_large_output_is_drained_and_keeps_caps() {
    let command = "import os; os.write(1,b'x'*200000); os.write(2,b'y'*200000)";
    let output = run_command("python3", &["-c", command], None)
        .await
        .unwrap();
    assert!(output.success);
    assert_eq!(output.stdout.len(), 200000usize.min(MAX_STDOUT_BYTES));
    assert_eq!(output.stderr.len(), 200000usize.min(MAX_STDERR_BYTES));
}
