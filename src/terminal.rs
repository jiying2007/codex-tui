use crossterm::{
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, Stdout, stdout};

struct RestoreGuard<F: FnOnce()> {
    restore: Option<F>,
}

impl<F: FnOnce()> RestoreGuard<F> {
    fn new(restore: F) -> Self {
        Self {
            restore: Some(restore),
        }
    }

    fn disarm(&mut self) {
        self.restore.take();
    }
}

impl<F: FnOnce()> Drop for RestoreGuard<F> {
    fn drop(&mut self) {
        if let Some(restore) = self.restore.take() {
            restore();
        }
    }
}

fn restore_terminal(mouse: bool) {
    let _ = disable_raw_mode();
    let mut out = stdout();
    if mouse {
        let _ = execute!(out, DisableMouseCapture);
    }
    let _ = execute!(out, LeaveAlternateScreen, cursor::Show);
}

pub struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    mouse: bool,
}

impl TerminalSession {
    pub fn enter(mouse: bool) -> io::Result<Self> {
        enable_raw_mode()?;
        let mut rollback = RestoreGuard::new(move || restore_terminal(mouse));
        let mut out = stdout();
        execute!(out, EnterAlternateScreen, cursor::Hide)?;
        if mouse {
            execute!(out, EnableMouseCapture)?;
        }
        let backend = CrosstermBackend::new(out);
        let terminal = Terminal::new(backend)?;
        rollback.disarm();
        Ok(Self { terminal, mouse })
    }

    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        restore_terminal(self.mouse);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn restore_guard_runs_when_setup_exits_early() {
        let calls = Rc::new(Cell::new(0_u8));
        {
            let calls = Rc::clone(&calls);
            let _guard = RestoreGuard::new(move || calls.set(calls.get().saturating_add(1)));
        }
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn restore_guard_disarm_suppresses_cleanup() {
        let calls = Rc::new(Cell::new(0_u8));
        {
            let calls_for_restore = Rc::clone(&calls);
            let mut guard =
                RestoreGuard::new(move || calls_for_restore.set(calls_for_restore.get() + 1));
            guard.disarm();
        }
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn production_enter_arms_rollback_before_fallible_terminal_setup() {
        let source = include_str!("terminal.rs");
        let production = source.split("#[cfg(test)]").next().expect("production source");
        let raw = production.find("enable_raw_mode()?;").expect("raw mode");
        let guard = production
            .find("RestoreGuard::new")
            .expect("restore guard");
        let alternate = production
            .find("EnterAlternateScreen")
            .expect("alternate screen setup");
        let terminal = production
            .find("Terminal::new(backend)?")
            .expect("terminal construction");
        let disarm = production.find("rollback.disarm()").expect("disarm");

        assert!(raw < guard);
        assert!(guard < terminal);
        assert!(alternate < terminal);
        assert!(terminal < disarm);
    }
}
