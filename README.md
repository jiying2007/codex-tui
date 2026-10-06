# codex-tui

A local-first terminal workbench for managing multiple Codex projects and conversations.

**License:** Apache-2.0 · **Stable release:** v1.0.0 · **Parked v1.1 candidate:** `release/v1.1-parked` · **Current development line:** v1.4.0 · **Tier 1:** Linux.

## Development status

v1.4 has reached development-scope completion and is frozen against new core functionality under `release/v1.4-completion.json`. It preserves v1.3 Search & Multi-Target and closes the remaining workflow/UX gaps: large Board navigation, WorkCard relationships, thread start/fork handoff, unified metadata search, Saved View editing, Review evidence/external open, bounded long-thread rendering, fuzzy Command Palette, runtime decomposition and exact-SHA 10k Board/Thread render diagnostics. This is not stable readiness: v1.4 stable publication still requires exact-SHA real Linux compatibility, controlling-TTY restoration, performance diagnostics and release evidence defined by `release/v1.4-criteria.json`. GitLab Issue Board membership remains evidence-gated until the native-transport/refresh-budget trigger is satisfied. The v1.3/v1.2 completion checkpoints and parked v1.1 candidate remain historical and separate.

## Audit hardening

The v1.4 line now validates CLI usage before initialization (`--help` exits 0,
malformed usage 2, blocked scoped Doctor 3), moves interactive SQLite and planning
work to bounded workers, and uses consistent online recovery backups. Linux GNU
archives target glibc 2.31 or later and include a source/binary-bound ABI receipt;
other Linux libc variants are not implied. See `docs/release/install-upgrade.md`.

Changed-state diagnostics are available through `release interaction-benchmark`.
`soak --duration-seconds 300 --json` retains same-process resource trends as well as
structural invariants. These are development measurements, not real-terminal
acceptance or demonstrated human-time/token-cost savings. Details and remaining
external evidence are in `docs/implementation/v1.4-audit-closure.md`.

## Product goal

codex-tui answers four questions quickly:

1. What work is in flight?
2. Which Codex thread needs me?
3. How do I jump into the exact conversation and continue?
4. What changed in the working tree?

The chat view is part of the product, but the default product surface is a multi-project thread registry.

## v1 principles

- Codex App Server owns conversation and runtime state.
- Git/worktrees own code state and isolation.
- AGENTS.md and .codex/config.toml own repository-shared team conventions.
- the configured code forge remains the team collaboration/delivery authority; GitLab Self-Managed is the first internal target, with GitHub behind the same forge contract.
- codex-tui owns only local projection, navigation, attention and review UX.
- Derive state whenever possible; persist only small UI metadata.
- Codex only for the initial stable product.
- No codex-tui cloud/service is required.

## v1 core

### Mission Control

- Workspaces grouped from Codex project identity, Git repository identity, or cwd
- Threads grouped into Needs You / Working / Ready / Inactive
- Fuzzy filter/search
- Pin and local alias
- Exact-thread navigation
- Jump to next attention item

### Thread View

- Paginated transcript
- Composer
- Turn start / steer / interrupt
- Approval UI
- Effective model, cwd, sandbox and approval state
- Per-thread draft and scroll restoration

### Git Context

- Repository/worktree identity
- Branch and dirty state
- Changed-file summary
- Warning when concurrent editing threads share one mutable checkout

### Review

- Changed files and diff
- Codex review integration where supported
- Open in external editor/browser

### Diagnostics

- Codex/App Server capability report
- Terminal information
- Config validation
- Clear degraded-mode explanations

## Explicit non-goals for v1

- shared team database
- team presence or RBAC
- web/mobile UI
- canonical custom Kanban/task database or team collaboration server
- general plugin system
- universal coding-agent support
- container/sandbox platform
- remote orchestration service
- agent-to-agent messaging
- workflow/job engine
- cost/accounting platform
- organization analytics

These can be separate optional layers later if real usage justifies them.

## Long-term target

The mature product adds planning without creating a second task authority:

