# Multi-project / multi-session control-plane research

Date: 2026-09-29
Status: research round 2

## Executive decision

Multi-project / multi-session management is a P0 product capability for codex-tui, not an optional session picker.

The problem to solve is control-plane fragmentation:

- many repositories and projects
- many sessions per project
- several sessions active at once
- same repository edited by multiple agents
- hard to see which session is working, idle, waiting for approval/input, failed, or finished
- hard to jump to the correct conversation and preserve local UI state
- hard to distinguish stored history from a live/running thread
- hard to find old sessions by project, title, branch, cwd, or prompt
- hard to manage worktrees and parallel tasks
- hard to see effective model/sandbox/approval policy before acting

Target hierarchy:

~~~text
Profile / Codex Home / App-server endpoint
  └─ Project / Workspace
      ├─ Worktree / execution root
      │   ├─ Thread / Session
      │   └─ Thread / Session
      └─ Worktree / execution root
          └─ Thread / Session
~~~

The chat surface is one view inside the product. The product itself is a Codex-native engineering command center.

## 1. Current upstream Codex capabilities

Current openai/codex already exposes enough App Server primitives to build this properly:

- thread/list
- thread/loaded/list
- thread/read
- thread/resume
- thread/fork
- thread/turns/list
- thread/items/list
- thread/search and thread/searchOccurrences where supported
- thread/status/changed
- persisted thread sections
- project list/read/create/import/update/move/delete where supported

Current thread status includes notLoaded, idle, systemError, and active with active flags.

The official Codex TUI itself now has an Agents Overview. It projects upstream status into operator-friendly groups:

- Needs input: waiting on approval/user input or system error
- Working: other active threads
- Ready: idle
- Inactive: not loaded

Current upstream Project data also has stable id, human name, roots, metadata, position, timestamps and project recency. Project APIs are currently experimental, so codex-tui must capability-negotiate rather than depend on them.

Fallback project identity:

1. upstream project id when available
2. git repository identity / common dir plus relative cwd
3. normalized cwd root
4. explicit locally registered workspace

## 2. Three identities must never be conflated

### Project identity

Stable workspace/project membership.

### Conversation identity

Canonical Codex thread id used by read/resume/fork/turn operations.

### Runtime identity

Whether and where the thread is loaded, subscribed, and actively executing.

Persisted history does not prove that a live process exists. A recent rollout mtime is not a reliable liveness signal.

## 3. Normalized domain model

### Profile

Represents one Codex environment.

Suggested fields:

- id
- label
- codex_home
- app_server_target
- codex_version
- protocol_capabilities
- health
- last_seen

Future profiles can represent another CODEX_HOME/account, a known daemon, or a remote app-server endpoint. Baseline ships with one local profile.

### Project

Suggested fields:

- local_id
- upstream_project_id optional
- name
- roots
- repo_identity optional
- primary_root
- default_cwd
- position
- recency_at
- tags
- policy_id optional

### Worktree / ExecutionRoot

Suggested fields:

- id
- project_id
- path
- branch
- base_branch optional
- git_sha optional
- dirty
- owner_thread_id optional
- lifecycle
- created_at

### ThreadRef

Suggested fields:

- thread_id
- profile_id
- project_id
- cwd
- worktree_id optional
- upstream_name optional
- local_alias optional
- preview
- model/provider
- effective permission/sandbox/approval state
- status and active flags
- source kind
- parent/fork lineage
- timestamps and recency
- archived/pinned
- attention acknowledgement state
- ui_last_opened_at

Codex remains authoritative for conversation content and runtime state. Local state only enriches the UI.

## 4. Local control-plane database

Use SQLite for codex-tui-owned metadata.

Suggested tables:

~~~text
profiles
projects
project_roots
worktrees
session_ui
session_tags
drafts
attention_events
ui_state
recent_targets
~~~

Codex owns canonical thread/history/turns/items/status/model/runtime settings/permissions/archive state.

codex-tui owns local aliases, pinning, tags/collections, ordering, drafts, scroll/focus state, acknowledgement/dismiss state, and locally managed worktree metadata.

Do not mirror the full Codex transcript into a second canonical database.

## 5. Mission-control UI

Default landing screen should be the session registry, not a blank composer.

~~~text
┌ Projects ──────────┬ Session registry ───────────────────────┬ Context ─────────┐
│ All                │ NEEDS YOU                               │ repo / branch     │
│ project-a      3   │ ! auth-refactor  approval        1m    │ worktree          │
│ project-b      7   │ ! flaky-ci      waiting input    4m    │ diff              │
│ project-c      2   │                                         │ model              │
│                    │ WORKING                                 │ sandbox            │
│ Pinned             │ ● kws-decoder   running          12m    │ approvals          │
│ Recent             │ ● audio-i025    running           8m    │ context/tokens     │
│ Archived           │                                         │                   │
│                    │ READY / RECENT                          │                   │
│                    │ ○ docs-cleanup  idle             20m    │                   │
└────────────────────┴─────────────────────────────────────────┴───────────────────┘
~~~

