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
- GitHub issues/PRs/CI remain team collaboration and delivery authorities.
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
- canonical custom Kanban/task database
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
- GitHub Issue/PR/CI projection
- notes/bookmarks, snooze/unread and lightweight notifications
- optional thread queue UI as upstream support stabilizes

Board cards should project existing Codex/Git/GitHub work whenever possible.

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

## Status

Architecture and product research are being archived under `docs/research/`.
Implementation design lives under `docs/design/`.

See:

- `docs/design/minimal-core.md`
- `docs/design/core-user-flows.md`
- `docs/design/kanban-and-feature-evaluation.md`
- `docs/design/target-state.md`
- `docs/design/final-plan.md`
- `docs/roadmap-v1.md`
