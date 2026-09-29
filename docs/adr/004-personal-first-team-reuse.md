# ADR-004: Product is personal-first; team value is reuse

Date: 2026-09-29
Status: Accepted

## Decision

codex-tui optimizes first for one developer operating many Codex threads and repositories.

Team value comes primarily from reuse of repository-owned assets and projection of shared forge state.

A shared codex-tui session service is not part of the normal team model.

## Shared team assets

Prefer AGENTS.md, .codex/config.toml, skills/plugins, repository commands/scripts, CI and code-forge issues/change requests.

Personal UI state remains local.

## Consequence

Features requiring identity, RBAC, presence or central shared state must justify themselves as an optional collaboration layer rather than enter the core.