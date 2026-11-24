//! The long-running TUI must reap launcher children, not just forget their PID.
#[cfg(target_os = "linux")]
#[test]
fn completed_external_handoff_children_leave_no_zombies() {
    use codex_tui::detached_process;
    use nix::{
        sys::wait::{WaitPidFlag, waitpid},
        unistd::Pid,
    };
    use std::{
        path::PathBuf,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let pids: Vec<_> = (0..4)
        .map(|_| {
            let mut command = Command::new("/bin/true");
            command
                .current_dir(root.path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            detached_process::spawn(&mut command).unwrap()
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(3);
    let remains = |pid: &u32| PathBuf::from(format!("/proc/{pid}")).exists();
    while pids.iter().any(remains) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let leaked: Vec<_> = pids.iter().copied().filter(remains).collect();
    // Clean up the expected baseline zombies only after recording the failure.
    for pid in &leaked {
        let _ = waitpid(Pid::from_raw(*pid as i32), Some(WaitPidFlag::WNOHANG));
    }
    assert!(
        leaked.is_empty(),
        "completed launcher children are still present: {leaked:?}"
    );
}
