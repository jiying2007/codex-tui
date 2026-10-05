//! The long-running TUI must reap launcher children, not just forget their PID.
#[cfg(target_os = "linux")]
#[test]
fn completed_launch_preset_children_leave_no_zombies() {
    use codex_tui::launch::LaunchPlan;
    use nix::{
        sys::wait::{WaitPidFlag, waitpid},
        unistd::Pid,
    };
    use std::{
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let plan = LaunchPlan {
        name: "short-lived launcher".into(),
        argv: vec!["/bin/true".into()],
        cwd: root.path().to_path_buf(),
        config_path: root.path().join("launch.toml"),
    };
    let pids: Vec<_> = (0..4).map(|_| plan.execute().unwrap()).collect();
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
