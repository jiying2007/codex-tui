//! Single-owner child lifetime. This guard is only held on the detached PTY actor.
use super::*;

pub(super) struct ReapChild {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    runtime: PtyRuntime,
    reaped: bool,
}
impl ReapChild {
    pub(super) fn new(
        child: Box<dyn portable_pty::Child + Send + Sync>,
        runtime: PtyRuntime,
    ) -> Self {
        Self {
            child,
            runtime,
            reaped: false,
        }
    }
    pub(super) fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        self.child.clone_killer()
    }
    #[cfg(test)]
    pub(super) fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }
    pub(super) fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.reaped = true;
            self.clear_killer();
        }
        Ok(status)
    }
    pub(super) fn terminate_and_wait(
        &mut self,
    ) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        if self.reaped {
            return Ok(None);
        }
        signal_foreground_process_group(&self.runtime.pty_master);
        let _ = self.child.kill();
        let status = self.child.wait()?;
        self.reaped = true;
        self.clear_killer();
        Ok(Some(status))
    }
    fn clear_killer(&self) {
        if let Ok(mut killer) = self.runtime.child_killer.lock() {
            killer.take();
        }
    }
}
impl Drop for ReapChild {
    fn drop(&mut self) {
        // Detached actor only: preserve ownership without spawning a fallible waiter.
        let _ = self.terminate_and_wait();
        self.clear_killer();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn every_startup_stage_failure_reaps_the_actual_child() {
        for stage in [
            StartupStage::Spawned,
            StartupStage::Registered,
            StartupStage::ReaderCloned,
            StartupStage::WriterTaken,
            StartupStage::ReadySent,
            StartupStage::ReaderStarted,
        ] {
            let home = tempfile::tempdir().unwrap();
            let size = TerminalSize { rows: 24, cols: 80 };
            let pair = native_pty_system().openpty(size.portable()).unwrap();
            let mut command = CommandBuilder::new("/bin/sh");
            command.args(["-c", "exec sleep 30"]);
            command.cwd(home.path());
            let (_commands, rx) = sync_channel(4);
            let (tx, _events) = sync_channel(16);
            let runtime = PtyRuntime {
                fail_after: Some(stage),
                ..PtyRuntime::new()
            };
            let result = spawn_and_drive_pty(
                pair,
                command,
                home.path().to_path_buf(),
                size,
                rx,
                &tx,
                &runtime,
            );
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("injected PTY startup failure")
            );
            let pid = runtime
                .spawned_pid
                .lock()
                .unwrap()
                .expect("actual child was spawned");
            let result = nix::sys::wait::waitpid(
                nix::unistd::Pid::from_raw(pid as i32),
                Some(nix::sys::wait::WaitPidFlag::WNOHANG),
            );
            assert_eq!(
                result.unwrap_err(),
                nix::errno::Errno::ECHILD,
                "unreaped child at {stage:?}"
            );
            assert!(runtime.child_killer.lock().unwrap().is_none());
        }
    }
    #[test]
    fn close_before_ready_does_not_lose_child_wait_ownership() {
        let home = tempfile::tempdir().unwrap();
        let size = TerminalSize { rows: 24, cols: 80 };
        let pair = native_pty_system().openpty(size.portable()).unwrap();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "exec sleep 30"]);
        let (commands, rx) = sync_channel(1);
        let (tx, events) = sync_channel(1);
        drop(events);
        drop(commands);
        let runtime = PtyRuntime::new();
        assert!(
            spawn_and_drive_pty(
                pair,
                command,
                home.path().to_path_buf(),
                size,
                rx,
                &tx,
                &runtime
            )
            .is_err()
        );
        let pid = runtime.spawned_pid.lock().unwrap().unwrap();
        assert_eq!(
            nix::sys::wait::waitpid(
                nix::unistd::Pid::from_raw(pid as i32),
                Some(nix::sys::wait::WaitPidFlag::WNOHANG)
            )
            .unwrap_err(),
            nix::errno::Errno::ECHILD
        );
    }
}
