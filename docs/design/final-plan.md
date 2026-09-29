# codex-tui 终版方案

Date: 2026-09-29
Status: Final design baseline
Audience: individual developers and small engineering teams

## 1. Product definition

codex-tui is a local-first Codex engineering workbench.

It unifies:

- planning
- multi-project / multi-thread visibility
- human attention routing
- exact thread navigation and control
- Git/worktree context
- review/diff
- lightweight team integrations

The product is not "a prettier Codex chat TUI".

The product goal is to reduce operator overhead when a developer has many projects, many Codex threads and several pieces of work moving at the same time.

The mature product should answer, within a few keystrokes:

1. What work exists?
2. What is running?
3. What needs me?
4. Which exact Codex thread should I enter?
5. What changed in the code?
6. What is ready for review?
7. What is blocked?
8. What is complete?
9. Which issue/PR/worktree belongs to this work?
10. Why is a session or environment not working?

## 2. Target users

### Individual developer

Typical environment:

- several repositories
- several active Codex threads
- parallel experiments
- Git worktrees
- many terminal windows today
- frequent resume / review / approval operations

Primary value:

- one place to see all active work
- one-key attention routing
- exact conversation resume
- drafts/scroll preserved per thread
- safe parallel worktree awareness
- fast review

### Small engineering team

Typical environment:

- shared Git repositories
- GitHub Issues / Projects / PR / CI
- shared AGENTS.md and project Codex config
- each engineer has their own local Codex sessions

Primary value:

- consistent local operator experience
- team work projected from GitHub / repo state
- no new mandatory collaboration server
- local state remains personal
- shared work remains in existing team systems

## 3. Product principles

### P1 — Existing systems remain authoritative

Codex App Server owns:

- thread identity
- thread history
- turns/items
- thread runtime status
- model/provider
- approvals
- sandbox/permission state
- goals
- queue where supported

Git owns:

- repository identity
- branches
- worktrees
- dirty state
- diffs
- commits

GitHub owns:

- issues
- pull requests
- CI
- team review/delivery state

Repository configuration owns:

- AGENTS.md
- .codex/config.toml
- project skills/plugins where appropriate

codex-tui owns:

- presentation
- local attention acknowledgement
- local draft
- local alias
- local pin
- local snooze
- local saved views
- local scratch work
- local navigation state

No duplicate canonical transcript database.

### P2 — Derive before persisting

If a value can be reliably derived from Codex/Git/GitHub, do not persist a second copy as authority.

### P3 — Local-first core

The core must work as a local terminal application without:

- codex-tui cloud
- mandatory web server
- central team database
- RBAC server
- Postgres
- mobile companion

### P4 — Deep Codex integration before broad agent support

Codex is the first-class backend.

Do not turn the core into a universal terminal-scraping agent manager.

### P5 — Capability over exact version

Codex version is evidence, not the main feature switch.

Each feature declares:

- preferred capability
- fallback
- degraded behavior

### P6 — Every compatibility promise has a maintenance budget

Any stable config/API/integration must define:

- support scope
- migration path
- fallback
- tests
- deprecation strategy
- recurring CI cost

## 4. Final product layers

### Layer 1 — Codex Control Core

Always present.

Capabilities:

- App Server connection
- capability negotiation
- project/workspace discovery
- thread registry
- thread status
- Attention Inbox
- conversation view
- approvals
- start/steer/interrupt
- resume/fork
- goals
- model/sandbox/permission visibility
- diagnostics

### Layer 2 — Developer Workspace

Always present in the mature product.

Capabilities:

- Git repository identity
- worktree identity
- branch/dirty state
- worktree collision detection
- managed worktree lifecycle
- changed files
- diff/review
- external editor/browser
- lightweight terminal drawer
- command palette
- global metadata search
- notifications

### Layer 3 — Planning

Mature core.

Capabilities:

- List view
- Board/Kanban view
- Saved Views
- Codex Goal projection
- local Scratch work
- priorities
- pins
- snooze
- mark unread
- review queue
- optional thread queue UI as upstream support stabilizes

Planning is a view layer, not a second Jira.

### Layer 4 — Team Integrations

Optional but expected for small-team use.

Capabilities:

- GitHub Issue projection
- GitHub Projects projection
- PR state
- CI state
- review state
- issue/thread/worktree/PR links
- repository-shared launch/review presets only when Codex/Git cannot express them

No codex-tui team server is required.

### Layer 5 — Remote / Collaboration

Optional separate layer.

Possible capabilities:

- explicit remote App Server target
- SSH-aware open
- browser/mobile companion
- shared presence
- shared live notes
- external notification bridge

The local core must never depend on this layer.

### Layer 6 — Extensions

Late optional layer.

Preferred evolution:

1. custom commands/actions
2. stable local control protocol
3. declarative integrations
4. only then a versioned plugin runtime if real demand exists

Internal Rust domain types are never the public plugin API.

## 5. Core domain model

The core model should stay small.

### Workspace

Purpose:

Group related Codex threads by technical project identity.

Identity resolution order:

1. stable native Codex project id when available
2. Git repository identity
3. normalized cwd
4. explicit local registration as fallback

Fields:

- id
- display_name
- roots
- primary_root
- repo_identity optional
- upstream_project_id optional
- recency

### ThreadRef

Represents one Codex thread.

Fields:

- thread_id
- workspace_id
- name
- preview
- cwd
- status
- active_flags
- archived
- recency
- model/provider when known
- goal summary when known
- branch/worktree metadata derived from Git
- PR/issue references when known

### Attention

Separate from execution status.

Execution state examples:

- active
- idle
- not loaded
- system error

Attention examples:

- approval required
- input required
- blocked goal
- error
- usage/budget limit
- completed but unseen
- review required

Attention projection:

- Needs You
- Working
- Ready
- Inactive

### Goal

Use Codex-native Goal when available.

Relevant state:

- objective
- active
- paused
- blocked
- usage limited
- budget limited
- complete
- token/time budget information

Goal remains owned by Codex.

### WorkCard

Unified planning projection.

A WorkCard can reference:

- Codex Thread
- Codex Goal
- GitHub Issue
- GitHub PR
- worktree/branch
- local Scratch item

Example:

~~~text
WorkCard
  title: Fix decoder boundary regression
  issue: GH-213
  thread: 01...
  goal: validate single-variable decoder boundary strategy
  worktree: wt/decoder-boundary
  branch: research/decoder-boundary
  pr: #392
  derived_stage: Review
~~~

The WorkCard is not the canonical task record.

### ViewState

Local-only metadata:

- selected workspace
- selected thread/card
- per-thread draft
- per-thread scroll/follow
- pins
- aliases
- snooze
- seen/unseen state
- last opened time
- hot slots
- saved view selection

### ScratchWork

Tiny local planning item.

Fields only:

- id
- title
- note
- workspace optional
- priority optional
- state: inbox / ready / done
- linked thread optional
- linked issue optional

No:

- RBAC
- assignee system
- comments system
- dependency graph
- team synchronization

If it becomes team work, promote/link it to GitHub Issue/Project.

## 6. Primary product views

### Mission Control

Question:

What needs me now?

Content:

- workspace grouping
- threads
- attention grouping
- recency
- goal summary
- branch/worktree hint
- PR/CI hint
- pins/snooze/unread

Default ordering:

1. Needs You
2. Working
3. Review
4. Ready
5. Inactive

Primary keys/actions:

- move
- open thread
- jump next attention
- quick prompt
- filter
- pin
- snooze
- mark unread
- create/fork
- command palette

### Board / Planning

Question:

What stage is each piece of work in?

Default columns:

- Inbox
- Ready
- Working
- Needs You
- Review
- Done

Derived-state examples:

- active thread -> Working
- approval/input request -> Needs You
- Goal blocked/usage-limited/budget-limited -> Needs You
- Goal complete + code changes -> Review
- open PR needing review -> Review
- merged PR or explicit completion -> Done

Manual drag is allowed only when the target mutation has a clear authority.

Never let drag silently contradict Codex/GitHub state.

### Thread

Question:

What is this Codex thread doing, and what should I tell it next?

Content:

- paginated transcript
- structured tool timeline
- composer
- approvals
- Goal status
- model
- cwd
- sandbox/permission
- branch/worktree
- token/context information
- draft state
- scroll state

Important rule:

Transcript viewport and composer are independent.

### Review

Question:

What changed, and is it ready?

Content:

- changed files
- diff
- word-diff option for long-line files
- Codex review findings
- tests/checks summary
- PR/CI state when available
- open editor/browser
- return to thread

### Workspace

Question:

Where is this work happening?

Content:

- repo identity
- worktrees
- branch
- dirty state
- active threads by worktree
- collision warning
- managed worktree actions
- dev terminal drawer
- open editor

### Search / Command Palette

Question:

How do I reach anything quickly?

Search dimensions:

- workspace
- thread
- alias
- goal objective
- issue/PR
- branch/worktree
- status
- stage
- tags/pins

Command IDs, not raw key conditionals, drive actions.

