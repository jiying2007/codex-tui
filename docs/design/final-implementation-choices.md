# Final implementation choices

Date: 2026-09-29
Status: Accepted implementation baseline

This document closes the remaining implementation choices before M0/M1.

## 1. LocalStore format

Decision:

- Human-edited configuration: TOML.
- Machine-written local state: JSON.
- Initial files:
  - config.toml
  - state-v1.json

Rationale:

TOML is better for comments and hand-edited preferences. JSON is better for atomic machine-written structured state, nested maps keyed by IDs, schema migration, diagnostics and serde compatibility.

Rules:

- UI never edits config.toml by text surgery; parse and rewrite through a configuration model.
- state-v1.json is not public configuration.
- state writes are atomic: write temp file, fsync where practical, rename.
- include schemaVersion.
- LocalStore hides file shape from application/domain code.
- no secrets/tokens are stored.

## 2. Default keymap

Decision:

Use terminal-safe, context-aware defaults and avoid Alt-heavy or modifier combinations known to be swallowed by terminal/OS layouts.

Global:

- Ctrl+C: quit / interrupt current blocking UI action according to context; at root, quit
- ?: help
- Ctrl+K: command palette
- /: search/filter
- Esc: back/cancel
- Tab / BackTab: next/previous focus or primary pane
- .: context actions for selected item

Mission Control / Board:

- j / Down: next item
- k / Up: previous item
- Enter: open selected item/thread
- Space: jump to next unsnoozed attention item
- a: Quick Prompt ("ask")
- b: Board / Mission Control toggle
- r: Review
- w: Workspace
- s: snooze selected item
- u: mark unread
- 1..9: jump to hot slot
- = then 1..9: bind selected item to hot slot
- 0: jump to most-recent previous target
- n: new local Scratch / new thread via context menu choice
- q: quit only from top-level/root view

Thread:

- Esc: return to previous view
- Ctrl+C: interrupt active turn when one is running; otherwise leave/cancel according to context
- a: focus composer / Quick Prompt semantics are not active inside the full Thread view
- r: open Review
- w: open Workspace
- g: Goal actions
- PageUp/PageDown: transcript paging
- Home/End: viewport top/bottom where terminal reports them reliably

Review:

- j/k or arrows: move file/finding
- Enter: open/expand
- w: toggle word-diff
- e: external editor
- o: open forge/browser target
- Esc: back

Board:

- h/l or Left/Right: previous/next workflow column
- j/k or Up/Down: move card
- Enter: open card
- a: Quick Prompt if linked Thread exists
- Space: next attention item
- .: context menu
- no direct drag-like mutation is bound to a single letter; workflow-changing actions go through context/command palette and explicit plans where needed

Keymap architecture:

Raw event -> normalized KeyChord -> Command ID -> Action.

Defaults are not compatibility promises forever; Command IDs are the stable semantic surface.

Avoid as required defaults:

- Ctrl+Space
- Shift+Space
- Alt+number
- OS-specific Command/Super chords

## 3. Syntax highlighting

Decision:

Use two-face as the syntax/theme dataset on top of syntect.

Regex backend:

Use syntect's pure-Rust fancy-regex path rather than the native Oniguruma path for the default distributed build.

Rationale:

- two-face carries the broader bat-curated syntax/theme set, including common languages missing from minimal syntect defaults;
- pure-Rust regex reduces native-library/distribution friction across Linux/macOS/Windows;
- highlighting is not on the critical input path and can be cached/background-computed, so the debug/performance trade-off is acceptable.

Implementation rules:

- isolate highlighting behind SyntaxHighlighter;
- cache by content revision + syntax + theme;
- never highlight multi-MB output synchronously on render;
- plain-text fallback is always available;
- benchmark before enabling highlighting for very large files/diffs.

## 4. Diff crate

Decision:

Use similar, not diffy, as the in-process presentation diff engine.

Rationale:

- similar supports line and character/word-oriented diffing and multiple algorithms;
- it is a better fit for Review UI and intra-line highlighting;
- it has a small dependency footprint;
- Git remains authoritative for repository diff generation.

Boundary:

- repository diff source: git diff / GitService;
- display parsing and intra-line highlighting: similar;
- do not use an in-process diff crate as a replacement for Git semantics.

Initial algorithm:

- line diff: Myers default;
- intra-line/word emphasis: token/character diff with Unicode segmentation where useful;
- consider patience only for measured readability wins on specific review cases.

## 5. SQLite introduction

Decision:

Introduce SQLite in v0.4.0, aligned with M4 Personal Planning.

Version mapping:

- v0.1.x: M0 + M1 — architecture + read-only real registry
- v0.2.x: M2 — daily conversation control
- v0.3.x: M3 — Git + Review
- v0.4.x: M4 — WorkCard + Board/List + Saved Views + Scratch + relationships; migrate LocalStore to SQLite
- v0.5.x: M5 — safe managed worktrees
- v0.6.x: M6 — GitLab Self-Managed forge integration
- v0.7.x: M7 — scale/polish/headless stabilization
- v1.0.0: stable product after compatibility/performance hardening