Entering a thread opens transcript + composer + effective context. Going back restores registry selection/filter/scroll.

## 6. Attention Inbox

Live status and user attention are two different state machines.

Normalized live status:

- NeedsYou
- Working
- Ready
- Inactive

Attention events can include:

- approval requested
- user input requested
- system error
- turn completed but unseen

Example: a thread may already be Ready while still carrying an unseen completed-turn notification. Visiting or acknowledging it clears the attention event without changing live status.

This avoids missed completions and repeated noisy alerts.

## 7. HachimoDock findings

HachimoDock is a useful conceptual reference for session observability and routing.

Useful concepts:

### Agent session bus

It normalizes multiple agent backends behind a common interface for availability, session list, active resolution, open-new, input injection and lifecycle events.

Its Codex adapter resumes existing Desktop threads through App Server and maps Codex deltas/tool events into a generic event stream.

### Bounded active queue

HachimoDock intentionally presents a small queue of operationally relevant sessions instead of all history.

codex-tui should similarly distinguish:

- Full Session Registry: searchable history
- Attention Queue: current operational work

### Exact-session routing

Explicit session selection should target exact thread identity rather than guessing latest.

### Manual selection versus auto-follow

Refreshing background state must not steal the user's selected thread. Baseline rule for codex-tui:

Manual selection never changes automatically.

An explicit follow-attention mode can be added separately.

### Status projection

Agent-specific states are projected into user-facing lifecycle states. codex-tui should do this using official ThreadStatus/activeFlags whenever possible.

### Bounded event log

Cursor/reset semantics are useful for synchronizing lightweight local lifecycle changes.

### Local rollout discovery

HachimoDock carefully scans Codex session files/indexes with bounded recursion, caching, filtering of internal/non-resumable rollouts, and model metadata.

For codex-tui this is a compatibility/recovery fallback only.

Primary discovery order:

~~~text
App Server thread/project/status APIs
        ↓
Codex local storage fallback/recovery
~~~

### License boundary

Current HachimoDock main uses a custom non-commercial source-available license and requires separate permission for independent product development.

Therefore codex-tui may study its product concepts and behavior, but must not copy/port its source implementation or use it as a dependency without separate permission.

## 8. Other reference projects

### Agent Deck — Tier A

License: MIT.

Most useful ideas:

- mission control for many agent sessions
- status groups and global search
- project/group organization
- fork and worktree isolation
- persistent SQLite control-plane state
- parent/child session lineage
- group concurrency caps and default path/policy
- explicit distinction between mutable display title, policy-bearing group, and lineage

Important principle:

A group should exist because policy differs, not merely because a topic name differs.

### Workbench — Tier A

License: MIT.

Very close to the desired shape:

- multiple workspaces
- multiple agents/terminals per workspace
- task/TODO queue
- live status and blocked-on-user state
- worktree isolation
- transcript reconstruction from provider logs
- control socket and published state snapshot
- peer roster and safe ambiguity rejection

Useful principle:

Never guess an ambiguous session target; show candidates and require exact selection.

### agent-manager — Tier A

License: Apache-2.0.

Useful ideas:

- foldable project tree
- high-density one-line session rows
- quick prompt without attaching
- inline focus while list remains visible
- kill/revive on the same conversation
- diff review as a first-class panel
- optional worktree per session
- comfortable/fullscreen density modes

### Claude Squad — Tier B

License: AGPL-3.0.

Useful architectural pattern:

- one worktree per parallel task
- persistent background sessions
- preview + diff
- attach/detach/pause/resume

For codex-tui, worktree isolation is the main lesson; tmux should not become our primary Codex runtime because App Server already gives a structured native control plane.

### MulmoTerminal — Tier A

License: MIT.

Important lessons:

- conversation id and terminal/runtime id are not the same
- durable conversation history should come from provider transcript/rollout, not terminal screen
- avoid launching a second backend against an already-live conversation
- combine worktree, git/diff, status and conversation views

### Smaller session managers — Tier B

Useful for simple MVP patterns:

- project -> sessions hierarchy
- favorites/pins
- search
- multiple CODEX_HOME discovery
- title fallback chain
- status indicators
- live preview

## 9. Worktrees are essential

Multiple conversations in one repository are not isolated code execution.

Default rules:

- two editing sessions: separate worktrees unless explicitly overridden
- read/review sessions: may share a root when not modifying files
- forks can choose same-worktree conversation experiment or new-worktree implementation experiment

