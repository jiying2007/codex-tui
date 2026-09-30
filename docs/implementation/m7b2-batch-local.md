# M7b2: transactional safe batch-local planning actions

Status: implementation slice under #24 / #26

M7b2 adds explicit batch productivity for the Board without introducing a batch executor for Codex, Git, worktrees, Forge or terminals.

## Safety model

The Board follows a strict three-step flow:

1. derive the currently visible planning cards;
2. freeze their exact local IDs, anchors and display titles into a `LocalBatchPlan`;
3. show a confirmation preview and execute only that frozen plan after `y`.

The target set is never recomputed at confirmation time. Changing SavedView, Board stage or selection after a plan was created cannot widen or replace its targets.

Cancelling the confirmation emits no write effect.

## Supported actions

Batch actions affect only codex-tui local planning authority:

- add a local tag;
- remove a local tag;
- set or clear local priority;
- mark ready or clear ready;
- acknowledge done or reopen;
- snooze or clear snooze.

For Codex-thread and forge-linked cards these values remain local WorkCard overlays.

For ScratchWork, fields already owned by the ScratchWork row continue to use that authority:

- priority updates `scratch_work.priority`;
- ready/clear-ready updates ScratchWork state Ready/Inbox;
- done/reopen updates ScratchWork state Done/Ready.

Snooze and tags remain WorkCard overlay fields.

## Transactionality

`SqliteStore::apply_local_batch`:

- validates the full frozen plan first;
- opens exactly one SQLite transaction;
- applies every target inside that transaction;
- commits once;
- rolls the entire batch back on any target failure.

A test intentionally places a valid ScratchWork first and a nonexistent frozen ScratchWork second and verifies the first target remains unchanged after failure.

After a successful commit the runtime reloads the planning snapshot, reconciles projections and emits one result notice.

## Bounds

- empty target sets are rejected;
- duplicate frozen targets are rejected;
- a plan is capped at 10,000 targets;
- tags must be nonempty and at most 64 characters;
- snooze deadlines must be in the future at planning time.

These limits are local safety bounds, not workflow-engine semantics.

## Explicitly not supported

M7b2 does not batch:

- Codex prompt submission;
- approvals or interactive responses;
- Git branches or worktrees;
- GitLab/GitHub mutations;
- merge/review actions;
- terminal commands;
- launch presets.

Launch presets remain M7b3. Embedded PTY/Terminal Drawer remains M7c.

## Authority

M7b2 creates no daemon, queue, scheduler or second task authority. It reuses the existing reducer/effect loop and the existing SQLite local planning store.
