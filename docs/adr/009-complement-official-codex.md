# ADR-009: Complement official Codex instead of duplicating it

Date: 2026-09-29
Status: Accepted

## Decision

codex-tui reuses official Codex Thread, Project, Goal, Queue, Section and status semantics whenever they are available and suitable.

The project does not define its long-term differentiation as a second implementation of the official Agent Center/Agents Overview.

## Differentiation

- personal cross-repository planning
- Attention routing
- WorkCard relationships
- Git/worktree safety
- GitLab-first Forge integration
- review/evidence workflow
- personal Saved Views and operator metadata
- cross-system diagnostics

## Consequence

When an upstream Codex capability makes a local compatibility layer redundant, prefer deleting/reducing the local layer rather than maintaining a competing source of truth.