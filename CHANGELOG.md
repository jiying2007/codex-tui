# Changelog

All notable codex-tui changes are recorded here.

## [1.0.0] - Unreleased

### Open source

- project licensed under the Apache License 2.0;
- Cargo package metadata declares SPDX license `Apache-2.0`;
- release archives include the project LICENSE together with generated third-party notices.

### Release candidate

- package/version line advanced from 0.7.0 to 1.0.0;
- non-publishing preview validation uses `v1.0.0-preview.N`;
- added `codex-tui release benchmark` as the canonical retained 10k performance evidence path;
- stable performance evidence requires at least 200 measured iterations, p95 <= 50 ms and p99 <= 100 ms;
- added a compatibility-capture helper that saves READY `compat/v2` JSON and its SHA-256.

### Release status

The 1.0.0 package line is stable-eligible but is not yet declared or published as a stable release.

Stable publication remains fail-closed until the exact release commit has canonical CI plus retained Linux/macOS/Windows compatibility receipts, terminal-restoration PASS receipts and retained 10k performance evidence.

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
