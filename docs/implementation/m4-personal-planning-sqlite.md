# M4 personal planning and SQLite

Date: 2026-09-30
Status: Implemented

M4 adds the personal planning layer and the first relational local store. It does not change the authority model: Codex App Server, Git, and future forge providers remain authoritative for their own data.

## Local authority boundary

SQLite owns only local operator/planning state:

- per-thread draft/scroll/follow and existing operator state;
- WorkCard stable local relationship identity;
- WorkCard links and presentation/planning overlays;
- ScratchWork;
- Saved Views;
- local notes;
- bookmarks;
- snooze;
- hot slots.

SQLite does **not** store:

- canonical Codex transcript/turn/item history;
- canonical Codex Goal state;
- canonical Git repository/worktree/diff state;
- future forge WorkItems/MRs/pipelines as local authority.

External observations are normalized in memory and reconciled with local overlays.

## SQLite introduction

M4 moves runtime state from atomic `state-v1.json` to bundled SQLite through `rusqlite`.

The human-edited configuration remains TOML.

### Legacy import

On first SQLite open:

1. create/configure the SQLite store;
2. create schema transactionally;
3. validate `state-v1.json` schema;
4. create `state-v1.json.pre-sqlite-backup`;
5. import operator state in one transaction;
6. mark the import in SQLite metadata;
7. archive the migrated source as `state-v1.json.migrated`.

The import marker prevents stale legacy state from being re-imported later.

### Integrity and privacy

Each operational open performs SQLite quick-check validation.

On Unix:

- state directory is tightened to mode 0700;
- SQLite and backup files are tightened to mode 0600.

The store never writes credentials or Goal/user-input secret answers.

### Degraded mode

The runtime switches to a safe local-store degraded state if SQLite loading, integrity validation, or a later planning/operator write fails.

After a write failure the session disables further local writes instead of falling back to JSON or creating two local authorities.

Codex control, Git context and Review remain usable.

`codex-tui doctor store` reports:

- backend path;
- SQLite schema version;
- integrity state;
- legacy import state;
- planning row counts.

## WorkCard identity

A WorkCard has a permanent local id and exactly one primary anchor.

Supported anchor/source kinds include:

- ScratchWork;
- CodexThread;
- ForgeWorkItem;
- Goal;
- Worktree;
- ChangeRequest.

The SQLite schema enforces uniqueness of active anchor kind + source reference.

Discovery never title-merges cards.

Thread cards can remain implicit projections until local overlays/relationships need persistence.

## Reconciliation

Planning projection is deterministic and pure where possible.

Inputs:

- live Codex Thread observation;
- live Git projection;
- optional live Goal observation;
- local WorkCard overlays;
- collision count;
- source freshness/error state.

Outputs include:

- workflow stage;
- explainable stage reason;
- attention overlay;
- snooze routing state;
- provenance/freshness;
- title/note/tags/priority overlays;
- optional Goal summary.

### Workflow stage

Canonical columns remain:

- Inbox;
- Ready;
- Working;
- Review;
- Done.

Done requires an explicit local completion acknowledgement; thread idle/unloaded does not silently mean Done.

### Attention remains orthogonal

Attention can coexist with every workflow stage.

Examples:

- Working + ApprovalRequired;
- Working + GoalBlocked;
- Working + ConflictRisk;
- Review + ReviewUnseen.

Needs You is a filter/virtual lane/order rule, not a workflow column.

Snooze suppresses routing through Needs You without deleting the source attention state.

## Board, List and Saved Views

`b` opens planning from Mission Control/Thread.

Built-in Saved Views include:

- All Work;
- Needs You;
- Needs Review.

Layouts:

- Board;
- List;
- Review Queue.

Board uses the five workflow columns. Compact terminals show one selected workflow column; wide terminals show all five.

Saved View filtering supports resident metadata including:

- `status:needs-you`;
- `stage:<stage>`;
- `workspace:<text>`;
- `tag:<tag>`;
- `source:thread|scratch|forge`;
- `goal:<objective-or-status>`.

User Saved Views are local SQLite records and can be created/deleted without changing source systems.

## ScratchWork

ScratchWork intentionally remains small:

- title;
- note;
- workspace;
- priority;
- Inbox/Ready/Done state;
- timestamps.

Creating ScratchWork never creates a Codex thread or forge issue.

Scratch state is the only directly local planning workflow state. External thread/Goal/Git stages are derived observations plus explicit local overlays.

## Local productivity overlays

M4 exposes SQLite-backed:

- note;
- bookmark;
- snooze;
- hot slots 1–9.

These operations target normalized source references.

Hot slots can jump to exact thread or ScratchWork targets.

Context actions are explicit; no title-based relationship guessing is introduced.

## Codex Goal projection

Current upstream Codex Goal methods are stable/non-experimental:

- `thread/goal/get`;
- `thread/goal/set`;
- `thread/goal/clear`;
- `thread/goal/updated`;
- `thread/goal/cleared`.

codex-tui keeps `experimentalApi=false`.

Goal fields projected in memory:

- objective;
- lifecycle status;
- token budget;
- tokens used;
- elapsed time;
- created/updated timestamps.

Goal status contributes to WorkCard derivation:

- Active -> Working;
- Paused -> Ready;
- Blocked -> Working + GoalBlocked;
- UsageLimited -> Working + UsageLimited;
- BudgetLimited -> Working + BudgetLimited;
- Complete -> Review + ReviewUnseen until explicit local completion acknowledgement.

Goal state is never stored in SQLite.

### Capability discovery

Goal capability discovery is bounded and does not block startup:

- at most one previously unseen thread Goal is probed per 250 ms tick;
- new registry threads are queued once;
- method-not-found or compatible old-protocol errors mark Goal unavailable globally;
- ordinary operational failures remain visible errors and do not masquerade as compatibility fallback.

A per-thread checked set distinguishes confirmed no-Goal from not-yet-probed, preventing accidental overwrite of an existing upstream Goal.

### Goal controls

Inside Thread View:

- `g` opens Goal actions;
- `e`/Enter creates or edits objective;
- `p` pauses an existing Goal;
- `r` resumes/activates an existing Goal;
- `c` clears an existing Goal;
- Esc closes the Goal action surface.

All mutations are explicit App Server effects.

Approval/user-input requests take precedence over Goal controls.

## Verification

Cross-platform CI covers Linux, macOS and Windows with:

- `cargo fmt --check`;
- Clippy with warnings denied;
- all-target/all-feature tests.

Coverage includes:

- transactional JSON -> SQLite migration;
- backup/archive/no-reimport behavior;
- SQLite integrity/schema health;
- WorkCard anchor uniqueness;
- Scratch/SavedView/notes/bookmarks/hot-slot round trips;
- workflow/Attention orthogonality;
- snooze routing semantics;
- Board/SavedView filtering;
- Goal parsing/status projection;
- Goal stage/attention derivation;
- Goal not leaking into LocalState/SQLite;
- Goal capability degradation classification;
- complete M0-M3 regression suite.

## Next slice

M5 adds safe managed worktrees:

- mutation scope;
- repository-scoped mutation serialization;
- explicit create/fork/remove plans;
- managed vs user-owned worktrees;
- dirty/destructive refusal by default;
- interruption reconciliation;
- operation receipts;
- branch deletion separate from worktree removal.
