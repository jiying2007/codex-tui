# v1.2 Maintainability & Terminal-State Completion

v1.2 changes the main engineering objective from feature accumulation to long-term maintainability.

The product authority model stays unchanged: Codex owns conversation/runtime, Git owns code/worktrees, the configured forge owns shared delivery state, repository files own reusable engineering convention, and codex-tui owns only local operator projection/state.

## Why this phase exists

The mature personal product is largely present, while several implementation modules have grown beyond the architecture ratchet in `docs/design/final-plan.md`. The next feature should not be added to a 7k-line reducer or a 3k-line renderer.

v1.2 therefore begins with behavior-preserving extraction, deterministic protocol replay and dependency/security governance. Product additions resume only after those boundaries exist.

## P0 order

1. **Architecture decomposition**
   - split runtime store/services from `main.rs`;
   - split AppState/action/reducer/projection responsibilities from `app.rs`;
   - split view renderers from `ui.rs`;
   - split RPC, registry/hydration and conversation from `app_server.rs`;
   - split SQLite connection/schema/recovery/operator/planning repositories;
   - split Forge provider/routing/observation and worktree coordinator/recovery.
2. **Replay compatibility**
   - introduce deterministic App Server protocol replay;
   - retain supported/previous/malformed/unknown-event fixtures;
   - make compatibility qualification runnable without model inference.
3. **Dependency/security governance**
   - cargo-deny;
   - RustSec/cargo-audit;
   - automated dependency updates with review, never blind merge.

## Module-size ratchet

The initial ratchet is deliberately a **no-growth ceiling**, not a claim that the existing large modules are acceptable long term. CI rejects growth above retained ceilings while extraction PRs reduce them.

New implementation modules should normally stay below 800 LOC and trend toward the final-plan target of roughly 500 LOC.

## P1 after the P0 boundaries

- expand the stable read-only headless surface;
- add lightweight notifications without inventing a second workflow state machine; default off, bounded terminal/native-OS delivery, edge-triggered from existing projections;
- implement the already-locked two-face + syntect highlighting choice behind a bounded/cached interface; use the pure-Rust fancy-regex backend, Review Diff syntax first, a 128 KiB synchronous cutoff and plain-text fallback.

## P2 mature accessibility mode

P2 stays inside the existing Ratatui renderer and event loop. The local `[ui]` config gains a presentation mode:

- `normal` preserves the existing immediate redraw behavior;
- `quiet` coalesces background-only redraws to a 100 ms minimum interval;
- `screen-reader` coalesces background-only redraws to a 500 ms minimum interval.

Keyboard, paste and resize interactions always bypass the background throttle. The mode does not add a second renderer, screen-reader protocol, new persisted workflow state or new product authority. Existing textual focus/attention labels, keyboard reachability and grapheme-safe rendering remain the semantic accessibility base.

## Completion and freeze

P0, P1 and P2 are closed by the machine-readable `release/v1.2-completion.json` contract. `scripts/release/validate_v1_2_completion.py` cross-checks every planned priority against retained source evidence, verifies the render-path/highlighting and accessibility constraints directly, and requires the deferred/frozen sets to remain aligned with stable criteria.

After this point, v1.2 does not accept new core functionality without a new development plan. Allowed changes are defect fixes, security, compatibility, qualification evidence, release tooling and documentation. Hosted development qualification binds the completion manifest SHA-256 but remains non-stable authority: `stableReady=false` and `publicationAllowed=false` are invariant.

## Evidence-driven only

Thread Queue, transcript FTS, native GitLab REST/GraphQL, extra package architectures and remote targets stay deferred until measured need or upstream capability justifies them.

## v1.1 relationship

v1.1 remains a parked candidate on `release/v1.1-parked` with its historical RC authority intact. v1.2 main uses a separate hosted development qualification and does not reinterpret missing v1.1 real evidence as PASS. If v1.1 is published later, it must still use exact-SHA real evidence under its existing stable criteria.
