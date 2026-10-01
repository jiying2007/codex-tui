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
- reserved screen rows for Terminal Drawer instead of overlaying it on Mission Control/Thread/Review/Workspace/Board content, keeping scrollbars, footers and context visible;
- reduced Mission Control Selected-detail noise, hid unavailable Forge internals from daily UI, and made custom Forge hosts require explicit gh/glab authentication instead of defaulting every non-GitHub host to GitLab.
- reduced Mission Control render work by computing Registry visibility and total match count in one projection pass and folding locality counts once.
- hardened reducer routing so Context/Forge/Scratch/batch state drift fails closed with an operator notice instead of panicking the TUI.
- added local UI language selection (`auto`, `en`, `zh-CN`) with locale auto-detection and Simplified Chinese coverage across daily TUI surfaces while preserving technical identifiers and upstream diagnostic text.
- deduplicated Git projection probes by exact host-local cwd, fan out completed checkout state across historical threads sharing the same working directory, and refresh selected/active checkouts every 10 seconds so dirty/branch state does not freeze after startup.
- removed Registry/planning O(N²) hot paths by indexing thread WorkCards, precomputing active-worktree collision counts, and added 10k recent/search Registry projection benchmarks.
- changed App Server Registry synchronization to event-driven thread lifecycle/status updates, suppressing full snapshot work for unrelated streaming notifications, reducing complete registry reconciliation from 2s to a 30s fallback, using canonical `recency_at` plus the state-DB fast path with legacy fallback for recurring reconciliation, and bounding eager Goal probing to the most recent 100 threads.
- completed the second i18n hardening pass for Terminal Drawer target/status errors, Forge freshness and planning errors, Selected attention labels and operation receipt states; locale auto-detection no longer misclassifies known Traditional Chinese locales as Simplified Chinese.
- cached cwd locality for Registry rendering and scope filters, removed repeated/all-history filesystem probing from ordinary Registry search and Git projection cleanup, and added a 10k host-local Registry benchmark so LOCAL ONLY / REPO ONLY scale paths are covered explicitly.
- indexed prior thread-local overlays during Registry snapshot replacement, removing the remaining O(N²) fresh-thread × existing-thread lookup while preserving pins, aliases, unread markers, attention acknowledgment invalidation and selection.
- completed a third bilingual UI cleanup for product-owned placeholders and labels, including Goal/model/branch fallbacks, Git/conversation fallback errors, ScratchWork title context and detached/empty branch placeholders while preserving raw upstream diagnostics and technical identifiers.
- added lightweight thread-id and cwd indexes for projection fan-out, removing repeated full-history scans from Git result propagation, Forge periodic refresh/propagation and active-worktree counting while keeping Registry snapshots as the source of index truth.
- bounded Git/Forge actor command and event queues, added finite projection concurrency, and suppressed duplicate pending active-Git refreshes so large multi-project refreshes cannot grow unbounded queues or serialize all probes behind one worker.
- added a canonical Linux Rust 1.88 MSRV compile gate so the `rust-version = "1.88"` package contract is continuously verified against the locked dependency graph.
- removed filesystem probing from Mission Control rendering itself: uncached native absolute cwd values render as unprobed until controlled locality/Git reconciliation checks them, while Terminal Drawer open keeps its fresh fail-closed cwd validation.
- bounded App Server command and conversation-event channels; user commands now fail explicitly under backpressure while conversation/interactive/Goal events wait for capacity instead of accumulating in unbounded memory or being dropped.
- bounded Forge/worktree mutation command and event queues and moved in-flight mutation tasks under coordinator-owned JoinSets with a total concurrency cap of 4, preserving repo/project locks while preventing unbounded queued or detached mutation work.
- bounded resident conversation history to a 16-thread LRU cache; evicted conversations reload from App Server on demand while per-thread drafts, aliases, pins and other local UI state remain resident/persisted independently.
- bounded resident Git review data to a 4-thread LRU cache, keeping the actively viewed review protected while allowing staged/unstaged diff payloads from older reviews to be reloaded on demand instead of accumulating indefinitely.
- indexed threads with pending interactive requests so 10k Registry attention projection no longer scans the full pending-request vector per thread, while preserving multiple simultaneous requests per thread and request-id retargeting semantics.
- hardened PTY teardown against bounded event-queue backpressure by releasing the receiver before joining the actor, and now emits `Ready` before reader/waiter producers can flood the queue.
- made PTY handle teardown non-blocking even when the actor is stuck in synchronous terminal I/O: the handle now owns an out-of-band child killer, drops event backpressure first, signals termination independently of the bounded command queue, and detaches the actor instead of joining it on the UI thread.
- made terminal startup transactional: once raw mode is enabled, an armed rollback guard restores raw mode, mouse capture, alternate screen and cursor state on every subsequent setup error before a `TerminalSession` can exist.
- coalesced high-frequency thread draft and scroll persistence behind a 250ms bounded write-behind window, while keeping pin/alias/attention/filter changes and submitted-draft clearing immediately durable and preserving the final shutdown flush.
- added bracketed paste support across the host terminal, application editors and Terminal Drawer: bulk paste is routed as one input action, single-line editors sanitize layout controls, multiline editors preserve normalized line breaks, and child PTY paste markers are emitted only when the child terminal mode explicitly enables them.
- bounded App Server messages accumulated while synchronous RPC requests wait for responses: notifications the runtime already ignores (for example streaming agent-message deltas) are discarded before queuing, semantic approval/Goal/Registry/boundary events are retained up to a hard 1024-message cap with explicit overflow failure, and JSON decode errors report payload size instead of duplicating the full wire line in error strings.
- surfaced staged Registry hydration as explicit backend state: an initial one-page snapshot is marked incomplete only when App Server reports another page, Mission Control labels recent/all-history/search results as hydrating/partial until full history arrives, and successful full hydration or full diagnostic probing marks the Registry complete.
- staged App Server Registry hydration for large histories: interactive startup now loads only the newest 200 threads for first paint, the Registry actor immediately hydrates the complete history afterward, `doctor codex` still performs a full probe, and full reconciliation fallback is reduced from every 30 seconds to every 5 minutes because lifecycle/status notifications remain the real-time authority.
- decoupled durable per-thread pins, aliases and local unread markers from the currently hydrated Registry slice, so staged startup, temporary thread omission and later full-history hydration cannot drop operator overlays or persist a truncated local-state snapshot.
- made staged Registry hydration cooperative instead of actor-blocking: startup retains the first-page cursor and query compatibility state, then fetches at most one additional page per actor turn with a 10ms yield; user commands are selected before the next hydration page, semantic notifications are drained before publishing pending page growth, newer live thread state wins over stale page data, and archive/delete tombstones prevent reintroduction during hydration.
- made the 5-minute authoritative Registry reconciliation cooperative as well: it builds a candidate full snapshot one page per actor turn, overlays live thread changes observed during reconciliation, applies archive/delete tombstones, and atomically replaces the live Registry only after the candidate is complete, eliminating periodic whole-history actor stalls.
- removed `thread/loaded/list` from the interactive Registry critical path: startup and periodic reconciliation now hydrate loaded-session metadata cooperatively one page per actor turn after Registry pages, while user commands and semantic notifications retain priority; `doctor compat` keeps the authoritative full capability probe.
- added an advisory Linux Registry scale-evidence harness for 10k/50k histories, capturing construction/reconciliation and recent/all-history/search/host-local projection latency plus process peak RSS as retained artifacts before any paging or SQLite-index redesign.
- removed the duplicate per-thread active-worktree collision pass from planning reconciliation: collision counts are now computed once while thread WorkCards are projected and reused for the collision index; scale-evidence now runs automatically when `src/app.rs` changes.
- upgraded retained scale evidence to schema v2: planning reconciliation now uses the same warmup + repeated p50/p95/p99/max sampling as Registry projections instead of a single noisy wall-clock measurement.
- upgraded scale evidence to schema v3 with a shared-code planning phase profiler: setup, thread projection, supplemental projection, sort, index commit, selection refresh and total distributions are measured without duplicating planning semantics or adding production-path timing.
- upgraded scale evidence to schema v4 by splitting the planning index-commit phase into collision-index build, WorkCard thread-index build, and final WorkCard vector commit so the remaining 50k rebuild cost can be attributed before changing data structures.
- stopped the 15-second Forge refresh timer from rebuilding the entire planning projection when no Forge probe was scheduled; pending-probe reconciliation remains unchanged, while fresh/pending Forge state no longer causes a periodic no-op WorkCard rebuild/render.
- classified Registry drain changes by responsibility: conversation/history/interactive UI events no longer trigger Git projection refresh plus full planning reconciliation, while Registry snapshots still refresh Git + planning and Goal observations still reconcile planning.
- extended drain classification to Git and mutation actors: Git review payloads, managed-worktree inventory updates and mutation notices render normally without triggering unrelated Forge/Git refresh plus full planning reconciliation; Git contexts and mutation receipts retain existing projection refresh semantics.
- coalesced automatic background planning invalidations across Registry, Git, Forge, mutation drains, periodic Forge refresh and fake ticks so one UI loop performs at most one full planning reconciliation before rendering, while explicit user-driven planning writes remain immediate.

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