- saved List and Board/Kanban views
- Codex Goal projection
- lightweight local scratch work
- worktree lifecycle and review
- GitLab Work Item/Issue Board/MR/Pipeline projection first; GitHub through the same forge abstraction
- notes/bookmarks, snooze/unread and lightweight notifications
- optional thread queue UI as upstream support stabilizes

Board cards should project existing Codex/Git/forge work whenever possible; Needs You is an attention overlay, not a workflow column.

## Architecture

```text
Codex App Server ───── conversation/runtime authority
        │
        ▼
 Codex compatibility adapter
        │
        ▼
 Session Registry  ◄──── Git metadata
        │
        ├──── Attention projection
        │
        ├──── Planning / Board projection
        │
        └──── tiny local ViewState
        │
        ▼
 Action / Reducer / Effect
        │
        ▼
      Ratatui
```

The v1 line remains a single binary with no codex-tui-owned daemon.

## Running the current implementation

Requirements:

- Rust stable (MSRV 1.88)
- a working `codex` executable on `PATH` for live registry mode
- optional `glab` authenticated to the repository's GitLab host for GitLab integration
- optional `gh` authenticated to the repository's GitHub host for GitHub projection and explicitly confirmed pull-request mutations

Commands:

```bash
cargo run
cargo run -- doctor codex
cargo run -- doctor git
cargo run -- doctor forge
cargo run -- doctor store
cargo run -- doctor compat
cargo run -- doctor compat --json
cargo run -- doctor presets
cargo run -- doctor terminal
cargo run -- doctor bundle --output ./codex-tui-support
cargo run -- headless threads
cargo run -- headless threads --json
cargo run -- headless work
cargo run -- headless work --json
cargo run -- headless status --json
cargo run -- headless attention --json
cargo run -- headless board --json
cargo run -- headless forge --json
cargo run -- headless worktrees --json
cargo run -- status --json
cargo run -- thread list --json
cargo run -- attention list --json
cargo run -- board list --json
cargo run -- forge status --json
cargo run -- worktree list --json
cargo run -- headless threads --fixture-10k
cargo run -- release verify --channel preview --tag v1.4.0-preview.1 --commit "$(git rev-parse HEAD)" --json
cargo run -- release benchmark --iterations 200 --source retained-runner --json
cargo run -- release failure-matrix --json
cargo run -- soak --rows 50000 --cycles 256 --json
cargo run -- --fake
```

### Transcript history search

Press `Ctrl+F` to search conversation history. Current App Server builds are used as
full-history authority through `thread/search` + `thread/searchOccurrences`; a
derived local SQLite index gives immediate fallback results for transcript pages that
codex-tui has already observed. Reasoning/tool-internal content is not written to the
local transcript index.

### Language / 语言

The TUI supports English and Simplified Chinese. The language is selected in the local config printed by `codex-tui doctor`:

```toml
[ui]
mouse = true
language = "auto" # auto | en | zh-CN
presentation = "normal" # normal | quiet | screen-reader

[notifications]
mode = "off" # off | terminal | os

[app_server]
active = "local" # implicit local stdio target; no target table is required
```

`auto` is the default and follows the process locale in standard precedence order: `LC_ALL`, then `LC_MESSAGES`, then `LANG`. Simplified Chinese locales such as `zh_CN.UTF-8`, `zh_SG.UTF-8`, `zh-CN` or `zh-Hans` select Simplified Chinese. Known Traditional Chinese locales such as `zh-TW`, `zh-HK`, `zh-MO` and `zh-Hant` fall back to English rather than being mislabeled as Simplified Chinese. Other or unavailable locales also select English. Use `en` or `zh-CN` to pin the UI language explicitly. Technical identifiers and upstream error text remain unchanged so terminal output still matches Codex/Git/Forge diagnostics.

`presentation = "normal"` preserves the existing render cadence. `quiet` coalesces background-only redraws to at most 10 Hz, while `screen-reader` uses a 500 ms minimum interval for background redraws to reduce repeated terminal updates. Keyboard, paste and terminal-resize interactions remain immediate in every mode. This is a screen-reader-oriented terminal presentation policy, not a second renderer or assistive-technology protocol layer.

