# ADR-010: Collision detection uses mutation scope, not only cwd

Date: 2026-09-29
Status: Accepted

## Decision

Parallel-edit collision detection is based on a derived MutationScope rather than only comparing thread cwd values.

MutationScope uses the best available evidence:

- runtime writable workspace roots exposed by Codex;
- runtime workspace roots;
- current worktree/cwd as conservative fallback;
- additional writable roots when upstream permission state exposes them.

## Rationale

Two threads can share writable repository state even when their cwd values differ. Conversely, different Git worktrees can be safely isolated even when they belong to the same repository.

## Consequence

When precise write scope is unavailable, codex-tui prefers a conservative ConflictRisk warning over falsely claiming isolation.