### Doctor

Question:

Why is this not working?

Report:

- codex-tui version
- Codex path/version
- App Server fingerprint
- CODEX_HOME
- platform
- capabilities
- terminal identity
- tmux/Zellij/SSH/WSL detection
- Git readiness
- config validation
- local state validation
- recent adapter errors

Secrets and prompt content are redacted by default.

## 7. Kanban / Board design

Kanban is part of the mature core, but as a derived view.

### Why it is useful

It connects:

- planning
- execution
- human intervention
- review
- completion

It is especially useful when work exists before a Codex thread exists.

### Why it must not become a second task authority

A full task authority would require:

- task IDs
- statuses
- users
- assignees
- dependencies
- comments
- history
- permissions
- team sync
- conflict resolution

That duplicates GitHub/Linear/Jira.

### Saved Views

Board and List are two layouts over the same WorkCards.

SavedView fields:

- name
- source scope
- filter
- grouping
- ordering
- layout
- visible fields

Examples:

- Attention
- Today
- KWS
- Audio
- Review
- Ready to merge
- Recently completed
- Board

## 8. Attention model

Attention must remain separate from workflow state.

Examples:

- Thread state Working + approval -> Needs You attention
- Thread state Ready + unseen completion -> attention
- Review stage + unread finding -> attention
- Working + snooze -> still Working, but temporarily omitted from attention rotation

Snooze is not a fake status.

Suggested attention fields:

- kind
- priority
- created_at
- seen_at
- snooze_until
- source revision

## 9. Quick Prompt

Allow sending a short message to the selected thread without entering full Thread view.

Use case:

~~~text
Registry
  -> quick prompt
  "tests pass then prepare PR summary"
  -> remain in Registry
~~~

This is high-value for parallel session control.

## 10. Notes, bookmarks and hot slots

### Notes

Small personal note per thread/work card.

Use cases:

- why a path was rejected
- hardware data pending
- next manual action
- important commit

### Bookmarks

Bookmark a specific thread event/turn/item for later return.

### Hot slots

Map 0-9 or another key scheme to frequently used threads.

Example:

- 1 = KWS decoder
- 2 = Audio i025
- 3 = Engineering Platform

This is intentionally local UI state.

## 11. Worktree strategy

### Awareness first

Always detect:

- repo
- worktree
- branch
- dirty state

### Collision warning

If multiple active editing threads share one mutable checkout:

~~~text
! 2 active editing threads share /repo/main
~~~

### Managed worktrees

Mature core can:

- create
- fork into worktree
- launch thread
- inspect
- remove safely

Safety rules:

- serialize Git mutations per repository
- distinguish user-owned vs codex-tui-managed worktrees
- refuse dirty destructive cleanup
- recover partial creation/removal
- never assume cwd equals repo root

## 12. GitHub integration

For small teams, GitHub is the team source of truth.

Read/projection capabilities:

- Issues
- Project items
- PR
- CI/checks
- review state
- merge readiness
- unresolved review threads

Relationship:

~~~text
Issue
  -> WorkCard
      -> Codex Thread
      -> Worktree
      -> PR
~~~

Mutating GitHub state should always be explicit and attributable.

No silent state changes from purely visual operations.

## 13. Team model

Normal small-team topology:

~~~text
Shared Git repository / GitHub Project
      |
      +-- AGENTS.md
      +-- .codex/config.toml
      +-- Issues / PR / CI
      |
Developer A codex-tui
Developer B codex-tui
Developer C codex-tui
~~~

Each developer keeps local:

- drafts
- pins
- saved personal views
- snooze
- hot slots
- scroll position
- attention acknowledgement

Shared state remains in repo/GitHub.

## 14. Persistence strategy

### Early stages

Use:

- config.toml
- versioned atomic local state file

### Mature product

Introduce SQLite only when justified by:

- Saved Views
- ScratchWork
- optional transcript FTS
- richer local relationships
- concurrent local clients

Even with SQLite:

- transcript remains Codex-owned
- Git state remains Git-owned
- team task state remains GitHub-owned

Database must support:

- schema version
- migrations
- backup
- integrity checks
- recovery

## 15. Technical stack

Recommended:

- Rust 2024
- Tokio
- Ratatui
- Crossterm
- Serde / serde_json
- Clap
- tracing
- thiserror
- color-eyre or equivalent
- textwrap
- unicode-width
- unicode-segmentation
- pulldown-cmark
- syntect/two-face or equivalent
- diffy or equivalent
- insta
- pretty_assertions

Start as one Cargo crate.

