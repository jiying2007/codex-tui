use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, PtySize, PtySystem, native_pty_system};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::thread::{self, JoinHandle};

const EVENT_QUEUE_CAPACITY: usize = 64;
const OUTPUT_CHUNK_BYTES: usize = 8 * 1024;
pub const DEFAULT_SCROLLBACK_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
}

impl TerminalSize {
    pub fn validate(self) -> Result<Self> {
        anyhow::ensure!(self.rows > 0, "PTY rows must be greater than zero");
        anyhow::ensure!(self.cols > 0, "PTY cols must be greater than zero");
        Ok(self)
    }

    fn portable(self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PtyCommand {
    Input(Vec<u8>),
    Resize(TerminalSize),
    Terminate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PtyEvent {
    Ready {
        cwd: PathBuf,
        size: TerminalSize,
    },
    Output(Vec<u8>),
    Exited {
        success: bool,
        code: Option<u32>,
    },
    Error(String),
}

pub struct PtyHandle {
    command_tx: SyncSender<PtyCommand>,
    event_rx: Receiver<PtyEvent>,
    actor: Option<JoinHandle<()>>,
}

impl PtyHandle {
    pub fn start(cwd: impl Into<PathBuf>, size: TerminalSize) -> Result<Self> {
        let cwd = cwd.into();
        size.validate()?;
        let (command_tx, command_rx) = sync_channel(64);
        let (event_tx, event_rx) = sync_channel(EVENT_QUEUE_CAPACITY);
        let actor = thread::Builder::new()
            .name("codex-tui-pty".into())
            .spawn(move || run_actor(cwd, size, command_rx, event_tx))
            .context("spawn PTY actor")?;

        Ok(Self {
            command_tx,
            event_rx,
            actor: Some(actor),
        })
    }

    pub fn send(&self, command: PtyCommand) -> Result<()> {
        self.command_tx
            .send(command)
            .context("PTY command channel closed")
    }

    pub fn try_recv(&self) -> Option<PtyEvent> {
        match self.event_rx.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }
}

impl Drop for PtyHandle {
    fn drop(&mut self) {
        let _ = self.command_tx.send(PtyCommand::Terminate);
        if let Some(actor) = self.actor.take() {
            let _ = actor.join();
        }
    }
}

fn run_actor(
    cwd: PathBuf,
    size: TerminalSize,
    command_rx: Receiver<PtyCommand>,
    event_tx: SyncSender<PtyEvent>,
) {
    if let Err(error) = run_actor_inner(&cwd, size, command_rx, &event_tx) {
        let _ = event_tx.send(PtyEvent::Error(format!("{error:#}")));
    }
}

fn run_actor_inner(
    cwd: &Path,
    size: TerminalSize,
    command_rx: Receiver<PtyCommand>,
    event_tx: &SyncSender<PtyEvent>,
) -> Result<()> {
    let cwd = std::fs::canonicalize(cwd)
        .with_context(|| format!("resolve PTY cwd {}", cwd.display()))?;
    anyhow::ensure!(cwd.is_dir(), "PTY cwd is not a directory: {}", cwd.display());

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(size.portable())
        .context("open native PTY")?;

    let mut command = CommandBuilder::new_default_prog();
    command.cwd(&cwd);
    let mut child = pair
        .slave
        .spawn_command(command)
        .context("spawn platform default PTY program")?;
    let mut killer = child.clone_killer();
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .context("clone PTY reader")?;
    let mut writer = pair.master.take_writer().context("take PTY writer")?;

    let output_tx = event_tx.clone();
    thread::Builder::new()
        .name("codex-tui-pty-reader".into())
        .spawn(move || {
            let mut buffer = vec![0_u8; OUTPUT_CHUNK_BYTES];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        if output_tx
                            .send(PtyEvent::Output(buffer[..count].to_vec()))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = output_tx.send(PtyEvent::Error(format!(
                            "PTY reader failed: {error}"
                        )));
                        break;
                    }
                }
            }
        })
        .context("spawn PTY reader")?;

    let exit_tx = event_tx.clone();
    thread::Builder::new()
        .name("codex-tui-pty-wait".into())
        .spawn(move || match child.wait() {
            Ok(status) => {
                let _ = exit_tx.send(PtyEvent::Exited {
                    success: status.success(),
                    code: status.exit_code(),
                });
            }
            Err(error) => {
                let _ = exit_tx.send(PtyEvent::Error(format!(
                    "PTY child wait failed: {error}"
                )));
            }
        })
        .context("spawn PTY waiter")?;

