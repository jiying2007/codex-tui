use crate::pty::{PtyCommand, PtyEvent, PtyHandle, TerminalSize};
use anyhow::{Context, Result};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};

pub const DEFAULT_DRAWER_ROWS: u16 = 12;
pub const DEFAULT_DRAWER_COLS: u16 = 80;
pub const DEFAULT_SCROLLBACK_LINES: usize = 2_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalProcessState {
    Starting,
    Running,
    Exited { success: bool, code: Option<u32> },
    Error(String),
}

impl TerminalProcessState {
    pub fn label(&self) -> String {
        match self {
            Self::Starting => "starting".into(),
            Self::Running => "running".into(),
            Self::Exited { success, code } => format!(
                "exited · success={} · code={}",
                success,
                code.map_or_else(|| "?".into(), |value| value.to_string())
            ),
            Self::Error(error) => format!("error · {error}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub cwd: String,
    pub size: TerminalSize,
    pub rows: Vec<String>,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub scrollback: usize,
    pub state: TerminalProcessState,
}

pub struct TerminalDrawerRuntime {
    handle: Option<PtyHandle>,
    parser: vt100::Parser,
    cwd: Option<PathBuf>,
    size: TerminalSize,
    state: TerminalProcessState,
}

impl Default for TerminalDrawerRuntime {
    fn default() -> Self {
        let size = TerminalSize {
            rows: DEFAULT_DRAWER_ROWS,
            cols: DEFAULT_DRAWER_COLS,
        };
        Self {
            handle: None,
            parser: vt100::Parser::new(size.rows, size.cols, DEFAULT_SCROLLBACK_LINES),
            cwd: None,
            size,
            state: TerminalProcessState::Exited {
                success: true,
                code: None,
            },
        }
    }
}

impl TerminalDrawerRuntime {
    pub fn open(&mut self, cwd: impl Into<PathBuf>, size: TerminalSize) -> Result<()> {
        self.close();
        let size = size.validate()?;
        let cwd = cwd.into();
        let canonical = std::fs::canonicalize(&cwd)
            .with_context(|| format!("resolve terminal drawer cwd {}", cwd.display()))?;
        anyhow::ensure!(
            canonical.is_dir(),
            "terminal drawer cwd is not a directory: {}",
            canonical.display()
        );

        self.parser = vt100::Parser::new(size.rows, size.cols, DEFAULT_SCROLLBACK_LINES);
        self.handle = Some(PtyHandle::start(canonical.clone(), size)?);
        self.cwd = Some(canonical);
        self.size = size;
        self.state = TerminalProcessState::Starting;
        Ok(())
    }

    pub fn is_open(&self) -> bool {
        self.handle.is_some()
    }

    pub fn close(&mut self) {
        self.handle.take();
        self.cwd = None;
        self.state = TerminalProcessState::Exited {
            success: true,
            code: None,
        };
    }

    pub fn send_input(&self, bytes: Vec<u8>) -> Result<()> {
        let handle = self
            .handle
            .as_ref()
            .context("terminal drawer is not open")?;
        handle.send(PtyCommand::Input(bytes))
    }

    pub fn resize(&mut self, size: TerminalSize) -> Result<()> {
        let size = size.validate()?;
        if size == self.size {
            return Ok(());
        }
        let handle = self
            .handle
            .as_ref()
            .context("terminal drawer is not open")?;
        handle.send(PtyCommand::Resize(size))?;
        self.parser.screen_mut().set_size(
            NonZeroU16::new(size.rows).context("terminal rows became zero")?,
            NonZeroU16::new(size.cols).context("terminal cols became zero")?,
        );
        self.size = size;
        Ok(())
    }

    pub fn scroll(&mut self, delta: i32) -> Result<()> {
        anyhow::ensure!(self.handle.is_some(), "terminal drawer is not open");
        let current = self.parser.screen().scrollback();
        let next = if delta >= 0 {
            current.saturating_add(delta as usize)
        } else {
            current.saturating_sub(delta.unsigned_abs() as usize)
        };
        self.parser.screen_mut().set_scrollback(next);
        Ok(())
    }

    pub fn drain(&mut self) -> Option<TerminalSnapshot> {
        let handle = self.handle.as_ref()?;
        let mut changed = false;
        while let Some(event) = handle.try_recv() {
            changed = true;
            match event {
                PtyEvent::Ready { cwd, size } => {
                    self.cwd = Some(cwd);
                    self.size = size;
                    self.state = TerminalProcessState::Running;
                }
                PtyEvent::Output(bytes) => {
                    self.parser.process(&bytes);
                }
                PtyEvent::Exited { success, code } => {
                    self.state = TerminalProcessState::Exited { success, code };
                }
                PtyEvent::Error(error) => {
                    self.state = TerminalProcessState::Error(error);
                }
            }
        }
        changed.then(|| self.snapshot())
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        let screen = self.parser.screen();
        let rows = screen
            .rows(0, self.size.cols)
            .map(|row| row.trim_end_matches(' ').to_string())
            .collect::<Vec<_>>();
        let (cursor_row, cursor_col) = screen.cursor_position();
        TerminalSnapshot {
            cwd: self
                .cwd
                .as_deref()
                .unwrap_or_else(|| Path::new(""))
                .to_string_lossy()
                .into_owned(),
            size: self.size,
            rows,
            cursor_row,
            cursor_col,
            scrollback: screen.scrollback(),
            state: self.state.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vt_screen_parses_escape_sequences_without_leaking_ansi() {
        let mut runtime = TerminalDrawerRuntime::default();
        runtime.parser.process(b"hello\r\n\x1b[31mRED\x1b[0m");
        let snapshot = runtime.snapshot();
        assert!(snapshot.rows.iter().any(|row| row.contains("hello")));
        assert!(snapshot.rows.iter().any(|row| row.contains("RED")));
        assert!(snapshot.rows.iter().all(|row| !row.contains("\x1b")));
    }

    #[test]
    fn terminal_state_labels_are_stable() {
        assert_eq!(TerminalProcessState::Starting.label(), "starting");
        assert_eq!(TerminalProcessState::Running.label(), "running");
        assert!(
            TerminalProcessState::Exited {
                success: false,
                code: Some(2)
            }
            .label()
            .contains("code=2")
        );
    }
}
