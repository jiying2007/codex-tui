# M7c3: PTY lifecycle E2E and cleanup hardening

Status: implementation slice under #31 / #34

M7c3 closes the Terminal Drawer lifecycle with real cross-platform PTY integration tests and explicit terminal-restoration evidence boundaries.

## Automated retained evidence

`tests/pty_lifecycle.rs` runs as part of the canonical:

```bash
cargo test --all-targets --all-features
```

on Ubuntu, macOS and Windows.

The integration tests use the real `portable-pty/native` backend and the platform default terminal program. They cover:

1. startup emits `Ready` with canonical cwd and requested size;
2. real input reaches the shell and output returns through the PTY;
3. resize is accepted while the shell is live;
4. explicit `Terminate` stops a long-running child promptly;
5. ETX / Ctrl-C interrupts a long-running foreground command and leaves the shell usable;
6. dropping `PtyHandle` terminates and joins the actor without hanging.

A separate unit test uses the same native PTY driver with a deterministic one-shot child program and verifies output, natural child exit, reader EOF, and `PtyEvent::Exited`. This avoids treating shell-specific `exit` command syntax as part of the PTY contract.

The long-running command is platform-specific only where required:

- Unix: `sleep 30`
- Windows: `ping.exe -n 30 127.0.0.1 >NUL`

Production and integration-test terminal sessions still come from `portable_pty::CommandBuilder::new_default_prog()`; codex-tui does not pick a shell executable for the Drawer. The deterministic one-shot program exists only inside the private PTY unit test.

## Windows ConPTY terminal queries

Windows ConPTY may emit terminal-device queries before the default shell becomes interactive. In retained CI this surfaced as `ESC[6n` (DSR cursor-position report request). The Drawer now responds as a terminal emulator rather than treating these bytes as display-only output:

- `ESC[5n` -> `ESC[0n`;
- `ESC[6n` -> `ESC[row;colR` using the current VT cursor position;
- query recognition keeps a three-byte tail so an escape sequence split across PTY output chunks is still answered.

The Windows PTY integration tests reuse this production response helper, so the CI path validates the same protocol behavior used by the Drawer.

## EOF

Normal EOF/child exit is observed through the same reader/wait path as production:

- PTY reader emits an explicit `PtyEvent::ReaderClosed` on `read == 0`;
- child waiter emits `PtyEvent::Exited`;
- normal reader EOF is not reported as an error.

All three platform integration suites require both `ReaderClosed` and `Exited` after explicit PTY termination. Unix additionally has a deterministic one-shot native-PTY test that proves natural child exit leads to reader EOF without explicit termination. Windows ConPTY couples natural child completion to pseudoconsole lifetime differently, so its portable authority is the explicit teardown boundary rather than shell-specific `exit` timing.

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