Notifications are deliberately lightweight and opt-in. `off` is the default and preserves upgrade behavior. `terminal` emits a terminal bell only. `os` uses the native desktop notification command with a 3-second timeout and falls back to the terminal bell if delivery is unavailable. Notifications are edge-triggered from existing attention/Goal/Forge projections (approval, user input, Goal blocked, completion, pipeline failure, review requested); startup seeds current state without replaying historical alerts, and snooze suppresses routing without changing source status. No prompt text, tool output, credentials, external bridge, scheduler, or second workflow state machine is introduced.

Normal startup selects `[app_server].active` and defaults to the implicit `local` target, which launches `codex app-server --listen stdio://` after the first UI frame. `--target NAME` overrides the active target for one invocation. Mission Control consumes App Server thread lifecycle/status notifications incrementally; the complete paginated registry is retained as an initial load and low-frequency reconciliation path rather than being rebuilt for every notification. Reconciliation requests canonical `recency_at` ordering and use the App Server state DB fast path after the complete initial scan, with automatic fallback for older servers that reject those optional parameters. Goal capability probing is eager only for the most recent 100 threads; older threads refresh Goal state explicitly when opened, avoiding an unbounded startup probe backlog. Mission Control stays metadata-first; opening a thread loads only the recent conversation page. Git checkout projection is deduplicated by exact cwd and the selected/active checkouts are refreshed every 10 seconds so branch/dirty state does not remain frozen after the initial probe. Thread View supports paginated history, persistent local drafts, turn start/steer/interrupt, approvals, user-input requests, and stable Codex Goal projection when the connected App Server supports it.

Personal planning state is stored locally in SQLite: WorkCard relationships/overlays, ScratchWork, Saved Views, notes, bookmarks, snooze and hot slots. Canonical Codex conversations/Goals and Git state are never copied into SQLite.

### Named App Server targets

The default requires no extra configuration. Named targets can point at a different local Codex binary, a WebSocket endpoint, or a Unix socket:

```toml
[app_server]
active = "remote-dev"

[app_server.targets.local-alt]
transport = "stdio"
codex_bin = "/opt/codex/bin/codex"

[app_server.targets.remote-dev]
transport = "websocket"
url = "wss://codex-dev.example.com/rpc"
auth_token_env = "CODEX_DEV_APP_SERVER_TOKEN"

[app_server.targets.local-daemon]
transport = "unix-socket"
path = "/run/user/1000/codex/app-server.sock"
```

Use `codex-tui --target local-alt` for a one-run override, or
`codex-tui doctor codex --target remote-dev` to verify a target. Bearer token
values are never stored in codex-tui config: only the environment-variable name
is retained. Token-bearing `ws://` is accepted only for loopback; use
`wss://` through a TLS proxy for authenticated remote access. Codex's direct
listener supports `ws://IP:PORT` and `unix://`; codex-tui additionally
supports `wss://` as the client side of a TLS-proxied listener.

The backend status source includes the selected target name and transport. Doctor
output prints a sanitized endpoint with WebSocket credentials/query/fragment
removed.

Thread Queue is projected directly from the experimental Codex App Server queue API. Press `q` in Thread View to inspect/add/edit/reorder/start/delete queued submissions. The queue is never persisted locally; older App Servers degrade this surface without blocking normal conversation use. Mixed/multimodal queued inputs can be started/deleted/reordered but are intentionally not text-edited by codex-tui.

`--fake` is a deterministic development/fixture mode; it is never an automatic fallback for a failed real backend.

M6a adds an asynchronous read-only GitLab projection. A normal forge refresh stays within four `glab api` calls (project, recent Issues, open MRs, recent Pipelines); approval/discussion details are loaded only when Review is opened. Forge observations remain derived and carry freshness/provenance. GitLab Issues appear as deduplicated WorkCards, while matching MRs/Pipelines enrich the corresponding Codex thread card. Forge failure never blocks Codex/Git operation.

