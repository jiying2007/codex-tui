# M7d1: accessibility, CJK/grapheme and keyboard/focus hardening

Status: implementation slice under #38 / #39

M7d1 hardens the existing v0.7.x interface without adding a new product surface or authority.

## Terminal text model

Non-terminal UI text now uses a shared terminal-text layer instead of Rust scalar-count truncation.

The helpers in `src/text.rs` provide:

- inline control-character sanitization;
- Unicode terminal display-width measurement;
- extended-grapheme-cluster-safe truncation;
- fixed-cell padding by terminal columns rather than Unicode scalar count.

Dependencies are explicit:

- `unicode-segmentation`
- `unicode-width`

This addresses CJK wide glyphs, combining marks and emoji/ZWJ clusters.

### Truncation rule

`truncate_display(value, columns)`:

1. sanitizes non-terminal inline controls;
2. returns the whole string when its display width fits;
3. reserves one terminal column for `…`;
4. iterates complete grapheme clusters only;
5. never returns a string wider than the requested terminal column budget.

`fit_display` applies the same truncation and pads by visible width.

## Control-character boundary

The terminal Drawer is intentionally excluded from inline sanitization because its content has already passed through the VT parser and represents terminal semantics.

Other inline metadata—thread workspace/title, planning titles, ScratchWork fields, worktree paths, changed paths and launch names—must not inject raw newline/control bytes into surrounding application layout.

Whitespace controls become spaces. Other control characters become U+FFFD.

## CJK and grapheme coverage

Tests cover:

- Chinese/CJK display widths;
- combining-character grapheme clusters;
- emoji/ZWJ clusters;
- fixed-width CJK padding;
- 40-column compact Registry with CJK/emoji/control-bearing metadata;
- Terminal Drawer semantic CJK rows.

The width helpers are deterministic and do not depend on a particular terminal font.

## Keyboard-only reachability

The keymap test locks keyboard reachability for every ViewKind:

- Help
- Search
- Context Actions
- Back

Existing surface-specific bindings remain covered separately for Registry, Board, Worktrees and Terminal Drawer.

## Focus priority

Binary-level input tests lock the actual `handle_key` priority rather than only reducer state:

1. focused Terminal Drawer consumes printable keys before app commands;
2. `Ctrl+]` releases Drawer focus without closing the session;
3. Terminal focus wins over launch/context overlays;
4. context overlay traps unrelated global navigation;
5. active text/search input consumes printable keys before global commands.

This prevents shortcut leakage between overlapping surfaces.

## Help synchronization

Help text is represented by one `HELP_LINES` source and tested against the locked keymap for the highest-value global bindings.

This is intentionally a small ratchet, not a second keybinding registry.

## Font and color assumptions

The UI does not require Nerd Font/private-use glyphs. A source-level test rejects Unicode private-use codepoints in `ui.rs`.

Selection/focus/attention state remains represented textually (prefixes, labels and explicit wording) rather than relying on color alone. Existing reverse-video selection is additive styling, not the only semantic carrier.

## Narrow terminal behavior

Registry rendering is exercised at 20, 30 and 40 columns, in addition to the existing 40/80/120/160 responsive snapshots.

M7d1 does not promise that every hint remains visible at extremely small widths; it guarantees bounded rendering without panic or malformed grapheme truncation.

## Non-goals

M7d1 does not add:

- a screen-reader protocol layer;
- mouse-only interaction;
- custom icon fonts;
- a theme system;
- a second renderer;
- FTS;
- new Codex/Git/Forge/terminal authority.

M7d2 owns compatibility evidence. M7d3 owns release/distribution hardening.
