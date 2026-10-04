use anyhow::{Context, Result};
use portable_pty::{ChildKiller, CommandBuilder, PtySize, native_pty_system};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

type SharedChildKiller = Arc<Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>>;
type SharedPtyMaster = Arc<Mutex<Option<Box<dyn portable_pty::MasterPty>>>>;

#[derive(Clone)]
struct PtyRuntime {
    child_killer: SharedChildKiller,
    pty_master: SharedPtyMaster,
}

impl PtyRuntime {
    fn new() -> Self {
        Self {
            child_killer: Arc::new(Mutex::new(None)),
            pty_master: Arc::new(Mutex::new(None)),
        }
    }
}

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
    Ready { cwd: PathBuf, size: TerminalSize },
    Output(Vec<u8>),
    ReaderClosed,
    Exited { success: bool, code: Option<u32> },
    Error(String),
}

pub struct PtyHandle {
    command_tx: Option<SyncSender<PtyCommand>>,
    event_rx: Option<Receiver<PtyEvent>>,
    actor: Option<JoinHandle<()>>,
    runtime: PtyRuntime,
}

impl PtyHandle {
    pub fn start(cwd: impl Into<PathBuf>, size: TerminalSize) -> Result<Self> {
        let cwd = cwd.into();
        size.validate()?;
        let (command_tx, command_rx) = sync_channel(64);
        let (event_tx, event_rx) = sync_channel(EVENT_QUEUE_CAPACITY);
        let runtime = PtyRuntime::new();
        let actor_runtime = runtime.clone();
        let actor = thread::Builder::new()
            .name("codex-tui-pty".into())
            .spawn(move || run_actor(cwd, size, command_rx, event_tx, actor_runtime))
            .context("spawn PTY actor")?;

        Ok(Self {
            command_tx: Some(command_tx),
            event_rx: Some(event_rx),
            actor: Some(actor),
            runtime,
        })
    }

    pub fn send(&self, command: PtyCommand) -> Result<()> {
        let tx = self
            .command_tx
            .as_ref()
            .context("PTY command channel closed")?;
        match tx.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => anyhow::bail!("PTY command queue is full"),
            Err(TrySendError::Disconnected(_)) => anyhow::bail!("PTY command channel closed"),
        }
    }

    pub fn try_recv(&self) -> Option<PtyEvent> {
        self.event_rx.as_ref()?.try_recv().ok()
    }
}

impl Drop for PtyHandle {
    fn drop(&mut self) {
        self.event_rx.take();

        signal_foreground_process_group(&self.runtime.pty_master);

        if let Ok(mut child_killer) = self.runtime.child_killer.lock()
            && let Some(child_killer) = child_killer.as_mut()
        {
            let _ = child_killer.kill();
        }

        if let Some(tx) = self.command_tx.take() {
            let _ = tx.try_send(PtyCommand::Terminate);
            drop(tx);
        }

        // Dropping the JoinHandle detaches the actor so drawer close never blocks the UI.
        drop(self.actor.take());
    }
}

fn run_actor(
    cwd: PathBuf,
    size: TerminalSize,
    command_rx: Receiver<PtyCommand>,
    event_tx: SyncSender<PtyEvent>,
    runtime: PtyRuntime,
) {
    let result = run_actor_inner(&cwd, size, command_rx, &event_tx, &runtime);
    if let Ok(mut master) = runtime.pty_master.lock() {
        master.take();
    }
    if let Err(error) = result {
        let _ = event_tx.send(PtyEvent::Error(format!("{error:#}")));
    }
}

fn run_actor_inner(
    cwd: &Path,
    size: TerminalSize,
    command_rx: Receiver<PtyCommand>,
    event_tx: &SyncSender<PtyEvent>,
    runtime: &PtyRuntime,
) -> Result<()> {
    let cwd =
        std::fs::canonicalize(cwd).with_context(|| format!("resolve PTY cwd {}", cwd.display()))?;
    anyhow::ensure!(
        cwd.is_dir(),
        "PTY cwd is not a directory: {}",
        cwd.display()
    );

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(size.portable())
        .context("open native PTY")?;

    let mut command = CommandBuilder::new_default_prog();
    command.cwd(&cwd);
    spawn_and_drive_pty(
        pair,
        command,
        cwd,
        size,
        command_rx,
        event_tx,
        runtime,
    )
}