M6b adds explicit GitLab merge-request mutations from Review / Workspace context actions (`.`): create MR, comment, approve, and merge. Every write is plan-first and requires explicit confirmation. Approve/merge revalidate the exact MR HEAD SHA immediately before execution; merge never requests force/policy bypass. Comment bodies remain memory-only and are not stored in SQLite. Uncertain external outcomes remain `OutcomeUnknown` and are never blindly retried.

M6c established the normalized GitHub read-only provider. v1.3 extends that same forge mutation contract to GitHub pull requests: create, comment, approve, and merge remain plan-first and require explicit confirmation. Approve/merge re-read and compare the exact pull-request HEAD immediately before the write; merge sends only the guarded `sha` and never requests force, bypass, or policy overrides. Comment bodies remain memory-only. Uncertain GitHub outcomes become `OutcomeUnknown` and are reconciled without blind retry. Read projection remains bounded (repository, Issues, open Pull Requests, Actions runs; reviews and bounded GraphQL review threads only in Review).

M7a establishes a read-only automation and scale baseline. v1.2 extends that stable surface with `status`, `thread list`, `attention list`, `board list`, `forge status` and `worktree list` top-level aliases plus equivalent `headless` commands. They emit text or secret-safe JSON snapshots with explicit degraded exit codes and introduce no mutating headless path. `doctor compat` reports local OS/architecture, SQLite, Codex, Git, `glab`, and `gh` compatibility without remote forge API probes. `--fixture-10k` provides deterministic scale data, while the Divan benchmark target measures resident 10k planning filters plus recent/search/host-local Registry projections without turning noisy hosted-runner timings into release gates.

M7b3 adds repository-shared `.codex-tui.toml` launch presets as a deliberately narrow argv-only feature. Presets are selected from Workspace/Review context actions, shown as an exact cwd/argv plan, and require explicit confirmation before codex-tui starts the external process. The config has no shell string, env templating, chaining, hooks or scheduler semantics; common shell executables are rejected.

M7d3 adds a fail-closed release path: publication is manually dispatched, while release-pipeline changes on main automatically run a non-publishing preview self-test. Preview and stable identity derive from the Cargo version; Cargo.lock pins candidates; Linux/macOS/Windows build native archives and smoke the extracted binary; runtime dependency licenses/notices and SHA-256 manifests are retained. v1.0.0 was published as the first stable release on 2026-09-30. The v1.1 candidate is parked on `release/v1.1-parked`; v1.2/v1.3 remain historical development-complete checkpoints; main now carries the v1.4.0 development-complete line. v1.4 preview and stable qualification are bound to the exact source SHA and retain the Failure Matrix, 50k scale-v4 evidence, 50k structural soak, UI command contract, state migration/recovery, support-bundle redaction, protocol replay and dependency-security gates. Exact-SHA 10k Board/Thread render diagnostics are retained with 20 warmups and at least 200 samples as user-perceived performance evidence, but hosted timings remain diagnostic rather than release thresholds. Stable publication additionally requires canonical CI plus real Linux Tier 1 compatibility and controlling-TTY restoration evidence. macOS/Windows remain Tier 2 automated-compatibility targets across the v1 stable line.

## Status

