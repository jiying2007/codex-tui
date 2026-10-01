use crate::pty::{PtyCommand, PtyEvent, PtyHandle, TerminalSize};
use anyhow::{Context, Result};
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
    query_tail: Vec<u8>,
    mode_tail: Vec<u8>,
    bracketed_paste: bool,
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
            query_tail: Vec::new(),
            mode_tail: Vec::new(),
            bracketed_paste: false,
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
        self.query_tail.clear();
        self.mode_tail.clear();
        self.bracketed_paste = false;
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
        self.query_tail.clear();
        self.mode_tail.clear();
        self.bracketed_paste = false;
    }

    pub fn send_input(&self, bytes: Vec<u8>) -> Result<()> {
        let handle = self
            .handle
            .as_ref()
            .context("terminal drawer is not open")?;
        handle.send(PtyCommand::Input(bytes))
    }

    pub fn send_paste(&self, text: String) -> Result<()> {
        let handle = self
            .handle
            .as_ref()
            .context("terminal drawer is not open")?;
        handle.send(PtyCommand::Input(encode_paste(&text, self.bracketed_paste)))
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
        self.parser.screen_mut().set_size(size.rows, size.cols);
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
                    self.bracketed_paste =
                        vt_bracketed_paste_mode(&mut self.mode_tail, &bytes, self.bracketed_paste);
                    self.parser.process(&bytes);
                    let (cursor_row, cursor_col) = self.parser.screen().cursor_position();
                    for response in
                        vt_query_responses(&mut self.query_tail, &bytes, cursor_row, cursor_col)
                    {
                        if let Err(error) = handle.send(PtyCommand::Input(response)) {
                            self.state = TerminalProcessState::Error(format!(
                                "terminal query response failed: {error:#}"
                            ));
                            break;
                        }
                    }
                }
                PtyEvent::ReaderClosed => {}
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

fn encode_paste(text: &str, bracketed_paste: bool) -> Vec<u8> {
    let mut bytes =
        Vec::with_capacity(
            text.len()
                .saturating_add(if bracketed_paste { 12 } else { 0 }),
        );
    if bracketed_paste {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    bytes.extend_from_slice(text.as_bytes());
    if bracketed_paste {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    bytes
}

pub fn vt_bracketed_paste_mode(tail: &mut Vec<u8>, bytes: &[u8], mut enabled: bool) -> bool {
    const ENABLE: &[u8] = b"\x1b[?2004h";
    const DISABLE: &[u8] = b"\x1b[?2004l";

    let mut probe = Vec::with_capacity(tail.len().saturating_add(bytes.len()));
    probe.extend_from_slice(tail);
    probe.extend_from_slice(bytes);

    let mut offset = 0;
    while offset < probe.len() {
        if probe[offset..].starts_with(ENABLE) {
            enabled = true;
            offset += ENABLE.len();
            continue;
        }
        if probe[offset..].starts_with(DISABLE) {
            enabled = false;
            offset += DISABLE.len();
            continue;
        }
        offset += 1;
    }

    let keep = probe
        .len()
        .min(ENABLE.len().max(DISABLE.len()).saturating_sub(1));
    tail.clear();
    tail.extend_from_slice(&probe[probe.len().saturating_sub(keep)..]);
    enabled
}

pub fn vt_query_responses(
    tail: &mut Vec<u8>,
    bytes: &[u8],
    cursor_row: u16,
    cursor_col: u16,
) -> Vec<Vec<u8>> {
    let mut probe = Vec::with_capacity(tail.len().saturating_add(bytes.len()));
    probe.extend_from_slice(tail);
    probe.extend_from_slice(bytes);

    let mut responses = Vec::new();
    let mut offset = 0;
    while offset < probe.len() {
        if probe[offset..].starts_with(b"\x1b[5n") {
            responses.push(b"\x1b[0n".to_vec());
            offset += 4;
            continue;
        }
        if probe[offset..].starts_with(b"\x1b[6n") {
            responses.push(
                format!(
                    "\x1b[{};{}R",
                    cursor_row.saturating_add(1),
                    cursor_col.saturating_add(1)
                )
                .into_bytes(),
            );
            offset += 4;
            continue;
        }
        offset += 1;
    }

    let keep = probe.len().min(3);
    tail.clear();
    tail.extend_from_slice(&probe[probe.len().saturating_sub(keep)..]);
    responses
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
    fn vt_query_responses_cover_status_and_cursor_reports() {
        let mut tail = Vec::new();
        assert_eq!(
            vt_query_responses(&mut tail, b"\x1b[5n", 0, 0),
            vec![b"\x1b[0n".to_vec()]
        );
        assert_eq!(
            vt_query_responses(&mut tail, b"prefix\x1b[6nsuffix", 4, 9),
            vec![b"\x1b[5;10R".to_vec()]
        );
        assert!(vt_query_responses(&mut tail, b"plain text", 0, 0).is_empty());
    }

    #[test]
    fn vt_query_response_survives_split_output_chunks() {
        let mut tail = Vec::new();
        assert!(vt_query_responses(&mut tail, b"\x1b[", 0, 0).is_empty());
        assert_eq!(
            vt_query_responses(&mut tail, b"6n", 2, 3),
            vec![b"\x1b[3;4R".to_vec()]
        );
    }

    #[test]
    fn paste_encoding_only_adds_markers_when_child_enabled_bracketed_mode() {
        assert_eq!(encode_paste("a\nb", false), b"a\nb");
        assert_eq!(encode_paste("a\nb", true), b"\x1b[200~a\nb\x1b[201~");
    }

    #[test]
    fn bracketed_paste_mode_tracks_enable_disable_across_split_chunks() {
        let mut tail = Vec::new();
        let mut enabled = false;

        enabled = vt_bracketed_paste_mode(&mut tail, b"\x1b[?20", enabled);
        assert!(!enabled);
        enabled = vt_bracketed_paste_mode(&mut tail, b"04h", enabled);
        assert!(enabled);

        enabled = vt_bracketed_paste_mode(&mut tail, b"plain", enabled);
        assert!(enabled);
        enabled = vt_bracketed_paste_mode(&mut tail, b"\x1b[?2004l", enabled);
        assert!(!enabled);
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
