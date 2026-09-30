# M7c3: PTY lifecycle E2E and cleanup hardening

Status: implementation slice under #31 / #34

M7c3 closes the Terminal Drawer lifecycle with real cross-platform PTY integration tests and explicit terminal-restoration evidence boundaries.

## Automated retained evidence

`tests/pty_lifecycle.rs` runs as part of the canonical:

```bash
cargo test --all-targets --all-features
```

on Ubuntu, macOS and Windows.

The tests use the real `portable-pty/native` backend and the platform default terminal program. They cover:

1. startup emits `Ready` with canonical cwd and requested size;
2. real input reaches the shell and output returns through the PTY;
3. normal shell `exit` produces an `Exited` event;
4. resize is accepted while the shell is live;
5. explicit `Terminate` stops a long-running child promptly;
6. ETX / Ctrl-C interrupts a long-running foreground command and leaves the shell usable;
7. dropping `PtyHandle` terminates and joins the actor without hanging.

The long-running command is platform-specific only where required:

- Unix: `sleep 30`
- Windows: `ping.exe -n 30 127.0.0.1 >NUL`

The actual child program still comes from `portable_pty::CommandBuilder::new_default_prog()`; codex-tui does not pick a shell executable.

## EOF

Normal EOF/child exit is observed through the same reader/wait path as production:

- PTY reader exits on `read == 0`;
- child waiter emits `PtyEvent::Exited`;
- the actor does not treat normal reader EOF as an error.

The startup/echo/exit E2E exercises this normal child-exit path on all retained CI platforms.

## Cleanup guarantees

`PtyHandle::Drop` remains the final local owner boundary:

- attempts `PtyCommand::Terminate`;
- closes the command sender;
- joins the PTY actor;
- the actor kills the child on terminate or command-channel closure.

The Drop E2E runs while a long foreground command is active, which guards against returning while the actor remains stuck.

The Terminal Drawer runtime owns one `PtyHandle`; closing or dropping the Drawer therefore uses the same lifecycle.

## Terminal restoration

`TerminalSession` remains the host-terminal RAII owner:

- enter: raw mode, alternate screen, hidden cursor, optional mouse capture;
- Drop: disable raw mode, disable mouse capture when enabled, leave alternate screen, show cursor.

A real restoration assertion cannot be made honestly from GitHub-hosted CI because those jobs do not provide an interactive controlling TTY with observable user terminal state. M7c3 therefore distinguishes:

- **automated authority:** Rust unit/integration tests and three-platform PTY lifecycle CI;
- **interactive smoke authority:** launch codex-tui from a real terminal, open/close Drawer, quit or force an app error/panic, then verify echo/input/cursor/line editing still work in the parent terminal.

This is an evidence boundary, not a skipped automated claim.

## Interactive smoke procedure

On Linux/macOS/Windows Terminal:

1. start codex-tui in a repository-backed thread;
2. press `t` and verify the Drawer opens in the selected thread cwd;
3. type `echo CODEX_TUI_DRAWER_SMOKE`;
4. resize the host terminal in both dimensions;
5. run a long command, press Ctrl-C, then run another echo;
6. press `Ctrl+]`, confirm codex-tui navigation works while the shell remains alive;
7. press `t` to refocus, then `T` to close the session;
8. quit codex-tui normally;
9. verify the parent terminal has a visible cursor, canonical line editing and echo;
10. repeat once with an induced/panic-style application exit in a development build to verify `TerminalSession::Drop` restoration.

Record platform, terminal emulator, commit SHA and PASS/FAIL when used as release evidence.

## M7c closure

M7c3 does not add:

- persistent shell server;
- detached/background terminal sessions;
- multi-pane terminal multiplexing;
- SSH orchestration;
- command synthesis from model/source metadata.

After M7c3, Terminal Drawer implementation is considered complete enough for M7d compatibility/release hardening.
