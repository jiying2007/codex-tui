# v1.4 Workflow Completion Reconciliation

Status: development-scope complete

v1.4 reconciles the remaining workflow/UX completion behaviors from the
historical `release/v1.3-development-complete` reference line onto the current
Search & Multi-Target codebase without importing that branch's stale release
authority.

All P0/P1/P2 work packages are implemented and frozen by
`release/v1.4-completion.json`. The package line is activated at 1.4.0, while
stable publication remains separately governed by exact-SHA release evidence
and real Linux compatibility / controlling-TTY restoration.

## P0 — complete

- Board large-dataset viewport/navigation
- WorkCard Goal/Worktree/ChangeRequest relationship closure
- Codex thread create/fork + managed-worktree handoff
- unified metadata search without transcript hydration

## P1 — complete

- Saved View editor
- Review evidence + external editor/browser open
- long-thread viewport/presentation cache
- queryable fuzzy Command Palette

## P2 — complete

- bounded module decomposition
- exact-SHA 10k Board/Thread user-perceived performance evidence

## Evidence-gated

GitLab Issue Board membership projection remains deferred until the existing
native-transport/refresh-budget trigger fires. The product does not claim board
list/card authority from data it does not have.

GitHub safe writes remain frozen by the inherited v1.3 Search & Multi-Target
contract.

## Freeze

New core functionality requires a new development plan. Until then, only
defect-fix, security, compatibility, qualification-evidence, release-tooling and
documentation changes belong on the v1.4 completion line.
