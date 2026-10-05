use super::*;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Instant,
};

#[test]
fn child_fixture() {
    let Some(marker) = std::env::var_os("CODEX_TUI_REAPER_TEST_MARKER") else {
        return;
    };
    std::fs::write(marker, b"started").unwrap();
    if let Some(release) = std::env::var_os("CODEX_TUI_REAPER_TEST_RELEASE") {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !Path::new(&release).exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
    }
    if std::env::var_os("CODEX_TUI_REAPER_TEST_FAILURE").is_some() {
        std::process::exit(23);
    }
}

fn command(marker: &Path, release: Option<&Path>) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "detached_process::tests::child_fixture"])
        .env("CODEX_TUI_REAPER_TEST_MARKER", marker)
        .env_remove("CODEX_TUI_REAPER_TEST_RELEASE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(release) = release {
        command.env("CODEX_TUI_REAPER_TEST_RELEASE", release);
    }
    command
}
fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "child lifecycle deadline exceeded"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn count(reaper: &Reaper) -> usize {
    reaper.shared.children.lock().unwrap().processes.len()
}
struct Release(PathBuf);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.0, b"release");
    }
}

#[test]
fn capacity_refuses_before_spawn_and_reuses_reaped_slot() {
    let root = tempfile::tempdir().unwrap();
    let release = Release(root.path().join("release"));
    let first = root.path().join("first");
    let second = root.path().join("second");
    let reaper = Reaper::start(1).unwrap();
    reaper
        .spawn(&mut command(&first, Some(&release.0)))
        .unwrap();
    until(|| first.exists());
    assert_eq!(count(&reaper), 1);
    let error = reaper.spawn(&mut command(&second, None)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert!(!second.exists(), "rejected command was executed");
    drop(release);
    until(|| count(&reaper) == 0);
    reaper.spawn(&mut command(&second, None)).unwrap();
    until(|| second.exists() && count(&reaper) == 0);
}

#[test]
fn spawn_failure_does_not_consume_capacity() {
    let root = tempfile::tempdir().unwrap();
    let reaper = Reaper::start(1).unwrap();
    assert!(
        reaper
            .spawn(&mut Command::new(root.path().join("missing")))
            .is_err()
    );
    assert_eq!(count(&reaper), 0);
    let marker = root.path().join("good");
    reaper.spawn(&mut command(&marker, None)).unwrap();
    until(|| marker.exists() && count(&reaper) == 0);
}

#[test]
fn long_running_child_does_not_block_other_reaping_or_get_killed() {
    let root = tempfile::tempdir().unwrap();
    let release = Release(root.path().join("release"));
    let reaper = Reaper::start(2).unwrap();
    let first = root.path().join("first");
    reaper
        .spawn(&mut command(&first, Some(&release.0)))
        .unwrap();
    until(|| first.exists());
    for index in 0..6 {
        let marker = root.path().join(format!("short-{index}"));
        let mut short = command(&marker, None);
        short.env("CODEX_TUI_REAPER_TEST_FAILURE", "1");
        reaper.spawn(&mut short).unwrap();
        until(|| marker.exists() && count(&reaper) == 1);
    }
    let mut state = reaper.shared.children.lock().unwrap();
    assert!(state.processes[0].try_wait().unwrap().is_none());
    drop(state);
    drop(release);
    until(|| count(&reaper) == 0);
}

#[test]
fn supervisor_drop_keeps_ownership_until_children_exit() {
    let root = tempfile::tempdir().unwrap();
    let release = Release(root.path().join("release"));
    let marker = root.path().join("child");
    let reaper = Reaper::start(1).unwrap();
    let shared = Arc::clone(&reaper.shared);
    reaper
        .spawn(&mut command(&marker, Some(&release.0)))
        .unwrap();
    until(|| marker.exists());
    drop(reaper);
    assert!(
        shared.children.lock().unwrap().processes[0]
            .try_wait()
            .unwrap()
            .is_none()
    );
    drop(release);
    until(|| Arc::strong_count(&shared) == 1);
    assert!(shared.children.lock().unwrap().processes.is_empty());
}

#[test]
fn concurrent_admission_never_exceeds_the_process_limit() {
    let root = tempfile::tempdir().unwrap();
    let release = Release(root.path().join("release"));
    let reaper = Arc::new(Reaper::start(2).unwrap());
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|index| {
            let reaper = Arc::clone(&reaper);
            let barrier = Arc::clone(&barrier);
            let marker = root.path().join(format!("worker-{index}"));
            let release = release.0.clone();
            thread::spawn(move || {
                barrier.wait();
                reaper.spawn(&mut command(&marker, Some(&release)))
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 2);
    assert!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .all(|error| error.kind() == io::ErrorKind::WouldBlock)
    );
    assert_eq!(count(&reaper), 2);
    drop(release);
    until(|| count(&reaper) == 0);
}
