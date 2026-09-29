# codex-tui product positioning update

Date: 2026-09-29
Status: archived follow-up from research round 2

## Product direction

codex-tui should not be positioned as "a prettier Codex chat TUI".

It should be positioned as a Codex-native terminal command center for:

- projects
- worktrees
- conversations / threads
- runtime status
- approvals and attention
- review/diff
- parallel development workflows

The chat surface is one view inside this product, not the product itself.

## Primary user problem

The difficult part of using Codex CLI across real engineering work is control-plane fragmentation:

- many projects
- many conversations
- multiple concurrent runs
- several worktrees/branches
- unclear attention state
- hard resume/navigation
- hard search
- weak cross-session situational awareness

Therefore the default screen should be a mission-control registry rather than a blank composer.

## Core interaction model

Top-level operator-friendly state groups:

- Needs You
- Working
- Ready
- Inactive

Live runtime state and user attention state remain separate.

Example:

- live state: Ready
- attention: unseen completed turn

This prevents completed work from disappearing merely because the thread is now idle.

## Identity model

Three identities must remain distinct:

1. Project identity
2. Codex conversation/thread identity
3. Runtime attachment/load/execution identity

Do not collapse these into one generic "session id".

## Project model

Use upstream Codex Project APIs when available, but capability-negotiate because they are experimental.

Fallback identity order:

1. upstream project id
2. git repository identity
3. normalized cwd
4. explicit local workspace registration

## Worktree policy

Different conversations do not imply filesystem isolation.

For concurrent editing sessions in the same repository, prefer separate worktrees by default and show a visible collision warning when several active editing threads share one mutable root.

## Reference findings

### HachimoDock

Useful conceptual ideas:

- agent/session bus
- bounded active-session queue
- exact-session routing
- manual selection distinct from auto-follow
- normalized lifecycle state
- bounded lifecycle event stream

License boundary:

Current HachimoDock main uses a custom non-commercial source-available license that restricts independent product development. It is conceptual/reference material only unless separate permission is obtained. Do not copy/port its source.

### Agent Deck

MIT. Strong reference for:

- session command center
- groups
- SQLite control-plane state
- worktrees
- lineage
- concurrency policy
- global search

### Workbench

MIT. Strong reference for:

- multiple workspaces
- attention/blocking state
- task/TODO queue
- worktree isolation
- provider transcript recovery
- control socket
- safe ambiguity handling

### agent-manager

Apache-2.0. Strong reference for:

- project/session tree
- high-density rows
- quick prompt
- diff review
- revive on same conversation
- worktree per session

### MulmoTerminal

MIT. Strong reference for:

- conversation/runtime identity separation
- provider transcript instead of terminal screen as durable history
- avoiding duplicate runtimes against one live conversation
- combining git/worktree/session/status views

## Revised delivery order

M0 — Control-plane foundation
- architecture + ADRs
- local SQLite metadata
- Project/Thread/Worktree domain
- SessionRegistry
- mission-control UI
- fake backend
- attention reducer/tests

M1 — Read-only real registry
- initialize
- project/list when supported
- thread/list
- thread/loaded/list
- thread/status/changed
- filtering/search
- aliases/pins/acknowledgement

M2 — Thread interaction
- read/resume
- paginated turns/items
- composer
- start/steer/interrupt
- approvals
- per-thread draft/scroll state

M3 — Parallel workspace management
- worktree discovery
- collision detection
- fork + worktree flows
- git status/diff
- review workspace

M4 — Operator ergonomics
- global search
- command palette
- attention notifications
- tags/collections
- multiple CODEX_HOME profiles
- local/remote app-server targets

M5 — Optional orchestration
- task queues
- parent/child overview
- concurrency caps
- repeatable jobs
- richer agent-to-agent workflows

## Current product definition

codex-tui is a Codex-native terminal command center for projects, worktrees and conversations: see every thread that needs attention, switch instantly, preserve drafts and reading state, isolate parallel edits, review changes, and continue any thread through the official App Server protocol.
