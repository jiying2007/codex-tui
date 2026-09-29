# M2 daily conversation control

Date: 2026-09-29
Status: Implemented

M2 turns the read-only Mission Control registry into a daily Codex conversation control loop while keeping Codex App Server authoritative for threads, turns, items, approvals and user-input requests.

## Delivered

### Bounded conversation reads

Opening a thread emits a reducer effect instead of doing I/O in the render path.

The App Server actor loads:

- `thread/read(includeTurns=false)` for thread metadata;
- `thread/turns/list` for the recent turn page;
- `thread/items/list` for the recent item page.

Older history is requested only when the user pages upward at the top. Repeated PageUp events are coalesced while an older-page request is already in flight.

The normalized UI projection supports:

- user messages;
- assistant messages;
- reasoning summaries;
- plans;
- command execution;
- file changes;
- tool calls;
- forward-compatible opaque items.

Raw App Server JSON does not enter Ratatui widgets.

### Legacy history compatibility

Paginated history is preferred.

When App Server explicitly reports unsupported pagination using the same compatibility classes recognized by the upstream Codex TUI—method-not-found, or legacy invalid-request/invalid-params errors mentioning history pagination fields—codex-tui falls back for the explicitly opened thread to:

`thread/read(includeTurns=true)`

Ordinary RPC, storage or transport failures do not trigger this fallback.

For an unloaded legacy thread, `thread/resume(excludeTurns=true)` similarly falls back to ordinary `thread/resume` only when the server explicitly lacks the paginated-history contract.

No fallback scans rollout files.

### Composer and turn control

The existing LocalStore draft is the only persistent composer state.

- `a` opens the composer.
- A draft survives thread switches and process restart.
- Successful submission clears and persists the draft.
- Failed submission leaves the draft intact.
- If a turn is active, submission uses `turn/steer` with `expectedTurnId`.
- Otherwise the actor ensures the thread is loaded and uses `turn/start`.
- `Ctrl+C` interrupts the exact active turn using `turn/interrupt`.
- On an idle thread, `Ctrl+C` returns to Mission Control and releases the thread watch.

Quick Prompt from Mission Control enters the exact selected thread and waits for the initial history read before allowing a start/steer decision.

### Interactive requests

The actor recognizes these App Server server requests:

- `item/commandExecution/requestApproval`;
- `item/fileChange/requestApproval`;
- `item/permissions/requestApproval`;
- `item/tool/requestUserInput`.

Requests are normalized into thread-scoped, in-memory state and become non-acknowledgeable Needs You attention.

Thread View exposes explicit controls:

- `y`: accept command/file/permission approval;
- `n`: decline;
- `c`: cancel/deny;
- `i` or Enter: answer a user-input request;
- `Ctrl+C`: interrupt the active turn.

Permission denial follows upstream Codex TUI semantics: an empty permission grant with turn scope, not a JSON-RPC protocol failure.

Free-text user-input answers preserve commas. Comma splitting is used only for questions that expose selectable options. Secret answers are masked in the UI and are never written to LocalStore.

Unknown server requests are rejected fail-closed and surfaced diagnostically.

### Live updates and watch lifecycle

The actor refreshes an opened conversation on turn/item lifecycle notifications without letting background activity alter registry selection or per-thread scroll/follow state.

Conversation watches are released when the user leaves a thread, including idle `Ctrl+C`, so opening many threads over time does not create an ever-growing background refresh set.

Pending interactive work remains globally visible through the registry attention projection even when its thread is not actively watched.

### Performance and authority

- first UI frame is not blocked on App Server startup;
- render path performs no blocking I/O;
- idle UI does not continuously redraw;
- per-request App Server RPC latency is bounded;
- recent history is paginated on capable servers;
- older loads are coalesced and deduplicated;
- exact Codex thread ID remains conversation identity;
- canonical conversation data is never persisted by codex-tui.

## Local persistence

M2 still persists only operator state:

- draft;
- scroll/follow;
- pins;
- aliases;
- unread;
- local non-blocking attention acknowledgement.

Conversation pages, pending approvals and user-input answers are memory-only projections.

## Verification

CI runs on Linux, macOS and Windows with:

- `cargo fmt --check`;
- Clippy with warnings denied;
- all-target/all-feature tests.

Tests cover:

- exact-thread navigation;
- start/steer readiness gating;
- draft persistence semantics;
- interactive request parsing and explicit resolution;
- multiple user-input questions;
- free-text comma preservation;
- secret-answer UI behavior;
- older-page merge/deduplication/coalescing;
- legacy history normalization and compatibility detection;
- watch-release lifecycle;
- responsive rendering.

## Next slice

M3 adds Git context and review without mixing Git identity with Codex thread identity:

- LocalRepoIdentity and WorktreeIdentity;
- branch/dirty state;
- changed-file summary;
- concurrent-checkout collision warning;
- Review/Diff;
- external editor integration.