Why v0.4.0:

Before M4 the state is small enough for atomic JSON. M4 introduces relational data (WorkCard anchor/links, Saved Views, ScratchWork, receipts) where SQLite provides real value.

Migration:

- config remains TOML;
- state-v1.json is imported transactionally into SQLite;
- keep a backup of the source state file;
- canonical Codex/Git/Forge data is never migrated into local authority.

## 6. glab -> native GitLab transport threshold

Decision:

Keep glab/glab api as the default GitLab transport through v0.6.x unless one of the following hard triggers is met.

Hard triggers:

1. A normal user-visible forge refresh cannot be implemented in <= 4 glab subprocesses after batching/pagination work.
2. Warm-cache interactive forge refresh p95 exceeds 500 ms and tracing shows >= 150 ms of that latency is local glab process startup/serialization overhead rather than GitLab network/server time.
3. A required stable GitLab capability cannot be accessed reliably through glab/glab api structured output.
4. We need sustained incremental/event-style synchronization where subprocess-per-refresh becomes structurally inefficient.

Measurement rule:

- measure on Linux and Windows, because process spawn cost differs materially;
- require repeatable benchmark evidence before adding native transport;
- native transport is added behind the existing ForgeProvider and does not replace glab auth ownership by default.

Until a trigger is met, native REST/GraphQL is maintenance cost without sufficient user value.

## 7. Internal GitLab capability matrix

Decision:

Do not hard-code a compile-time version table as authority.

Use runtime capability discovery with version/tier recorded for diagnostics.

At M6a, Doctor captures:

- GitLab host
- server version
- edition/tier when discoverable
- project numeric ID
- glab version
- authenticated host
- capability probes

Baseline capabilities:

| Capability | Baseline |
| --- | --- |
| Repository/project resolution | Required |
| Issues | Required |
| Merge Requests | Required |
| Pipelines | Required |
| Issue Boards | Required for Board projection; graceful unavailable if instance disables it |
| MR approval summary | Capability-gated |
| MR discussions/unresolved threads | Capability-gated |
| Work Items GraphQL | Experimental/optional, not baseline |
| Group-level board extras | Optional |
| Premium/Ultimate-only approval features | Tier-gated |

Capability source precedence:

1. explicit endpoint/command probe;
2. API response shape/availability;
3. version/tier inference only when probing is not practical.

Support policy:

- internal production GitLab version discovered during M6a becomes the first retained integration fixture;
- also retain fixtures for one older compatible version and current GitLab docs/schema shape where practical;
- upgrade compatibility is tested by provider contract, not by scattered version checks.

## 8. Terminal Drawer timing

Decision:

Terminal Drawer moves after M6, into v0.7.x / M7 polish.

It is not part of M5.

Rationale:

- it adds PTY/terminal-emulation/input-routing complexity;
- users already have terminals/tmux/Zellij;
- it is convenient but not required for the core planning/control/review loop;
- GitLab integration has higher immediate value for the actual internal environment;
- delaying it protects the terminal core from becoming a multiplexer project too early.

Before Terminal Drawer exists:

- open external terminal/editor actions are sufficient;
- Workspace view can expose useful commands and copy/run hints without embedding a PTY.

## 9. Performance gates

Decision:

Use explicit service-level engineering budgets rather than a single arbitrary "40 vs 50 ms" number.

Interactive latency:

- key/input -> rendered response p95 <= 50 ms
- key/input -> rendered response p99 <= 100 ms

Render cost:

- normal 120x40 frame render p95 <= 16 ms
- large common 160x60 frame render p95 <= 33 ms
- streaming redraw is coalesced and capped at 60 FPS maximum; no redraw tick while idle

Registry/search:

- 10,000 metadata rows remain navigable
- metadata filter/search p95 <= 50 ms after data is resident

Startup:

- first local shell/UI frame <= 200 ms on a normal developer machine, excluding external Codex startup
- after a backend metadata response is received, first registry render <= 100 ms

I/O:

- no unbounded subprocess/network probe on startup
- optional Git/Forge probes do not block the first usable UI
- every external subprocess has an explicit timeout/cancellation policy

Performance gates become CI/regression gates only after a stable benchmark harness establishes repeatable baselines. A regression of >20% in a mature benchmark requires explicit review even if the absolute SLO is still met.

## Final choices summary

- Config: TOML.
- Runtime state: JSON initially.
- Keymap: Vim-like navigation + Space attention + a Quick Prompt + Ctrl+K palette + context-safe commands.
- Syntax: two-face + syntect/fancy-regex.
- Diff: similar; Git remains authoritative.
- SQLite: v0.4.0 / M4.
- Native GitLab transport: only after measured glab trigger.
- GitLab compatibility: runtime capability probing, internal version retained as fixture.
- Terminal Drawer: v0.7.x / after M6.
- Performance: p95 50 ms interaction, p99 100 ms, explicit render/startup/search budgets.