Show collision risk prominently:

~~~text
⚠ 2 active editing sessions share /repo/main
~~~

Never imply that different conversations automatically mean isolated files.

## 10. Project vs Group vs Lineage

Do not overload one hierarchy.

Project = technical workspace identity.

Group/Collection = user organization or policy bucket, potentially carrying max concurrency, default model, permissions, worktree rule and notification policy.

Parent/Fork lineage = execution/task derivation.

This keeps navigation stable and machine identity unambiguous.

## 11. Global search

One search surface should cover:

- project
- session alias/name
- thread preview
- cwd
- branch/worktree
- status
- model
- tags
- prompt/content search where upstream supports it

Examples:

~~~text
status:needs-you
project:audio
branch:research/i025
cwd:kws-pipeline
thread:decoder
~~~

## 12. Preserve per-thread UI state

Persist at least:

- unsent draft
- transcript scroll anchor
- follow-bottom state
- tool expansion state
- selected context tab
- alias/tags
- last selected timeline item
- last opened time

Thread switching should feel like changing editor buffers, not restarting an application.

## 13. SessionRegistry architecture

Introduce a dedicated WorkspaceIndex / SessionRegistry.

~~~text
App Server
  ├─ project/list
  ├─ thread/list
  ├─ thread/loaded/list
  ├─ thread/status/changed
  └─ thread/project updates
          │
          v
 SessionRegistry
   ├─ normalized projects
   ├─ normalized threads
   ├─ live status
   ├─ attention events
   └─ lazy metadata cache
          │
          v
       AppState
~~~

Bootstrap lazily, subscribe to notifications, fetch details only for visible/selected/active rows, and page turns/items only when a thread is entered.

Local file polling is fallback, not the primary event mechanism.

## 14. Known upstream failure modes

Design around these explicitly:

- stored thread is not the same as running thread
- loaded thread is not necessarily another UI's foreground selection
- history projection can become stale/damaged even while turns continue
- generated display names are not stable machine identifiers
- experimental Project APIs may not exist on every supported version

The UI should surface freshness/errors rather than silently presenting stale state as current.

## 15. Revised roadmap

The previous research placed session management too late. Revised order:

### M0 — control-plane foundation

- architecture and ADRs
- local SQLite metadata DB
- Project/Session/Worktree domain model
- FakeBackend
- SessionRegistry
- static mission-control UI
- attention reducer/tests

### M1 — read-only real registry

- initialize
- project/list when supported
- thread/list pagination
- thread/loaded/list
- thread/status/changed
- search/filter/sort
- project fallback grouping
- alias/pin/ack

No agent turn creation is required to prove this milestone.

### M2 — thread interaction

- thread/read/resume
- turns/items paging
- composer
- turn/start/steer/interrupt
- approvals
- per-thread draft/scroll state

### M3 — parallel workspace management

- worktree discovery
- collision detection
- fork + worktree flows
- git status/diff
- review workspace

### M4 — operator ergonomics

- command palette
- global search
- attention notifications
- tags/collections/policy groups
- multiple CODEX_HOME profiles
- local/remote app-server targets

### M5 — optional orchestration

- task queues
- parent/child overview
- concurrency caps
- repeatable jobs
- richer agent-to-agent workflows

Orchestration must not be a prerequisite for a reliable multi-session client.

## 16. New ADRs

- ADR-011: Multi-project/session management is a first-class control plane.
- ADR-012: Project, Codex thread, and runtime attachment are distinct identities.
- ADR-013: Native App Server inventory/status is primary; local rollout scanning is fallback only.
- ADR-014: Local SQLite stores UI/control metadata, never canonical conversation history.
- ADR-015: Concurrent editing sessions use worktree isolation by default.
- ADR-016: Live status and user attention/acknowledgement are separate state machines.
- ADR-017: Project, collection/group, and thread lineage are separate concepts.
- ADR-018: Manual session selection never changes automatically.
- ADR-019: HachimoDock is conceptual reference only unless separate licensing permission is obtained.

## 17. Reference priority

Tier A — use deeply:

- OpenAI Codex App Server + official TUI
- Agent Deck (MIT)
- Workbench (MIT)
- agent-manager (Apache-2.0)
- MulmoTerminal (MIT)
- Ratatui official patterns

Tier B — use selectively:

- Claude Squad (AGPL-3.0)
- smaller agent-session-manager projects

Tier C — conceptual only:

- HachimoDock current main due to its non-commercial source-available license for independent product development

## Final product definition

codex-tui should be:

A Codex-native terminal command center for projects, worktrees and conversations: see every thread that needs attention, switch instantly, preserve drafts and reading state, isolate parallel edits, review changes, and continue any thread through the official App Server protocol.

Chat is one view inside that product, not the product itself.
