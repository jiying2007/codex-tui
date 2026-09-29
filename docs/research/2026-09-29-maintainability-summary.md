# codex-tui maintainability conclusions — archived summary

Date: 2026-09-29

## Main conclusion

The dominant long-term maintenance cost of codex-tui is the number of compatibility surfaces it promises, not Rust or Ratatui itself.

Every stable promise creates recurring cost:

- Codex App Server versions / experimental APIs
- daemon / remote / local runtime topologies
- terminals / tmux / Zellij / SSH / WSL / Windows
- config and keymap schemas
- SQLite schema
- Git/worktree behavior
- public control APIs
- plugin APIs
- multiple agent adapters
- release/update/install mechanisms

The project should explicitly budget compatibility and keep stable public surfaces small.

## Key policies

- Codex remains the only first-class agent backend through the initial stable release.
- App Server remains the only canonical agent boundary.
- Stable baseline must not require experimental App Server APIs.
- Capability negotiation is more important than exact Codex version matching.
- Local SQLite stores UI/control metadata only, never canonical Codex transcript history.
- Config/keymap are public interfaces and need schema + deprecation/migration policy.
- Terminal detection/probes are an isolated subsystem; every external probe is bounded by timeout/cancellation.
- Accessibility is designed in: reduced motion, semantic non-color states, no font/icon dependency for critical meaning.
- No general plugin system in v0.x.
- Public control APIs must not expose internal Rust Action/domain types.
- Releases should be automated, reproducible, multi-platform, and checksummed.
- Dependency/license/security policy should be CI-governed.
- Diagnostics/doctor is part of the product, not an afterthought.

## Compatibility tiers

Tier 0: private internals, freely refactorable.

Tier 1: persisted local user state, requires migrations.

Tier 2: user config/keymap, requires deprecation policy.

Tier 3: upstream Codex integration, requires capability/version matrix and graceful degradation.

Tier 4: public extension/control protocol, highest maintenance cost and should be delayed.

## New guiding principle

Every compatibility promise needs an explicit maintenance budget.

Any proposal for a new stable surface should answer:

- what versions/topologies are supported?
- who owns the interface?
- what is the fallback?
- how is it tested?
- how is it migrated?
- how is it deprecated/removed?
- what recurring CI/release burden does it add?

If these are unclear, keep the feature internal/experimental rather than stable.
