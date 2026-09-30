use codex_tui::pty::{PtyCommand, PtyEvent, PtyHandle, TerminalSize};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;

const WAIT: Duration = Duration::from_secs(8);

fn wait_event(handle: &PtyHandle, predicate: impl Fn(&PtyEvent) -> bool) -> PtyEvent {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(event) = handle.try_recv() {
            if predicate(&event) {
                return event;
            }
            if let PtyEvent::Error(error) = &event {
                panic!("PTY error while waiting: {error}");
            }
        }
        assert!(Instant::now() < deadline, "timed out waiting for PTY event");
        thread::sleep(Duration::from_millis(20));
    }
}

fn collect_until_exit(handle: &PtyHandle) -> (Vec<u8>, bool, Option<u32>) {
    let deadline = Instant::now() + WAIT;
    let mut output = Vec::new();
    loop {
        if let Some(event) = handle.try_recv() {
            match event {
                PtyEvent::Output(bytes) => output.extend(bytes),
                PtyEvent::Exited { success, code } => return (output, success, code),
                PtyEvent::Error(error) => panic!("PTY error: {error}"),
                PtyEvent::Ready { .. } => {}
            }
        }
        assert!(Instant::now() < deadline, "timed out waiting for PTY exit");
        thread::sleep(Duration::from_millis(20));
    }
}

fn line(value: &str) -> Vec<u8> {
    format!("{value}\r").into_bytes()
}

#[test]
fn default_shell_starts_accepts_input_and_exits() {
    let root = tempdir().expect("tempdir");
    let handle =
        PtyHandle::start(root.path(), TerminalSize { rows: 24, cols: 80 }).expect("start PTY");

    let ready = wait_event(&handle, |event| matches!(event, PtyEvent::Ready { .. }));
    match ready {
        PtyEvent::Ready { cwd, size } => {
            assert_eq!(cwd, std::fs::canonicalize(root.path()).expect("canonical"));
            assert_eq!(size, TerminalSize { rows: 24, cols: 80 });
        }
        _ => unreachable!(),
    }

    handle
        .send(PtyCommand::Input(line("echo CODEX_TUI_PTY_READY")))
        .expect("echo");
    handle.send(PtyCommand::Input(line("exit"))).expect("exit");

    let (output, _, _) = collect_until_exit(&handle);
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains("CODEX_TUI_PTY_READY"),
        "missing marker in PTY output: {text:?}"
    );
}

#[test]
fn resize_command_is_accepted_while_shell_is_running() {
    let root = tempdir().expect("tempdir");
    let handle =
        PtyHandle::start(root.path(), TerminalSize { rows: 12, cols: 40 }).expect("start PTY");
    wait_event(&handle, |event| matches!(event, PtyEvent::Ready { .. }));

    handle
        .send(PtyCommand::Resize(TerminalSize {
            rows: 30,
            cols: 120,
        }))
        .expect("resize");
    handle
        .send(PtyCommand::Input(line("echo CODEX_TUI_RESIZED")))
        .expect("echo");
    handle.send(PtyCommand::Input(line("exit"))).expect("exit");

    let (output, _, _) = collect_until_exit(&handle);
    assert!(String::from_utf8_lossy(&output).contains("CODEX_TUI_RESIZED"));
}

#[test]
fn explicit_terminate_stops_a_long_running_child_promptly() {
    let root = tempdir().expect("tempdir");
    let handle =
        PtyHandle::start(root.path(), TerminalSize { rows: 24, cols: 80 }).expect("start PTY");
    wait_event(&handle, |event| matches!(event, PtyEvent::Ready { .. }));

    #[cfg(windows)]
    let long_command = "ping.exe -n 30 127.0.0.1 >NUL";
    #[cfg(not(windows))]
    let long_command = "sleep 30";

    handle
        .send(PtyCommand::Input(line(long_command)))
        .expect("long command");
    thread::sleep(Duration::from_millis(150));

    let started = Instant::now();
    handle.send(PtyCommand::Terminate).expect("terminate");
    let _ = wait_event(&handle, |event| matches!(event, PtyEvent::Exited { .. }));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "explicit terminate took too long"
    );
}

#[test]
fn ctrl_c_is_delivered_to_child_and_shell_remains_usable() {
    let root = tempdir().expect("tempdir");
    let handle =
        PtyHandle::start(root.path(), TerminalSize { rows: 24, cols: 80 }).expect("start PTY");
    wait_event(&handle, |event| matches!(event, PtyEvent::Ready { .. }));

    #[cfg(windows)]
    let long_command = "ping.exe -n 30 127.0.0.1 >NUL";
    #[cfg(not(windows))]
    let long_command = "sleep 30";

    handle
        .send(PtyCommand::Input(line(long_command)))
        .expect("long command");
    thread::sleep(Duration::from_millis(200));
    handle.send(PtyCommand::Input(vec![0x03])).expect("Ctrl-C");
    thread::sleep(Duration::from_millis(100));
    handle
        .send(PtyCommand::Input(line("echo CODEX_TUI_AFTER_CTRL_C")))
        .expect("post Ctrl-C echo");
    handle.send(PtyCommand::Input(line("exit"))).expect("exit");

    let (output, _, _) = collect_until_exit(&handle);
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains("CODEX_TUI_AFTER_CTRL_C"),
        "shell did not remain usable after Ctrl-C: {text:?}"
    );
}

#[test]
fn dropping_handle_cleans_up_actor_without_hanging() {
    let root = tempdir().expect("tempdir");
    let handle =
        PtyHandle::start(root.path(), TerminalSize { rows: 24, cols: 80 }).expect("start PTY");
    wait_event(&handle, |event| matches!(event, PtyEvent::Ready { .. }));

    #[cfg(windows)]
    let long_command = "ping.exe -n 30 127.0.0.1 >NUL";
    #[cfg(not(windows))]
    let long_command = "sleep 30";

    handle
        .send(PtyCommand::Input(line(long_command)))
        .expect("long command");
    thread::sleep(Duration::from_millis(100));

    let started = Instant::now();
    drop(handle);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "PtyHandle drop cleanup took too long"
    );
}
