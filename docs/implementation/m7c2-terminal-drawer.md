# M7c2: Terminal Drawer UI, focused input, and resize

Status: implementation slice under #31 / #33

M7c2 connects the M7c1 PTY engine to a bounded bottom Drawer without changing Codex/Git/Forge authority.

## Screen model

PTY output remains raw bytes inside the PTY engine and is parsed by `vt100 0.16.2`.

The runtime owns:

- `PtyHandle`
- `vt100::Parser`
- current PTY size/cwd/process state

The reducer/UI receives only a cloneable `TerminalSnapshot`:

- canonical cwd
- rows/cols
- visible plain-text rows
- cursor row/column
- scrollback offset
- running/exited/error state

This keeps thread/process resources out of `AppState`.

The parser uses a bounded 2,000-line scrollback. The lower PTY layer remains bounded by the M7c1 event queue/output chunk contract.

## Open/focus/close

Default bindings:

- `t`: open Terminal Drawer for the selected Codex thread cwd, or focus the existing Drawer
- `T`: explicitly close/terminate the Drawer session
- `Ctrl+]`: while Drawer is focused, return keyboard focus to codex-tui without closing the PTY

Drawer open is unavailable when the current view cannot resolve a selected Codex thread cwd (for example local ScratchWork).

Opening starts only the platform default PTY program. No source/model metadata is converted into a command.

## Focused input

When focused, key events bypass the normal codex-tui command map and are encoded as baseline terminal bytes.

Examples:

- printable UTF-8 -> UTF-8 bytes
- Enter -> CR
- Backspace -> DEL
- Esc -> ESC
- arrows/home/end/delete/page keys -> standard ANSI sequences
- Ctrl+C -> ETX (`0x03`) sent to the PTY, not Codex interrupt
- Alt+character -> ESC prefix

Baseline input does not require Kitty/CSI-u.

`Ctrl+]` is reserved as the focus-release chord so ordinary Esc remains usable inside terminal programs.

## Scrollback

While focused:

- Shift+PageUp -> move 10 rows back
- Shift+PageDown -> move 10 rows toward the live screen

Scrollback is a local parser view operation; it does not send page-key bytes to the child when Shift is held.

## Resize

The Drawer occupies a bounded bottom region (roughly 40%, clamped to 7–22 terminal rows).

The PTY dimensions are the Drawer inner rectangle after its border.

On host-terminal resize:

1. recompute Drawer inner rows/cols;
2. send bounded `PtyCommand::Resize`;
3. resize the VT screen model;
4. refresh the projected snapshot.

Initial PTY size is computed from the current Crossterm terminal dimensions when the Drawer opens.

## Cursor

When the Drawer is focused, Ratatui `Frame::set_cursor_position` places the real host cursor at the parsed VT cursor position within the Drawer.

When unfocused the normal Ratatui behavior hides the cursor unless another app editor owns it.

## Rendering

M7c2 renders the VT parser's visible semantic text rows into Ratatui. ANSI control sequences are never rendered as text.

Foreground/background/style mapping from VT cells is optional polish and is not required for the lifecycle/input/resize correctness contract in this slice.

## Explicitly deferred to M7c3

- retained real PTY startup/exit smoke evidence
- Ctrl-C child lifecycle verification
- EOF behavior
- explicit terminate/wait timing
- panic/drop cleanup evidence
- terminal restoration E2E

M7c2 does not create a terminal daemon, multiplexer, SSH orchestrator, or background job service.
