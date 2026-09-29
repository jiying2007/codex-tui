# ADR-001: codex-tui is a thin local Codex control plane

Date: 2026-09-29
Status: Accepted

## Decision

The initial stable product is a single-user, local-first Codex workbench.

It does not own a collaboration server, shared team database, cloud sync, generic plugin runtime, or universal agent abstraction.

## Rationale

The primary workflow is local:

- discover projects and Codex threads
- see which thread needs attention
- switch into the exact thread
- approve/steer/interrupt
- understand Git/worktree context
- review changes

Adding shared services or universal adapters before this workflow is mature increases maintenance surface without improving the core loop.

## Consequences

- one binary is sufficient for v1
- Codex is the only first-class agent backend
- team conventions reuse repository/Git/GitHub mechanisms
- future collaboration remains an optional separate layer
- new features that create a new source of truth need a separate ADR
