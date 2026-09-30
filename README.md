# codex-tui

A local-first terminal workbench for managing multiple Codex projects and conversations.

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
- optional `glab` authenticated to the repository's GitLab host for M6 GitLab integration

Commands:

```bash
cargo run
cargo run -- doctor codex
cargo run -- doctor git
cargo run -- doctor forge
cargo run -- doctor store
cargo run -- --fake
```

Normal startup launches a local `codex app-server --listen stdio://` connection after the first UI frame. Mission Control stays metadata-first; opening a thread loads only the recent conversation page. Thread View supports paginated history, persistent local drafts, turn start/steer/interrupt, approvals, user-input requests, and stable Codex Goal projection when the connected App Server supports it.

Personal planning state is stored locally in SQLite: WorkCard relationships/overlays, ScratchWork, Saved Views, notes, bookmarks, snooze and hot slots. Canonical Codex conversations/Goals and Git state are never copied into SQLite.

`--fake` is a deterministic development/fixture mode; it is never an automatic fallback for a failed real backend.

M6a adds an asynchronous read-only GitLab projection. A normal forge refresh stays within four `glab api` calls (project, recent Issues, open MRs, recent Pipelines); approval/discussion details are loaded only when Review is opened. Forge observations remain derived and carry freshness/provenance. GitLab Issues appear as deduplicated WorkCards, while matching MRs/Pipelines enrich the corresponding Codex thread card. Forge failure never blocks Codex/Git operation.

M6b adds explicit GitLab merge-request mutations from Review / Workspace context actions (`.`): create MR, comment, approve, and merge. Every write is plan-first and requires explicit confirmation. Approve/merge revalidate the exact MR HEAD SHA immediately before execution; merge never requests force/policy bypass. Comment bodies remain memory-only and are not stored in SQLite. Uncertain external outcomes remain `OutcomeUnknown` and are never blindly retried.

## Status

- M0 local control-plane skeleton: implemented and merged.
- M1 read-only Codex thread registry: implemented in the v0.1.x line.
- M2 daily conversation control: implemented in the v0.2.x line.
- M3 Git context and review: implemented in the v0.3.x line.
- M4 personal planning + SQLite: implemented in the v0.4.x line.
- M5 safe managed worktrees: implemented in the v0.5.x line.
- M6a GitLab Self-Managed read-only forge projection: implemented in the v0.6.x line.
- M6b safe explicit GitLab MR mutations: implemented in the v0.6.x line.
- M6c GitHub provider: remaining M6 work.

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
