# Changelog

All notable codex-tui changes are recorded here.

## [1.1.0] - Unreleased

### Development

- opened the 1.1 development line after the immutable v1.0.0 stable release;
- new 1.1 changes are recorded here until the next stable publication;
- generalized Linux Tier 1 qualification and the retained v1 stable support policy beyond the original v1.0.0 release;
- surfaced Mission Control backend provenance (App Server platform/Codex Home) and selected cwd terminal readiness so local/foreign/stale sessions are explainable before opening the Terminal Drawer;
- added a dedicated host-local-only Registry toggle that composes with text search instead of overwriting it;
- persisted the host-local-only Registry preference through the existing operator-state store while keeping text search ephemeral;
- restricted Git/Forge projection probes to host-local directories and made nonlocal Git probing explicitly skipped in Mission Control;
- surfaced selected-session Git repository backing in the Mission Control status line before the potentially long Codex Home path;
- added a persistent Registry repo-backed-only toggle that excludes nonlocal and confirmed non-repository sessions while keeping unresolved/probing/degraded candidates visible until Git projection resolves;
- ordered Mission Control sessions by newest activity and bounded the default Registry to the recent 100 while retaining pinned/Needs You history, full-history search and an explicit all-history toggle;
- reserved screen rows for Terminal Drawer instead of overlaying it on Mission Control/Thread/Review/Workspace/Board content, keeping scrollbars, footers and context visible.

## [1.0.0] - 2026-09-30

### Open source

- project licensed under the Apache License 2.0;
- Cargo package metadata declares SPDX license `Apache-2.0`;
- release archives include the project LICENSE together with generated third-party notices.

### Stable release

- package/version line advanced from 0.7.0 to 1.0.0;
- non-publishing preview validation uses `v1.0.0-preview.N`;
- added `codex-tui release benchmark` as the canonical retained 10k performance evidence path;
- stable performance evidence requires at least 200 measured iterations, p95 <= 50 ms and p99 <= 100 ms;
- added a compatibility-capture helper that saves READY `compat/v2` JSON and its SHA-256.

### Release status

1.0.0 was published on 2026-09-30 as the first stable codex-tui release.

Stable publication was gated on the exact release commit by canonical CI, retained Linux Tier 1 READY compatibility and real terminal-restoration evidence, and retained Linux 10k performance evidence. macOS and Windows remained required in canonical CI plus native package/archive smoke as Tier 2 automated-compatibility platforms.

## [0.7.0] - 2026-09-30

### Added

- personal-first Codex thread Registry with real App Server integration;
- conversation control, approvals, user-input handling, prompt submission and interrupt;
- Git context, Review, external-editor handoff and managed-worktree safety;
- SQLite-backed WorkCard, Board/List, Saved Views, ScratchWork, local notes and hot slots;
- GitLab Self-Managed read-only forge integration and explicit plan/verify/receipt mutations;
- GitHub.com read-only ForgeProvider;
- read-only headless CLI, compatibility Doctor and deterministic 10k scale fixtures;
- richer resident SavedView queries, transactional batch-local actions and argv-only launch presets;
- bounded cross-platform Terminal Drawer with PTY lifecycle evidence and Windows ConPTY DSR/CPR handling.

### Hardened

- grapheme-safe and terminal-column-aware CJK/emoji rendering;
- keyboard/focus priority and narrow-terminal accessibility;
- compatibility schema v2 with required/optional/degraded semantics;
- canonical Linux/macOS/Windows compatibility matrix and retained evidence contract;
- Cargo.lock-based release candidate dependency pinning;
- preview/stable release verification with fail-closed evidence gates.

### Release status

0.7.0 is the feature-complete M7 preview line. Preview artifacts may be built and retained without publication.

At the time of the 0.7.0 preview line, stable publication was still blocked pending an explicit project license and v1 release evidence.
