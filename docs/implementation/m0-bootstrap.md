# M0 implementation bootstrap

Date: 2026-09-29
Status: Implemented on `feat/m0-bootstrap`

This implementation turns the architecture-only repository into an executable Rust baseline without violating the final authority model.

## Included

- single Rust binary, version 0.1.0;
- Ratatui 0.30 + Crossterm 0.29 + Tokio;
- Action / Reducer / Effect skeleton;
- normalized domain types for Thread, runtime and Attention;
- FakeBackend for deterministic development without a live model;
- operator-only LocalStore abstraction;
- TOML human configuration + versioned JSON machine state;
- atomic same-directory state replacement;
- responsive Mission Control layouts at compact/standard/wide widths;
- command-ID keymap layer instead of widget-local raw key handling;
- terminal raw-mode/alternate-screen restore guard;
- `doctor` command;
- cross-platform Linux/macOS/Windows CI;
- reducer/store/keymap/backend tests;
- render harness covering 40/80/120/160 columns.

## Intentionally not included

M0 does not add canonical transcript storage, SQLite, Git mutations, forge APIs, PTY drawer, or a second Codex state model.

## Next slice

M1 replaces FakeBackend registry data with a read-only Codex App Server adapter while keeping the same normalized domain boundary.
