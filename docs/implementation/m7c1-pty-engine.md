# M7c1: PTY engine and terminal capability contract

Status: implementation slice under #31 / #32

M7c1 introduces the cross-platform PTY engine beneath the future Terminal Drawer. It deliberately does not add Drawer UI or terminal-focused key routing yet.

## Backend

The engine uses `portable-pty 0.9` and its native platform backend.

Properties:

- single codex-tui process; no terminal daemon;
- platform default program via `CommandBuilder::new_default_prog()`;
- explicit canonical cwd;
- reader thread for PTY output;
- child-wait thread for exit status;
- control actor for input, resize and terminate;
- child killer split from the blocking wait path;
- PTY slave dropped after child spawn;
- master writer held explicitly so EOF/lifetime semantics remain controlled.

## Bounded memory / backpressure

The event queue is a bounded sync channel with capacity 64.

Output is read in chunks of at most 8 KiB and sent through that bounded queue. If the UI/runtime stops consuming events, the reader applies backpressure instead of growing memory without bound.

The provided `BoundedScrollback`:

- defaults to 2 MiB;
- supports a caller-selected positive bound;
- evicts complete oldest chunks when over budget;
- trims an oversized individual chunk to its bounded tail;
- counts truncated bytes.

M7c2 will layer terminal-screen parsing/rendering over this bounded byte stream.

## Commands

`PtyCommand` supports only:

- `Input(Vec<u8>)`
- `Resize(TerminalSize)`
- `Terminate`

Sizes with zero rows or columns are rejected.

## Events

`PtyEvent` emits:

- `Ready { cwd, size }`
- `Output(bytes)`
- `Exited { success, code }`
- `Error(message)`

The engine itself does not reinterpret terminal output and does not synthesize commands.

## Lifetime

Dropping `PtyHandle`:

1. sends `Terminate`;
2. the control actor invokes the child killer;
3. the control actor is joined.

Reader/wait helper threads are not promoted to a persistent service. Closing the receiver after the handle drops releases blocked bounded-channel sends.

M7c3 will add retained lifecycle E2E specifically for EOF, Ctrl-C, panic/Drop cleanup and terminal restoration.

## Doctor

```bash
codex-tui doctor terminal
```

reports:

- backend;
- platform;
- default-program support;
- resize support;
- input support;
- event queue bound;
- default scrollback bound.

It does not start a shell.

## Deferred to M7c2

- bottom Drawer layout;
- terminal focus/input routing;
- PTY resize from Ratatui layout;
- ANSI/terminal-screen parsing;
- scrollback navigation;
- open/close/toggle key binding.

## Deferred to M7c3

- retained startup/exit E2E;
- Ctrl-C behavior;
- explicit terminate verification;
- EOF behavior;
- panic/drop cleanup;
- cross-platform retained smoke evidence.
