# ADR-002: Existing systems remain authoritative

Date: 2026-09-29
Status: Accepted

## Decision

Authority is split explicitly:

- Codex App Server: thread identity, history, runtime state, model/sandbox/approval
- Git/worktrees: code state and filesystem isolation
- AGENTS.md / .codex config: repository-shared Codex behavior
- GitHub/Git: review and delivery
- codex-tui: local UI projection and navigation state

## Rationale

Duplicating canonical state creates synchronization, migration and recovery problems.

codex-tui should derive views from upstream sources and persist only UI state that no existing system owns.

## Consequences

Do not persist canonical transcripts, project catalogs or turn histories in v1.

Local state may contain:

- pin
- alias
- draft
- scroll/follow state
- attention acknowledgement
- last selection

A database/index may be added later for acceleration, but it cannot become the conversation authority.