fn spawn_and_drive_pty(
    pair: portable_pty::PtyPair,
    command: CommandBuilder,
    cwd: PathBuf,
    size: TerminalSize,
    command_rx: Receiver<PtyCommand>,
    event_tx: &SyncSender<PtyEvent>,
    runtime: &PtyRuntime,
) -> Result<()> {
    let mut child = pair
        .slave
        .spawn_command(command)
        .context("spawn PTY child program")?;
    let mut killer = child.clone_killer();
    if let Ok(mut shared) = runtime.child_killer.lock() {
        *shared = Some(killer.clone_killer());
    }
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().context("clone PTY reader")?;
    let mut writer = pair.master.take_writer().context("take PTY writer")?;
    {
        let mut shared_master = runtime.pty_master
            .lock()
            .map_err(|_| anyhow::anyhow!("PTY master state poisoned"))?;
        *shared_master = Some(pair.master);
    }

    event_tx
        .send(PtyEvent::Ready {
            cwd: cwd.clone(),
            size,
        })
        .context("emit PTY ready")?;

    let output_tx = event_tx.clone();
    thread::Builder::new()
        .name("codex-tui-pty-reader".into())
        .spawn(move || {
            let mut buffer = vec![0_u8; OUTPUT_CHUNK_BYTES];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        let _ = output_tx.send(PtyEvent::ReaderClosed);
                        break;
                    }
                    Ok(count) => {
                        if output_tx
                            .send(PtyEvent::Output(buffer[..count].to_vec()))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ =
                            output_tx.send(PtyEvent::Error(format!("PTY reader failed: {error}")));
                        break;
                    }
                }
            }
        })
        .context("spawn PTY reader")?;

    let exit_tx = event_tx.clone();
    let exit_child_killer = Arc::clone(&runtime.child_killer);
    thread::Builder::new()
        .name("codex-tui-pty-wait".into())
        .spawn(move || {
            let result = child.wait();
            if let Ok(mut shared) = exit_child_killer.lock() {
                shared.take();
            }
            match result {
                Ok(status) => {
                    let _ = exit_tx.send(PtyEvent::Exited {
                        success: status.success(),
                        code: Some(status.exit_code()),
                    });
                }
                Err(error) => {
                    let _ =
                        exit_tx.send(PtyEvent::Error(format!("PTY child wait failed: {error}")));
                }
            }
        })
        .context("spawn PTY waiter")?;

    while let Ok(command) = command_rx.recv() {
        match command {
            PtyCommand::Input(bytes) => {
                writer.write_all(&bytes).context("write PTY input")?;
                writer.flush().context("flush PTY input")?;
            }
            PtyCommand::Resize(size) => {
                let size = size.validate()?;
                let master = runtime.pty_master
                    .lock()
                    .map_err(|_| anyhow::anyhow!("PTY master state poisoned"))?;
                master
                    .as_ref()
                    .context("PTY master unavailable")?
                    .resize(size.portable())
                    .context("resize PTY")?;
            }
            PtyCommand::Terminate => break,
        }
    }

    signal_foreground_process_group(&runtime.pty_master);
    let _ = killer.kill();
    Ok(())
}

