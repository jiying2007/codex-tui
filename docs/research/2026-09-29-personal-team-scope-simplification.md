# Personal / small-team scope simplification

Date: 2026-09-29

## Conclusion

The long-term architecture is directionally sound, but it is too complex if implemented as v1. For individual developers and small engineering teams, codex-tui should be a local thin control plane, not a new collaboration platform.

Authority model:

- Codex App Server owns conversation/runtime state.
- Git/worktrees own code state and isolation.
- AGENTS.md and .codex/config.toml own repo-shared conventions.
- GitHub issues/PRs own team review and delivery.
- codex-tui owns local projection, navigation, attention, drafts and review UX.

## Reduce the v1 domain

Use only four first-class concepts:

1. Workspace: derived from upstream project id, git repo identity, or cwd.
2. Thread: Codex thread summary plus runtime status.
3. Attention: NeedsYou / Working / Ready / Inactive plus seen/unseen locally.
4. ViewState: selected thread, draft, scroll state, pin/alias, last-seen state.

Worktree is derived Git metadata first; full lifecycle management comes later.

## Storage simplification

Do not require SQLite in v1. Use user config plus a small atomically written local state file for pins, aliases, drafts, attention acknowledgements and last selection. Do not mirror transcripts, project catalogs or turn indexes.

Introduce SQLite only when real requirements appear: full-text indexing, multiple writers, large cross-project queries, or durable event history.

## Personal mode

Default should be one local binary with no codex-tui-owned daemon, web server, account system, cloud sync, RBAC, Postgres, browser UI or mobile companion.

## Team mode

Small-team use does not require a shared codex-tui server. Share behavior through repository-owned AGENTS.md, .codex/config.toml, skills/plugins where appropriate, Git branches/worktrees and GitHub PR/issues. Managed Codex environments can already enforce requirements.toml and managed policy; codex-tui should surface effective policy rather than reimplement it.

Pins, drafts, scroll positions and attention state remain personal.

A team-wide live dashboard would introduce identity, auth, shared state, presence, conflict resolution, retention and service operations. Treat that as a separate future product layer.

## v1 product surface

Mission Control:
- workspace/thread registry
- NeedsYou / Working / Ready / Inactive
- fuzzy filter/search
- pin/alias
- exact thread navigation
- attention jump

Thread view:
- paginated transcript
- composer
- steer/interrupt
- approvals
- model/cwd/sandbox/approval visibility
- draft/scroll persistence

Git context:
- repo/worktree
- branch/dirty state
- changed-file summary
- shared-working-tree collision warning

Review:
- diff/review
- editor/browser integration
- optional PR/CI read-only summary

Diagnostics:
- doctor
- backend/version/capability
- terminal/config validation

## Explicitly not v1

- shared team DB
- team presence/RBAC
- cloud service
- web/mobile UI
- custom Kanban/task DB
- general plugin system
- universal agent adapters
- containers/sandbox platform
- remote orchestration service
- agent-to-agent messaging
- workflow/job engine
- cost/accounting platform
- organization analytics
- custom policy engine

## Search lessons

Fleet validates a compact operator loop: see who needs attention, one-key jump, worktree awareness and deep agent integration rather than broad shallow compatibility.

Chat Room provides the strongest simplification principle: coordination is advisory; Git and the provider stay authoritative. Its transcript index is optional and the core still works without it. It also derives board state instead of storing another independent Kanban status.

Agent Workspace shows that a useful manager can remain conceptually small: list + preview + status + groups + notes + worktrees + attach.

Artemis shows a good boundary if collaboration is ever needed: solo mode requires no application server, while collaboration is an optional separate service.

## Recommended sequence

P0: personal control plane — single binary, App Server, derived workspaces, thread registry, attention, thread interaction, tiny local state, diagnostics.

P1: developer workflow — Git context, existing worktree awareness, review/diff, collision warnings, search, command palette/keymap.

P2: safe worktree operations — create/fork/cleanup and serialized per-repo mutations.

P3: small-team convention — optional tiny repo-local presets only if needed, GitHub issue/PR integration.

Separate future layer: shared live dashboard, collaboration service, web/mobile, multi-agent orchestration and job engine.

## Final recommendation

Yes: the previous design was over-complex as an implementation plan. Keep the architectural boundaries but drastically reduce the initial product. codex-tui v1 should be a single-user, local-first Codex control plane that derives projects and sessions from Codex + Git, stores only tiny UI metadata, highlights what needs attention, switches threads instantly, and makes review/worktree context obvious. For small teams, share conventions through the repository and collaborate through Git/GitHub rather than building a new shared platform.
