# v1 Roadmap

Date: 2026-09-29

The roadmap intentionally optimizes for individual developers first and small teams through existing repository/Git conventions.

## M0 — Static control-plane skeleton

Goal: prove the application architecture without a live model.

Deliver:

- Rust/Ratatui/Crossterm/Tokio scaffold
- Action / Reducer / Effect
- fake Codex backend
- static registry screen
- thread screen
- attention projection
- tiny versioned state file
- terminal restore guard
- snapshot tests
- Linux/macOS/Windows CI

Acceptance:

- app starts/quits cleanly
- fake threads can transition NeedsYou/Working/Ready/Inactive
- selection survives screen switches
- drafts/scroll state are modeled independently
- UI snapshots cover 40/80/120/160 columns

## M1 — Read-only real Codex registry

Goal: solve “where are my Codex sessions?” before sending any prompt.

Deliver:

- spawn/connect App Server
- initialize handshake
- backend fingerprint/capabilities
- thread/list pagination
- thread/status/changed
- thread/loaded/list where supported
- derived workspace grouping
- fuzzy filter
- pin/alias
- attention jump
- doctor codex

Acceptance:

- hundreds of thread metadata rows do not require transcript hydration
- current status updates without polling full histories
- unsupported optional capabilities degrade clearly
- no local rollout scan is needed in the normal path

## M2 — Daily thread interaction

Goal: replace normal multi-terminal Codex navigation.

Deliver:

- thread/read/resume
- paginated turns/items
- composer
- turn/start
- steer
- interrupt
- approval requests/responses
- per-thread draft/scroll restoration
- model/cwd/sandbox/approval footer

Acceptance:

- registry -> exact thread -> prompt -> stream -> completion -> registry is a complete loop
- user can read old output while editing the next prompt
- switching threads preserves unsent drafts
- approval UI shows what is being approved

## M3 — Git and review context

Goal: make parallel development safe enough to understand.

Deliver:

- repo/worktree identity
- branch and dirty state
- changed-file summary
- same-working-tree collision warning
- review/diff view
- external editor/browser open

Acceptance:

- two active editing threads on the same mutable checkout produce a visible warning
- review can be completed without leaving codex-tui for basic diff inspection
- Git inspection never mutates repository state

## M4 — Safe worktree operations

Goal: make parallel implementation convenient, not merely visible.

Deliver:

- create managed worktree
- fork/start thread in worktree
- cleanup
- per-repository mutation serialization
- recovery from partial create/remove
- optional PR/CI read-only summary

Acceptance:

- concurrent worktree mutation cannot contend on one repo index
- cleanup refuses dirty/unsafe deletion
- managed vs user-owned worktrees are distinguishable

## Deferred beyond v1

Do not make these release blockers:

- shared team server/database
- web/mobile client
- Kanban/task database
- generic plugin system
- multi-agent adapters
- agent-to-agent messaging
- workflow/job engine
- containers/sandbox platform
- organization analytics
- cloud sync

## 80% workflow validation

The roadmap is intentionally centered on repeated needs seen across Codex issues and reference tools:

- see all relevant sessions
- identify waiting/finished work
- jump to the exact session
- resume without losing conversation identity
- keep several projects understandable
- avoid concurrent working-tree collisions
- inspect what changed
- approve/steer/interrupt quickly

Features outside this loop should prove demand before entering the core.