fn signal_foreground_process_group(_pty_master: &SharedPtyMaster) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, killpg};
        use nix::unistd::Pid;

        let Ok(master) = _pty_master.try_lock() else {
            return;
        };
        let Some(master) = master.as_ref() else {
            return;
        };
        let Some(process_group) = master.process_group_leader() else {
            return;
        };
        if process_group > 1 {
            // portable-pty's cloned child killer targets the shell itself. A
            // foreground command can live in a separate process group and keep
            // the slave PTY open after the shell exits, so signal that group
            // first and let the reader observe EOF promptly.
            let group = Pid::from_raw(process_group);
            let _ = killpg(group, Signal::SIGHUP);
            let _ = killpg(group, Signal::SIGCONT);
        }
    }
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
        anyhow::ensure!(
            max_bytes > 0,
            "scrollback max_bytes must be greater than zero"
        );
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

    #[cfg(not(windows))]
    #[test]
    fn one_shot_child_emits_output_exit_and_reader_eof() {
        use std::sync::mpsc::sync_channel;
        use std::time::{Duration, Instant};
        use tempfile::tempdir;

        let root = tempdir().expect("tempdir");
        let cwd = std::fs::canonicalize(root.path()).expect("canonical cwd");
        let size = TerminalSize { rows: 24, cols: 80 };
        let pair = native_pty_system()
            .openpty(size.portable())
            .expect("open test PTY");

        let mut command = {
            let mut command = CommandBuilder::new("/bin/sh");
            command.arg("-c");
            command.arg("printf CODEX_TUI_EOF");
            command
        };
        command.cwd(&cwd);

        let (command_tx, command_rx) = sync_channel(4);
        let (event_tx, event_rx) = sync_channel(16);
        let actor = std::thread::spawn({
            let cwd = cwd.clone();
            move || {
                let runtime = PtyRuntime::new();
                spawn_and_drive_pty(
                    pair,
                    command,
                    cwd,
                    size,
                    command_rx,
                    &event_tx,
                    &runtime,
                )
            }
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut exited = false;
        let mut reader_closed = false;
        while Instant::now() < deadline && !(exited && reader_closed) {
            match event_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(PtyEvent::Output(bytes)) => output.extend(bytes),
                Ok(PtyEvent::ReaderClosed) => reader_closed = true,
                Ok(PtyEvent::Exited { .. }) => exited = true,
                Ok(PtyEvent::Error(error)) => panic!("test PTY error: {error}"),
                Ok(PtyEvent::Ready { .. }) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("test PTY event channel failed: {error}"),
            }
        }

        assert!(exited, "one-shot child did not emit Exited");
        assert!(reader_closed, "one-shot child did not close the PTY reader");
        assert!(
            String::from_utf8_lossy(&output).contains("CODEX_TUI_EOF"),
            "missing child output: {:?}",
            String::from_utf8_lossy(&output)
        );

        drop(command_tx);
        actor.join().expect("actor join").expect("actor result");
    }

    #[test]
    fn drop_releases_event_receiver_for_backpressured_actor() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc::sync_channel,
        };

        let (command_tx, _command_rx) = sync_channel(1);
        let (event_tx, event_rx) = sync_channel(1);
        event_tx
            .send(PtyEvent::ReaderClosed)
            .expect("fill event queue");

        let send_unblocked = Arc::new(AtomicBool::new(false));
        let send_unblocked_in_actor = Arc::clone(&send_unblocked);
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let actor = std::thread::spawn(move || {
            let result = event_tx.send(PtyEvent::Error("blocked".into()));
            send_unblocked_in_actor.store(result.is_err(), Ordering::SeqCst);
            let _ = done_tx.send(());
        });

        let handle = PtyHandle {
            command_tx: Some(command_tx),
            event_rx: Some(event_rx),
            actor: Some(actor),
            runtime: PtyRuntime::new(),
        };

        drop(handle);
        done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("backpressured actor should observe receiver teardown");
        assert!(send_unblocked.load(Ordering::SeqCst));
    }

    #[test]
    fn drop_is_nonblocking_and_kills_out_of_band_when_command_queue_is_full() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc::{channel, sync_channel},
        };
        use std::time::Duration;

        #[derive(Debug)]
        struct FlagKiller(Arc<AtomicBool>);

        impl ChildKiller for FlagKiller {
            fn kill(&mut self) -> std::io::Result<()> {
                self.0.store(true, Ordering::SeqCst);
                Ok(())
            }

            fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
                Box::new(Self(Arc::clone(&self.0)))
            }
        }

        let (command_tx, _command_rx) = sync_channel(1);
        command_tx
            .send(PtyCommand::Input(vec![b'x']))
            .expect("fill command queue");
        let (_event_tx, event_rx) = sync_channel(1);

        let killed = Arc::new(AtomicBool::new(false));
        let child_killer: SharedChildKiller =
            Arc::new(Mutex::new(Some(Box::new(FlagKiller(Arc::clone(&killed))))));

        let (release_tx, release_rx) = channel();
        let (done_tx, done_rx) = channel();
        let actor = std::thread::spawn(move || {
            let _ = release_rx.recv();
            let _ = done_tx.send(());
        });

        let handle = PtyHandle {
            command_tx: Some(command_tx),
            event_rx: Some(event_rx),
            actor: Some(actor),
            runtime: PtyRuntime {
                child_killer,
                pty_master: Arc::new(Mutex::new(None)),
            },
        };

        drop(handle);
        assert!(killed.load(Ordering::SeqCst));
        release_tx.send(()).expect("release synthetic actor");
        done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("detached actor should remain independently releasable");
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
