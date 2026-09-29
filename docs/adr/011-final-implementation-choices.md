# ADR-011: Final implementation technology and threshold choices

Date: 2026-09-29
Status: Accepted

## Decisions

- Human-edited configuration uses TOML; machine-written initial LocalStore state uses versioned JSON.
- Default keymap uses terminal-safe Vim-style navigation, Space for next attention, a for Quick Prompt, Ctrl+K for command palette, and context-aware commands through stable Command IDs.
- Syntax highlighting uses two-face + syntect with the pure-Rust fancy-regex backend by default.
- Review intra-line diff uses similar; repository diff semantics remain provided by Git.
- SQLite is introduced in v0.4.0/M4 when WorkCard/SavedView/Scratch relationships justify relational storage.
- glab remains the GitLab transport until explicit measured performance/capability triggers justify native REST/GraphQL.
- GitLab compatibility is capability-probed at runtime; server version/tier are diagnostics, not scattered feature gates.
- Terminal Drawer is deferred until after M6, in v0.7.x/M7.
- Primary interactive SLO is p95 <= 50 ms and p99 <= 100 ms, with separate frame/startup/search budgets.

Full rationale and thresholds: docs/design/final-implementation-choices.md.
