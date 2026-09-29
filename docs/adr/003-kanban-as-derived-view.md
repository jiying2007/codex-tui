# ADR-003: Kanban is a derived work view, not a new task authority

Date: 2026-09-29
Status: Accepted

## Decision

The mature product will include a board/Kanban view, but board cards primarily project existing Codex, Git and GitHub work objects.

codex-tui will not introduce a full independent team task-management authority.

## Allowed sources

- Codex Thread
- Codex Thread Goal
- GitHub Issue
- GitHub Pull Request
- Git branch/worktree
- minimal local Scratch item

## Consequences

- board columns may be derived from source state
- manual drag cannot silently contradict authoritative source state
- GitHub/other team systems remain team source of truth
- local Scratch stays intentionally small
- saved list/board views share one underlying WorkCard projection
- a team collaboration server is not required for Kanban
