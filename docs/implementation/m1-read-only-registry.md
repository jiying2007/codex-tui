# M1 read-only Codex App Server registry

Date: 2026-09-29
Status: Implemented

M1 turns the M0 fixture registry into the first useful personal workflow surface backed by the real Codex App Server.

## Delivered

- local stdio App Server launch: `codex app-server --listen stdio://`;
- `initialize` / `initialized` handshake;
- backend version/platform/CODEX_HOME diagnostics;
- paginated `thread/list` without full-history hydration;
- optional `thread/loaded/list` capability probe;
- incremental `thread/status/changed` handling;
- preservation of notifications that arrive while a request is in flight;
- bounded 5-second RPC requests;
- exact Codex thread ID as conversation identity;
- official status semantics:
  - Active + waiting-on-approval/user-input -> Needs You;
  - Active -> Working;
  - Idle -> Ready;
  - SystemError -> Needs You / ERROR;
  - NotLoaded -> Inactive;
- deterministic workspace fallback:
  1. Codex project ID;
  2. Git origin identity;
  3. cwd;
- sticky manual selection across background refresh/reordering;
- metadata fuzzy filter that matches within individual fields rather than across field boundaries;
- local-only pin, alias, unread and attention acknowledgement;
- actionable approval/user-input/system-error attention cannot be hidden by acknowledgement;
- degraded/offline backend markers;
- `codex-tui doctor codex`;
- explicit `--fake` fixture mode;
- dirty-driven rendering: idle loops do not redraw continuously.

## Authority boundary

M1 does not persist canonical conversation/runtime data. Codex App Server remains authoritative for thread identity and runtime state.

LocalStore persists only operator metadata:

- per-thread draft/scroll/follow;
- pins;
- aliases;
- marked-unread;
- local acknowledgement state.

M1 does not scan `~/.codex` rollout files as a normal data path.

## Failure model

- optional `thread/loaded/list` absence degrades gracefully;
- unknown notifications fail soft;
- failed App Server startup shows an offline/degraded registry rather than silently substituting fake data;
- long-lived App Server lifetime is expected, but individual RPC requests are bounded;
- dropping the registry task drops the child process with `kill_on_drop`.

## Verification

CI runs on Linux, macOS and Windows with:

- `cargo fmt --check`;
- Clippy with warnings denied;
- all-target/all-feature tests.

Tests cover protocol normalization, workspace fallback, exact identity, selection stability, filter behavior, local-state round-trips, responsive layouts and operator-state semantics.

## Next slice

M2 adds daily conversation control: metadata-only thread read/resume, paginated turns/items, composer, turn start/steer/interrupt, approvals, user input, per-thread view state and Quick Prompt.