- M0 local control-plane skeleton: implemented and merged.
- M1 read-only Codex thread registry: implemented in the v0.1.x line.
- M2 daily conversation control: implemented in the v0.2.x line.
- M3 Git context and review: implemented in the v0.3.x line.
- M4 personal planning + SQLite: implemented in the v0.4.x line.
- M5 safe managed worktrees: implemented in the v0.5.x line.
- M6a GitLab Self-Managed read-only forge projection: implemented in the v0.6.x line.
- M6b safe explicit GitLab MR mutations: implemented in the v0.6.x line.
- M6c GitHub.com read-only provider: implemented in the v0.6.x line.
- M6 forge integration: complete.
- M7a headless read-only CLI + scale baselines: implemented in the v0.7.x line.
- M7b1 richer SavedView query language: implemented in the v0.7.x line.
- M7b2 safe transactional batch-local actions: implemented with frozen targets, explicit confirmation, and one SQLite transaction.
- M7b3 repository-shared safe launch presets: implemented with argv-only config, explicit plan/confirmation, and no shell/PTY semantics.
- M7c1 bounded cross-platform PTY engine + terminal capability doctor: implemented under the v0.7.x line.
- M7c2 Terminal Drawer UI/input/resize: implemented.
- M7c3 PTY lifecycle/cleanup E2E: implemented across Ubuntu/macOS/Windows, including Windows ConPTY terminal-query handling.
- M7d1 accessibility/CJK/grapheme/keyboard-focus hardening: implemented on the v0.7.x line.
- M7d2 compatibility matrix + Doctor evidence contract: implemented as `compat/v2` with required/optional readiness and retained-evidence metadata.
- M7d3 stable/preview release hardening: implemented with locked three-platform packaging, archive smoke, notices/checksums, and fail-closed stable evidence gates.
- v1.0.0 stable release: published on 2026-09-30 with Linux Tier 1 retained evidence and three-platform native package/archive smoke.
- v1.1.0 candidate: parked on `release/v1.1-parked`; its historical real-environment qualification remains separate.
- v1.2.0 checkpoint: development-scope complete and retained as historical authority; not published as stable.
- v1.3.0 checkpoint: development-scope complete with transcript search, Thread Queue, named App Server targets and safe GitHub mutations; retained as historical authority.
- v1.4.0 development line: P0/P1/P2 workflow-completion scope complete; stable readiness remains externally gated by exact-SHA real Linux compatibility and controlling-TTY evidence.

Architecture and product research are archived under `docs/research/`.
Implementation design lives under `docs/design/`.
Implementation notes live under `docs/implementation/`.

See:

- `docs/design/minimal-core.md`
- `docs/design/core-user-flows.md`
- `docs/design/kanban-and-feature-evaluation.md`
- `docs/design/target-state.md`
- `docs/design/final-plan.md`
- `docs/design/final-implementation-choices.md`
- `docs/roadmap-v1.md`
- `docs/implementation/m0-bootstrap.md`
- `docs/implementation/m1-read-only-registry.md`
- `docs/implementation/m2-daily-conversation-control.md`
- `docs/implementation/m3-git-context-review.md`
- `docs/implementation/m4-personal-planning-sqlite.md`
- `docs/implementation/m5-safe-managed-worktrees.md`
- `docs/implementation/m6-gitlab-readonly-forge.md`
- `docs/implementation/m6b-safe-gitlab-mutations.md`
- `docs/implementation/m6c-github-provider.md`
- `docs/implementation/m7a-headless-scale.md`
- `docs/implementation/m7b1-saved-view-query.md`
- `docs/implementation/m7b2-batch-local.md`
- `docs/implementation/m7b3-launch-presets.md`
- `docs/implementation/m7c1-pty-engine.md`
- `docs/implementation/m7c2-terminal-drawer.md`
- `docs/implementation/m7c3-pty-lifecycle.md`
- `docs/implementation/m7d1-accessibility.md`
- `docs/implementation/m7d2-compatibility.md`
- `docs/implementation/m7d3-release.md`
- `docs/implementation/v1.3-transcript-search.md`
- `docs/implementation/v1.3-thread-queue.md`
- `docs/implementation/v1.3-remote-app-server-targets.md`
- `docs/implementation/v1.3-github-safe-mutations.md`
- `docs/roadmap-v1.4.md`
- `docs/implementation/v1.4-unified-metadata-search.md`
- `docs/implementation/v1.4-long-thread-viewport-cache.md`
- `docs/implementation/v1.4-fuzzy-command-palette.md`
- `docs/implementation/v1.4-review-evidence-external-open.md`
- `docs/implementation/v1.4-core-module-decomposition.md`
- `docs/implementation/v1.2-accessibility-mature-mode.md`
- `docs/roadmap-v1.2.md`
- `docs/release/install-upgrade.md`
- `docs/release/v1.1-rc-plan.md`
- `docs/team-quickstart.md`
- `docs/qualification/provider.md`
- `release/v1.2-plan.json` — v1.2 development plan and phase exits
- `release/v1.2-completion.json` — v1.2 development-scope completion/freeze contract; never stable authority
- `release/v1.2-criteria.json` — v1.2 stable qualification authority
- `release/v1.3-plan.json` — v1.3 Search & Multi-Target development plan
- `release/v1.3-completion.json` — v1.3 development-scope completion/freeze contract; never stable authority
- `release/v1.3-criteria.json` — v1.3 stable qualification authority
- `release/v1.4-plan.json` — v1.4 Workflow Completion Reconciliation plan
- `release/v1.4-completion.json` — v1.4 development-scope completion/freeze contract; never stable authority
- `release/v1.4-criteria.json` — v1.4 stable qualification authority
- `release/v1.1-criteria.json` — historical parked v1.1 qualification authority
- `release/v1.1-rc-plan.json` — historical parked RC freeze/deferred-real-evidence handoff contract
- `release/v1.0-criteria.json` — historical v1.0 release record
- `CHANGELOG.md`

