# v1.4 Workflow Completion Reconciliation

Status: active isolated development plan

v1.4 reconciles the still-missing workflow/UX completion behaviors from the
historical `release/v1.3-development-complete` branch onto the current
Search & Multi-Target codebase. It does **not** merge or replace the historical
branch's stale release authority.

Main remains the v1.3 stable candidate while this work is developed on
`v1.4/integration`.

## P0

- Board large-dataset viewport/navigation
- WorkCard Goal/Worktree/ChangeRequest relationship closure
- Codex thread create/fork + managed-worktree handoff
- unified metadata search without transcript hydration

## P1

- Saved View editor
- Review evidence + external editor/browser open
- long-thread viewport/presentation cache
- queryable fuzzy Command Palette

## P2

- bounded module decomposition
- exact-SHA 10k Board/Thread user-perceived performance evidence

## Evidence-gated

GitLab Issue Board membership projection remains deferred until the existing
native-transport/refresh-budget trigger fires. The product must not claim board
list/card authority from data it does not have.

GitHub safe writes are **not** deferred here: they are already implemented and
frozen by v1.3 Search & Multi-Target and must remain regression-safe.
