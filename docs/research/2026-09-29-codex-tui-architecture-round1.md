# Codex TUI architecture research — round 1

Date: 2026-09-29

## Decision

Build `codex-tui` as an independent rich terminal client over the Codex App Server protocol, not as a fork/skin of `openai/codex`, not as a new agent harness, and not by directly depending on `codex-core`.

Target stack:

- Rust 2024
- Tokio
- Ratatui
- Crossterm
- Serde / serde_json
- tracing
- insta snapshot tests
- PTY end-to-end tests

Core boundary:

```text
Terminal -> UI -> Action/Reducer/State -> Effects -> Domain Model
                                               |
                                               v
                                      AppServer Adapter
                                               |
                               stdio / unix / websocket
                                               |
                                               v
                                      codex app-server
```

## Architecture rules

1. Codex owns agent execution.
2. App Server is the only Codex domain boundary.
3. UI owns presentation, never agent semantics.
4. Every external event becomes an Action.
5. Rendering never performs blocking work.
6. Terminal state always has one owner.
7. Protocol wire types do not leak into widgets.
8. Unknown upstream events fail soft.
9. Codex compatibility is capability/version negotiated; do not bind to one exact CLI version.
10. Transcript and composer are independently navigable.

## Why App Server

Current Codex already exposes the lifecycle that a rich TUI needs:

- initialize / initialized
- thread start, list, read, resume, fork, archive
- turn start, steer, interrupt
- streaming items and deltas
- command/tool/file-change lifecycle
- approvals and sandbox policy
- review
- model/config/MCP surfaces
- persisted sessions

The official Codex TUI itself is Rust + Ratatui + Crossterm + Tokio and increasingly routes behavior through app-server client/protocol boundaries. That validates the stack while also showing why an external project should avoid copying the official monorepo's large internal dependency graph.

## Protocol layer

Use an explicit adapter:

```text
App Server wire
    -> compatibility / raw decoding
    -> codex-tui domain model
    -> reducer/state
    -> UI
```

Recommended transport rollout:

- P0: stdio JSON-RPC/JSONL
- P1: Unix socket
- P2: remote WebSocket when the upstream interface is mature enough

Unknown fields are ignored. Unknown notifications/items are retained as generic/raw events and logged rather than crashing the UI.

## State model

Use Action / Reducer / Effect:

```text
Input or backend event
       -> Action
       -> Reducer
       -> AppState
          |     |
          |     -> Effect -> async work -> Action
          -> Render
```

Reducers stay pure: no RPC, file I/O, process spawning, or terminal mutation.

## Rendering

Do not use an unconditional fixed-tick full redraw.

Use event-driven dirty frames with coalescing and a maximum frame rate. Streaming deltas may coalesce frame requests, but semantic backend event order must never be changed.

Long conversations require transcript virtualization and height/layout caches keyed by item revision, terminal width, expansion state, and theme revision.

## Transcript model

Do not model history as `Vec<String>`.

Use typed timeline items such as:

- UserMessage
- AssistantMessage
- ReasoningSummary
- Command
- FileChange
- ToolCall
- McpCall
- Review
- Warning
- Error
- Notice
- Unknown

Tool output is compact by default and expandable on demand.

## Interaction model

Transcript scrolling and prompt editing are independent.

Recommended viewport state:

- FollowBottom
- Detached(offset)

When detached, new output does not steal the user's reading position; show a visible new-output indicator and allow a one-key return to the bottom.

Keybindings are mapped through command IDs, not hard-coded conditionals. The same registry powers the command palette, help, footer hints, and user configuration.

## Terminal model

Terminal lifecycle is a dedicated authority that owns:

- raw mode
- alternate/main screen transitions
- mouse capture
- bracketed paste
- suspend/resume
- external editor handoff
- panic/signal cleanup
- resize handling

The architecture must allow full-screen and hybrid/main-screen strategies so future copy/paste/native scrollback improvements do not require a rewrite.

## UI structure

Baseline:

```text
project / branch / model / status
--------------------------------
transcript
--------------------------------
composer
--------------------------------
sandbox / approval / tokens / connection
```

Wide terminals may add a contextual right panel for task state, changed files, tools, review, or git context. Narrow layouts collapse to transcript + composer + compact status.

Approval is a first-class UI object that displays operation, command/path/host, reason, sandbox/network escalation, working directory, and decision scope.

## Suggested source tree

```text
src/
  app/
    state.rs
    action.rs
    reducer.rs
    effect.rs
  domain/
  protocol/
    raw.rs
    adapter.rs
    compat.rs
  transport/
    stdio.rs
    unix.rs
    websocket.rs
  tui/
    terminal.rs
    event.rs
    frame.rs
    scrollback.rs
    input.rs
  ui/
    layout.rs
    theme.rs
    keymap.rs
    components/
  features/
    chat/
    sessions/
    approvals/
    review/
    command_palette/
    status/
    model_picker/
```

Start as one crate. Split only after stable boundaries emerge.

## Testing

Three layers:

1. Pure reducer/state tests.
2. Ratatui TestBackend + insta snapshots at multiple widths (including CJK, emoji, long paths, markdown, diffs).
3. Real PTY E2E for raw mode, signals, terminal restore, paste, resize, external editor, approval, scrolling while typing, and process lifecycle.

CI must cover Linux, macOS, and Windows and later include a Codex compatibility matrix rather than one permanently pinned Codex version.

## Delivery stages

### M0 — architecture foundation

- Cargo scaffold
- CLI
- terminal guard
- Action/AppState/Reducer
- FakeBackend
- static transcript/composer
- snapshots
- CI
- ADRs

### M1 — real App Server vertical slice

- spawn `codex app-server`
- initialize
- thread/start
- turn/start
- streaming assistant text
- completion
- interrupt

### M2 — usable daily chat

- multiline composer
- independent scroll
- markdown/syntax
- structured tool items
- approval UI
- status bar
- paste
- external editor

### M3 — sessions and review

- thread list/read/resume/fork/archive
- search/filter/preview/pagination
- review/start
- diff/file navigation

### M4 — terminal hardening

- tmux
- Zellij
- SSH
- Windows Terminal
- VS Code terminal
- mouse/copy
- resize
- hybrid scrollback

### M5 — advanced client

- model/config UI
- command palette/keymap
- goal/plan/compact workflows
- MCP/App surfaces
- richer multi-thread navigation
- Unix/remote app-server support

## Initial ADR set

- ADR-001: App Server is the Codex domain boundary
- ADR-002: Action / Reducer / Effect application architecture
- ADR-003: Protocol types do not leak into UI components
- ADR-004: Compatibility negotiation instead of exact-version binding
- ADR-005: Event-driven/coalesced rendering
- ADR-006: Transcript and composer are independently navigable
- ADR-007: Approval and sandbox are first-class UI state
- ADR-008: Terminal lifecycle has one authority
- ADR-009: Full-screen and hybrid terminal modes are abstracted
- ADR-010: Unknown server events fail soft

## Reference projects reviewed

- OpenAI Codex: https://github.com/openai/codex
- Ratatui: https://github.com/ratatui/ratatui
- Ratatui templates: https://github.com/ratatui/templates
- OpenCode: https://github.com/anomalyco/opencode
- Yazi: https://github.com/sxyazi/yazi
- GitUI: https://github.com/gitui-org/gitui
- Zellij: https://github.com/zellij-org/zellij
- bottom: https://github.com/ClementTsang/bottom

This document captures the first research round. The next round focuses on multi-project / multi-session orchestration, workspace management, session observability, and related products such as HachimoDock.