Split crates only after independent compile/API boundaries prove valuable.

## 16. Application architecture

Use:

~~~text
External Event
   -> Action
   -> Reducer
   -> AppState
      -> Render
      -> Effect
          -> async I/O
          -> Action
~~~

Reducers perform no:

- RPC
- filesystem I/O
- process spawn
- Git mutation
- terminal mutation

Rendering performs no blocking work.

## 17. Backend architecture

### CodexBackend

Internal trait/concept:

- initialize
- capabilities
- list threads
- read/resume/fork thread
- list turns/items
- start/steer/interrupt turn
- approvals
- Goal get/set
- queue operations when supported
- review
- event stream

Implementations:

- AppServerBackend
- FakeBackend
- ReplayBackend

UI never depends on raw wire types.

### Compatibility adapter

Layers:

~~~text
Wire JSON-RPC
  -> raw/compat decode
  -> normalized Codex domain
  -> registry/app state
~~~

Unknown fields/events fail soft.

## 18. Capability strategy

Each feature declares:

- stable baseline requirement
- preferred capability
- fallback
- unavailable behavior

Examples:

### Workspace

Preferred:
- native project identity

Fallback:
- Git repo identity
- cwd

### Goal

Preferred:
- thread Goal APIs

Fallback:
- no Goal controls; use thread preview/attention only

### Queue

Preferred:
- thread queue APIs

Fallback:
- local quick prompt only

### Project search

Preferred:
- upstream search

Fallback:
- metadata fuzzy search

Experimental upstream APIs never become hard startup dependencies.

## 19. Terminal architecture

Dedicated subsystem:

- identity
- capabilities
- keyboard
- screen
- palette
- mouse
- clipboard
- probes
- quirks

All external probes:

- timeout
- cancellation
- tracing
- fallback

No probe may block the UI startup indefinitely.

## 20. Keymap architecture

Pipeline:

~~~text
Raw terminal event
  -> normalized KeyChord
  -> Command ID
  -> Action
~~~

Command registry drives:

- behavior
- command palette
- help
- footer hints
- user remapping

No widget-local hard-coded key semantics for stable commands.

## 21. Rendering and performance

Event-driven rendering.

Do not fixed-tick redraw the entire TUI continuously.

Coalesce visual updates.

Do not reorder semantic backend events.

### Long transcript

Use virtualized items.

Cache layout height based on:

- item id
- item revision
- terminal width
- expanded state
- theme revision

Large tool output:

- truncate preview
- lazy expand
- bounded memory
- do not pre-render multi-MB text

### Large registry

Startup loads metadata only.

Thread turns/items are lazy.

Target scale:

- 10k thread metadata rows remain navigable
- multi-MB tool output does not block input
- input-to-frame p95 target under roughly 50 ms under normal load
- optional probes do not block usable UI

## 22. Accessibility

Required principles:

- no state only by color
- static alternative to animation
- reduced-motion setting
- no Nerd Font dependency
- keyboard-first
- CJK/emoji/combining tests

Potential mature mode:

- screen-reader-friendly presentation
- simplified redraw behavior
- no shimmer/spinner animation

## 23. Security

### Approvals

Display:

- operation
- command/path/host
- reason
- cwd
- network/filesystem escalation
- decision scope

### External commands

- no shell-string concatenation
- explicit argv
- explicit cwd
- timeout/cancellation where safe

### Local control socket

If added later:

- local-user only
- versioned
- explicit methods
- no credentials
- no internal Action enum exposure

### Logging

Default logs exclude:

- prompt plaintext
- tool output
- credentials
- environment secrets

## 24. Testing strategy

### Pure tests

- reducers
- attention
- WorkCard stage derivation
- Saved Views
- capability negotiation
- project identity
- worktree policy
- keymap
- migrations

### Protocol fixtures

Test supported Codex schema/payload generations.

### Snapshot

Widths:

- 40
- 80
- 120
- 160

Content:

- CJK
- combining marks
- emoji
- long paths
- Markdown
- approvals
- diff
- board
- large tool output

### PTY E2E

- startup/exit
- Ctrl-C
- panic cleanup
- resize
- paste
- mouse
- editor handoff
- tmux
- Zellij
- WSL/Windows cases where CI permits

### Real Codex compatibility

Scheduled matrix:

- minimum supported
- previous stable
- current stable
- optional prerelease

Avoid paid live inference for basic protocol certification.

## 25. Diagnostics

Commands:

- codex-tui doctor
- codex-tui doctor --json
- codex-tui doctor codex
- codex-tui doctor terminal
- codex-tui doctor git
- codex-tui config validate

