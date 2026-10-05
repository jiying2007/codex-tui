//! Reap external handoff children without waiting for their lifetime on the UI.
//! One lazy worker and a fixed number of owned children; no process is killed.
use std::{
    io,
    process::{Child, Command},
    sync::{Arc, Condvar, Mutex, OnceLock},
    thread,
    time::Duration,
};

pub const DETACHED_CHILD_LIMIT: usize = 32;
static REAPER: OnceLock<Result<Reaper, String>> = OnceLock::new();

/// Return the started PID, not an exit-success receipt. At capacity, refuse
/// before spawning. Exiting the TUI does not terminate an external application.
pub fn spawn(command: &mut Command) -> io::Result<u32> {
    match REAPER.get_or_init(|| Reaper::start(DETACHED_CHILD_LIMIT).map_err(|e| e.to_string())) {
        Ok(reaper) => reaper.spawn(command),
        Err(error) => Err(io::Error::other(format!(
            "external child reaper unavailable: {error}"
        ))),
    }
}

struct Children {
    processes: Vec<Child>,
    closed: bool,
    wait_error: Option<String>,
}
struct Shared {
    children: Mutex<Children>,
    changed: Condvar,
}
struct Reaper {
    shared: Arc<Shared>,
    limit: usize,
}

impl Reaper {
    fn start(limit: usize) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            children: Mutex::new(Children {
                processes: Vec::with_capacity(limit),
                closed: false,
                wait_error: None,
            }),
            changed: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        thread::Builder::new()
            .name("codex-tui-child-reaper".into())
            .spawn(move || reap(worker))?;
        Ok(Self { shared, limit })
    }

    fn spawn(&self, command: &mut Command) -> io::Result<u32> {
        let mut state = self
            .shared
            .children
            .lock()
            .map_err(|_| io::Error::other("external child reaper state poisoned"))?;
        if state.closed || state.wait_error.is_some() {
            return Err(io::Error::other(format!(
                "external child reaper unavailable: {}",
                state.wait_error.as_deref().unwrap_or("closed")
            )));
        }
        if state.processes.len() >= self.limit {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!(
                    "external child limit ({}) reached; no process started",
                    self.limit
                ),
            ));
        }
        // Registration cannot fail after a successful spawn: capacity is already
        // allocated and the worker shares this brief lock. Neither path waits
        // for a running child's lifetime while holding the lock.
        let child = command.spawn()?;
        let pid = child.id();
        state.processes.push(child);
        self.shared.changed.notify_one();
        Ok(pid)
    }
}

impl Drop for Reaper {
    fn drop(&mut self) {
        let mut state = self
            .shared
            .children
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        self.shared.changed.notify_one();
    }
}

fn reap(shared: Arc<Shared>) {
    let mut state = shared.children.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        let mut wait_error = None;
        state.processes.retain_mut(|child| match child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(error) => {
                // Keep ownership and retry rather than dropping an unreaped PID.
                wait_error = Some(error.to_string());
                true
            }
        });
        state.wait_error = wait_error;
        if state.processes.is_empty() {
            if state.closed {
                return;
            }
            // No polling while idle; new handoffs wake this single worker.
            state = shared
                .changed
                .wait(state)
                .unwrap_or_else(|e| e.into_inner());
        } else {
            state = shared
                .changed
                .wait_timeout(state, Duration::from_millis(100))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

#[cfg(test)]
mod tests;