    event_tx
        .send(PtyEvent::Ready {
            cwd: cwd.clone(),
            size,
        })
        .context("emit PTY ready")?;

    while let Ok(command) = command_rx.recv() {
        match command {
            PtyCommand::Input(bytes) => {
                writer.write_all(&bytes).context("write PTY input")?;
                writer.flush().context("flush PTY input")?;
            }
            PtyCommand::Resize(size) => {
                let size = size.validate()?;
                pair.master
                    .resize(size.portable())
                    .context("resize PTY")?;
            }
            PtyCommand::Terminate => {
                let _ = killer.kill();
                break;
            }
        }
    }

    let _ = killer.kill();
    Ok(())
}

#[derive(Clone, Debug)]
pub struct BoundedScrollback {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
    max_bytes: usize,
    truncated_bytes: u64,
}

impl BoundedScrollback {
    pub fn new(max_bytes: usize) -> Result<Self> {
        anyhow::ensure!(max_bytes > 0, "scrollback max_bytes must be greater than zero");
        Ok(Self {
            chunks: VecDeque::new(),
            bytes: 0,
            max_bytes,
            truncated_bytes: 0,
        })
    }

    pub fn push(&mut self, mut bytes: Vec<u8>) {
        if bytes.len() > self.max_bytes {
            let dropped = bytes.len() - self.max_bytes;
            bytes.drain(..dropped);
            self.truncated_bytes = self
                .truncated_bytes
                .saturating_add(u64::try_from(dropped).unwrap_or(u64::MAX));
        }
        self.bytes = self.bytes.saturating_add(bytes.len());
        self.chunks.push_back(bytes);

        while self.bytes > self.max_bytes {
            let Some(front) = self.chunks.pop_front() else {
                self.bytes = 0;
                break;
            };
            self.bytes = self.bytes.saturating_sub(front.len());
            self.truncated_bytes = self
                .truncated_bytes
                .saturating_add(u64::try_from(front.len()).unwrap_or(u64::MAX));
        }
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn truncated_bytes(&self) -> u64 {
        self.truncated_bytes
    }

    pub fn snapshot(&self) -> Vec<u8> {
        let mut output = Vec::with_capacity(self.bytes);
        for chunk in &self.chunks {
            output.extend_from_slice(chunk);
        }
        output
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtyCapabilities {
    pub backend: &'static str,
    pub platform: &'static str,
    pub default_program: bool,
    pub resize: bool,
    pub input: bool,
    pub bounded_event_queue: usize,
    pub default_scrollback_bytes: usize,
}

pub const fn capabilities() -> PtyCapabilities {
    PtyCapabilities {
        backend: "portable-pty/native",
        platform: std::env::consts::OS,
        default_program: true,
        resize: true,
        input: true,
        bounded_event_queue: EVENT_QUEUE_CAPACITY,
        default_scrollback_bytes: DEFAULT_SCROLLBACK_BYTES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_size_rejects_zero_dimensions() {
        assert!(TerminalSize { rows: 0, cols: 80 }.validate().is_err());
        assert!(TerminalSize { rows: 24, cols: 0 }.validate().is_err());
        assert!(TerminalSize { rows: 24, cols: 80 }.validate().is_ok());
    }

    #[test]
    fn scrollback_is_strictly_bounded() {
        let mut scrollback = BoundedScrollback::new(8).expect("scrollback");
        scrollback.push(b"1234".to_vec());
        scrollback.push(b"5678".to_vec());
        assert_eq!(scrollback.snapshot(), b"12345678");
        scrollback.push(b"90".to_vec());
        assert!(scrollback.bytes() <= 8);
        assert_eq!(scrollback.snapshot(), b"567890");
        assert_eq!(scrollback.truncated_bytes(), 4);
    }

    #[test]
    fn oversize_chunk_keeps_only_bounded_tail() {
        let mut scrollback = BoundedScrollback::new(4).expect("scrollback");
        scrollback.push(b"123456".to_vec());
        assert_eq!(scrollback.snapshot(), b"3456");
        assert_eq!(scrollback.truncated_bytes(), 2);
    }

    #[test]
    fn capability_contract_is_explicit() {
        let capabilities = capabilities();
        assert_eq!(capabilities.backend, "portable-pty/native");
        assert!(capabilities.default_program);
        assert!(capabilities.resize);
        assert!(capabilities.input);
        assert_eq!(capabilities.bounded_event_queue, 64);
    }
}