Doctor is part of the product, not post-launch support tooling.

## 26. Release engineering

Targets:

- Linux x86_64 GNU
- Linux x86_64 musl
- macOS arm64
- macOS x86_64
- Windows x86_64
- Linux arm64 when practical

Release assets:

- archives
- checksums
- installer scripts

Later:

- Homebrew
- winget/scoop/etc based on demand

Channels:

- stable
- preview

Preview can validate upcoming Codex protocol changes.

## 27. Dependency and supply-chain policy

Early automation:

- cargo-deny
- cargo-audit / RustSec
- Dependabot/Renovate
- GitHub Actions minimal permissions
- Actions static analysis

Reference license registry:

- source repository
- license
- concepts used
- whether code reuse is allowed

HachimoDock remains conceptual reference under its current custom license.

AGPL projects remain architectural references unless obligations are intentionally accepted.

## 28. Feature tiers

### Mature Core

- Mission Control
- Attention Inbox
- Thread conversation/control
- Goal
- Board/List Saved Views
- Quick Prompt
- Review/Diff
- Git/worktree awareness
- worktree lifecycle
- Search/Palette
- Pins/Aliases/Snooze/Unread
- Notes/Bookmarks
- Hot slots
- lightweight notifications
- Doctor

### Strong additions

- GitHub Issue/Projects
- PR/CI/review
- Local ScratchWork
- terminal drawer
- batch actions
- launch presets
- thread queue when stable
- optional transcript FTS
- token/Goal budget visibility

### Optional layers

- web/mobile
- remote collaboration
- multi-agent
- agent-to-agent messaging
- generic plugin runtime
- job/workflow engine
- organization analytics
- cost/accounting platform

## 29. Explicit non-goals, including target state

Even the mature product should not become:

- its own model runtime
- its own agent loop
- its own sandbox/policy engine
- a replacement for Git
- a replacement for GitHub/Linear/Jira
- an enterprise identity/RBAC server
- a cloud transcript warehouse
- a universal coding-agent terminal shim

## 30. Delivery roadmap to target state

### M0 — Static control-plane skeleton

- Rust scaffold
- Action/Reducer/Effect
- FakeBackend
- registry
- thread view
- attention
- local state abstraction
- terminal guard
- snapshots
- CI

### M1 — Read-only real registry

- App Server initialize
- capability fingerprint
- thread/list
- status notifications
- loaded threads
- workspace grouping
- filter/search
- pin/alias
- doctor

### M2 — Daily thread interaction

- read/resume
- paginated turns/items
- composer
- start/steer/interrupt
- approvals
- per-thread draft/scroll

### M3 — Git + review

- repo/worktree
- branch/dirty
- collision warnings
- changed files
- diff/review
- editor/browser

### M4 — Planning foundation

- Goal projection
- WorkCard
- Board/List
- Saved Views
- local ScratchWork
- snooze/unread
- quick prompt
- notes/bookmarks
- hot slots

### M5 — Managed parallel development

- worktree create/fork/cleanup
- mutation serialization
- recovery
- richer review
- notifications
- terminal drawer

### M6 — Small-team integration

- GitHub Issue
- GitHub Projects
- PR/CI/review state
- issue-thread-worktree-PR linking
- repo-shared presets where justified

### M7 — Scale and polish

- optional transcript FTS
- batch actions
- launch presets
- performance hardening
- accessibility hardening
- compatibility matrix
- stable/preview channels

### Optional post-core tracks

- remote App Server
- web/mobile companion
- collaboration service
- plugin runtime
- multi-agent adapters
- jobs/workflows

These tracks never become prerequisites for the local core.

## 31. Success criteria

The product is successful when an individual developer with many active repositories can replace a collection of terminal windows and remembered thread IDs with one reliable terminal workflow.

A small team should be able to adopt it without introducing a new shared service.

The mature interaction loop should be:

~~~text
Plan
 -> Board / Goal
 -> Start or resume Codex thread
 -> Work in isolated workspace
 -> Attention when human input is needed
 -> Review code and findings
 -> PR / CI
 -> Done
~~~

At every stage:

- Codex remains the agent authority
- Git remains the code authority
- GitHub remains the team delivery authority
- codex-tui remains the operator workbench

## 32. Final positioning

codex-tui is the terminal-native operating surface for Codex engineering work.

Its differentiator is not "more AI".

Its differentiator is lower operator overhead across planning, many projects, many threads, parallel worktrees, human attention and review — while preserving clear system boundaries and low long-term maintenance cost.
