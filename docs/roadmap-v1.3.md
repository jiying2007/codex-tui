# v1.3 Search & Multi-Target

Status: active development plan

v1.3 opens a new core-development phase after the immutable
`release/v1.2-development-complete` checkpoint. It does not reinterpret the
remaining v1.2 real-environment release gates.

## Scope

1. Transcript search: App Server full-history search first, SQLite FTS5 as a derived local fallback/index.
2. Thread Queue: project the upstream experimental queue as Codex-owned state.
3. Remote App Server targets: named local/WS/WSS/Unix targets, with local stdio remaining the default.
4. GitHub safe mutations: create/comment/approve/merge under the retained plan/confirm/revalidate/receipt model.

## Authority boundaries

- Codex App Server owns transcript and Thread Queue state.
- SQLite FTS never becomes a canonical conversation database.
- Git owns branches/worktrees/diffs.
- GitHub owns PR/review/merge state.
- codex-tui owns local target selection, local search acceleration and UI state.

## Version activation

During implementation, Cargo may remain on 1.2.0 so incomplete v1.3 work is not
presented as a releasable 1.3 candidate. After all four work packages close,
the completion change advances Cargo/criteria/CHANGELOG to 1.3.0 and makes
v1.3 the active qualification authority.

## Explicitly not reopened

Native GitLab transport still requires the measured trigger documented in
`final-implementation-choices.md`. Team server/RBAC, web/mobile,
collaboration service, generic plugins, multi-agent providers and a workflow
engine remain optional/frozen.