## License

codex-tui is licensed under the Apache License 2.0. See `LICENSE`.

Third-party runtime dependency notices are generated from the locked Cargo dependency graph for release archives.


### Mission Control host-local sessions

Mission Control classifies each Codex thread cwd against the current host:

- `L` / `local`: cwd is a directory that exists on this host; Terminal Drawer is allowed.
- `F` / `foreign-windows` or `foreign-unix`: cwd belongs to another OS; Terminal Drawer is blocked.
- `!` / `stale`: native absolute cwd no longer exists.
- `?`: cwd is empty/relative, or is a native absolute path whose filesystem locality has not been probed yet.

Press `l` in Mission Control to toggle **LOCAL ONLY** without changing the text search. The local-only projection composes with `/` search, so you can keep only host-local sessions visible and still filter by project, title or source. Searching for `local` remains supported.

Mission Control also shows the connected App Server platform, active Codex Home, selected cwd locality and whether the Terminal Drawer is ready, blocked or not yet checked before you press `t`. Normal rendering never performs filesystem probes: native absolute paths outside the locality cache remain `unprobed` until Git/locality reconciliation checks them. Terminal open still performs a fresh fail-closed cwd check.

Codex app-server may normalize a stored Windows cwd while running on Linux, yielding a value such as `/linux/current/dir/C:\\Users\\...`. codex-tui detects the embedded foreign Windows path, displays the Windows portion as foreign, and never uses that value as a Linux PTY cwd.

Run `codex-tui doctor codex` to see the active Codex home plus local/foreign/stale session counts and sample cwd values.

## Linux Tier 1 stable qualification

Release helper scripts require **Python 3.8+**. Use `python3`; do not rely on a `python` alias.

The v1 stable line prioritizes Linux. macOS and Windows remain in canonical CI and native release packaging, but they do not block v1 stable releases on real-environment retained evidence.

After completing the documented real-TTY Terminal Drawer smoke on Linux:

```bash
python3 scripts/release/create_terminal_receipt.py \
  --platform linux \
  --terminal "<your terminal>" \
  --source-sha "$(git rev-parse HEAD)" \
  --pass \
  --output release/evidence/linux/terminal-linux.json

python3 scripts/release/linux_qualify.py \
  --canonical-ci-run <exact-main-ci-run-id> \
  --terminal-receipt release/evidence/linux/terminal-linux.json \
  --source "<retained-linux-machine-id>"

# Add --dispatch to trigger stable + publish=false automatically after local PASS.
```

The second command verifies a clean exact-main SHA, canonical CI, locked tests/build, source-bound READY `compat/v2` plus SHA-256-bound real terminal-restoration receipts, the exact-SHA 50k scale-v4 startup/interaction distributions, 50k structural soak, support-bundle privacy manifest, state/UI/failure gates, a 200-sample 10k performance diagnostic, evidence assembly, and local stable verification. It does **not** publish.
