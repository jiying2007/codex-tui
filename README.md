# codex-tui

A local-first terminal workbench for managing multiple Codex projects and conversations.

**License:** Apache-2.0 · **Current package line:** v1.0.0 release candidate (stable not yet published) · **Tier 1:** Linux.

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

The first release should remain a single binary with no codex-tui-owned daemon.

## Running the current implementation

Requirements:

- Rust stable (MSRV 1.88)
- a working `codex` executable on `PATH` for live registry mode
- optional `glab` authenticated to the repository's GitLab host for GitLab integration
- optional `gh` authenticated to `github.com` for the M6c GitHub read-only provider

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
cargo run -- headless threads
cargo run -- headless threads --json
cargo run -- headless work
cargo run -- headless work --json
cargo run -- headless threads --fixture-10k
cargo run -- release verify --channel preview --tag v1.0.0-preview.1 --commit "$(git rev-parse HEAD)" --json
cargo run -- release benchmark --iterations 200 --source retained-runner --json
cargo run -- --fake
```

Normal startup launches a local `codex app-server --listen stdio://` connection after the first UI frame. Mission Control stays metadata-first; opening a thread loads only the recent conversation page. Thread View supports paginated history, persistent local drafts, turn start/steer/interrupt, approvals, user-input requests, and stable Codex Goal projection when the connected App Server supports it.

Personal planning state is stored locally in SQLite: WorkCard relationships/overlays, ScratchWork, Saved Views, notes, bookmarks, snooze and hot slots. Canonical Codex conversations/Goals and Git state are never copied into SQLite.

`--fake` is a deterministic development/fixture mode; it is never an automatic fallback for a failed real backend.

M6a adds an asynchronous read-only GitLab projection. A normal forge refresh stays within four `glab api` calls (project, recent Issues, open MRs, recent Pipelines); approval/discussion details are loaded only when Review is opened. Forge observations remain derived and carry freshness/provenance. GitLab Issues appear as deduplicated WorkCards, while matching MRs/Pipelines enrich the corresponding Codex thread card. Forge failure never blocks Codex/Git operation.

M6b adds explicit GitLab merge-request mutations from Review / Workspace context actions (`.`): create MR, comment, approve, and merge. Every write is plan-first and requires explicit confirmation. Approve/merge revalidate the exact MR HEAD SHA immediately before execution; merge never requests force/policy bypass. Comment bodies remain memory-only and are not stored in SQLite. Uncertain external outcomes remain `OutcomeUnknown` and are never blindly retried.

M6c completes the normalized forge layer with a GitHub.com read-only provider. Exact `github.com` remotes route through authenticated `gh`; other hosts remain GitLab-first so internal GitLab refreshes pay no provider-detection probe. A normal GitHub refresh uses four `gh api` calls (repository, Issues, open Pull Requests, Actions runs), while reviews and bounded GraphQL review threads load only in Review. GitHub capabilities degrade independently and no GitHub write path is introduced.

M7a establishes a read-only automation and scale baseline. `headless threads` and `headless work` emit stable text or secret-safe JSON snapshots with explicit degraded exit codes. `doctor compat` reports local OS/architecture, SQLite, Codex, Git, `glab`, and `gh` compatibility without remote forge API probes. `--fixture-10k` provides deterministic scale data, while the Divan benchmark target measures resident 10k planning filters without turning noisy hosted-runner timings into release gates.

M7b3 adds repository-shared `.codex-tui.toml` launch presets as a deliberately narrow argv-only feature. Presets are selected from Workspace/Review context actions, shown as an exact cwd/argv plan, and require explicit confirmation before codex-tui starts the external process. The config has no shell string, env templating, chaining, hooks or scheduler semantics; common shell executables are rejected.

M7d3 adds a fail-closed release path: publication is manually dispatched, while release-pipeline changes on main automatically run a non-publishing preview self-test. Preview and stable identity derive from the Cargo version; Cargo.lock pins candidates; Linux/macOS/Windows build native archives and smoke the extracted binary; runtime dependency licenses/notices and SHA-256 manifests are retained. The project is now Apache-2.0 licensed and the package line is v1.0.0. Stable release keeps canonical CI and native package/archive smoke on Linux/macOS/Windows, but real retained compatibility, terminal-restoration and performance evidence are required only on Linux Tier 1. macOS/Windows are Tier 2 automated-compatibility targets for v1.0. Linux performance evidence requires at least 200 samples, p95 <= 50 ms and p99 <= 100 ms.

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
- v1.0 release candidate: Apache-2.0 license selected; package line advanced to 1.0.0; stable publication still awaits retained cross-platform compatibility, terminal-restoration and performance evidence.

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
- `docs/release/install-upgrade.md`
- `release/v1.0-criteria.json`
- `CHANGELOG.md`

## License

codex-tui is licensed under the Apache License 2.0. See `LICENSE`.

Third-party runtime dependency notices are generated from the locked Cargo dependency graph for release archives.

## Linux Tier 1 stable qualification

Release helper scripts require **Python 3.8+**. Use `python3`; do not rely on a `python` alias.

v1.0 stable prioritizes Linux. macOS and Windows remain in canonical CI and native release packaging, but they do not block v1.0 stable on real-environment retained evidence.

After completing the documented real-TTY Terminal Drawer smoke on Linux:

```bash
python3 scripts/release/create_terminal_receipt.py \
  --platform linux \
  --terminal "<your terminal>" \
  --pass \
  --output release/evidence/linux/terminal-linux.json

python3 scripts/release/linux_qualify.py \
  --canonical-ci-run <exact-main-ci-run-id> \
  --terminal-receipt release/evidence/linux/terminal-linux.json \
  --source "<retained-linux-machine-id>"
```

The second command verifies a clean exact-main SHA, canonical CI, locked release build, READY `compat/v2`, the 200-sample 10k performance SLO, evidence assembly, and local stable verification. It does **not** publish.
