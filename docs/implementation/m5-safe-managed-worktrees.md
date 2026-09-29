# M5 safe managed worktrees

Date: 2026-09-30
Status: Implemented

M5 adds the first codex-tui-owned Git mutation workflow. It is deliberately narrow: managed worktree lifecycle and branch cleanup with explicit plans, locks, verification and durable receipts.

Terminal Drawer / PTY execution remains deferred to M7.

## Authority and safety boundary

Git CLI remains authoritative for repository and worktree state.

codex-tui owns:

- explicit OperationPlan objects;
- user confirmation state;
- per-repository mutation serialization;
- managed/adopted worktree metadata;
- durable OperationReceipt history;
- recovery/reconciliation metadata.

M5 does not own Git history, branches, worktree truth or arbitrary shell execution.

## Operation lifecycle

Every mutation follows:

`Plan → Confirm → Execute → Verify → Receipt`

States are:

- Planned;
- Executing;
- Succeeded;
- Failed;
- OutcomeUnknown.

A visible Managed Worktrees view shows the exact operation before execution, including cwd, argv, preconditions, target worktree/branch and expected side effect.

Only explicit `y` confirmation emits the execution effect. Esc / `c` cancels before execution.

## Supported operations

### Create managed worktree

Uses:

`git worktree add -b <branch> <absolute-path> <start-point>`

Safety:

- absolute target path required;
- target path must not exist;
- target branch must not exist;
- exact argv displayed before confirmation;
- result verified against `git worktree list --porcelain`;
- verified worktree persisted as managed metadata.

### Adopt existing worktree

Adoption is metadata-only.

No Git mutation command runs.

Safety:

- absolute path required;
- path must already be a real Git worktree;
- explicit user confirmation required;
- record marked `adopted=true`.

### Remove managed/adopted worktree

Uses:

`git worktree remove <path>`

Safety:

- only codex-tui-managed/adopted records are removable;
- dirty worktrees are refused;
- active mutation-scope overlap is refused;
- no force flag;
- success verified from Git worktree inventory;
- branch is preserved.

### Delete local branch

Uses:

`git branch -d <branch>`

This is intentionally separate from worktree removal.

Safety:

- no `-D` force delete;
- branch must exist;
- branch must not be checked out in any worktree;
- success verified with `git show-ref`.

## Mutation scope and locking

Each active Codex thread derives a MutationScope.

Preferred scope:

- LocalRepoIdentity;
- exact canonical worktree root.

Fallback scope:

- conservative thread cwd when Git identity is incomplete.

Threads that are Working / WaitingHuman, or have active/blocked/limited Goal states, contribute active scopes.

Removal refuses overlap with any active scope.

The coordinator serializes mutations by canonical Git common-directory identity:

- same repository → one async mutex;
- different repositories → may proceed independently.

## Bounded execution

The mutation coordinator never invokes a shell.

It executes `git` with explicit argv and:

- null stdin;
- piped/capped stdout and stderr;
- kill-on-drop;
- 15-second mutation timeout;
- 5-second verification timeout;
- 256 KiB retained output per pipe while continuing to drain.

## Durable receipts

SQLite schema v2 stores:

- managed worktrees;
- operation receipts.

Receipts include:

- operation ID;
- exact plan;
- lifecycle state;
- start/completion times;
- result reference;
- verification;
- failure / uncertainty reason.

SQLite v1→v2 migration is transactional and preserves all M4 state.

## Crash recovery and OutcomeUnknown

On startup the mutation coordinator reads only recoverable receipts:

- Planned;
- Executing;
- OutcomeUnknown.

Recovery rules:

- Planned from a previous process is marked Failed because it was never confirmed/executed;
- Executing becomes OutcomeUnknown;
- OutcomeUnknown is reconciled against current Git state;
- no mutation command is blindly retried.

Examples:

- create: reconcile target path, Git worktree inventory and branch;
- remove: reconcile absence from worktree inventory;
- delete branch: reconcile local branch absence;
- adopt: re-verify the existing worktree and metadata.

Partial side effects remain OutcomeUnknown rather than being guessed successful/failed.

## Managed Worktrees UI

From Thread or Workspace, `m` opens the dedicated manager.

Controls:

- j/k — select managed/adopted worktree;
- n — plan create;
- a — plan adoption of current worktree;
- d — plan removal;
- x — plan branch deletion;
- y — explicitly confirm pending plan;
- c / Esc — cancel pending plan / leave view.

Create is a staged input flow:

1. new branch;
2. absolute worktree path;
3. start point (defaults to HEAD);
4. review exact OperationPlan;
5. explicit confirmation.

The manager shows the latest receipt / verification / failure and lightweight mutation notices.

## Runtime recovery and projection invalidation

Recovery starts asynchronously; it does not block the first UI frame.

After a receipt arrives:

- matching pending plan is cleared;
- same-repository Git projections are invalidated;
- Git context is re-probed;
- planning projection is reconciled again;
- managed-worktree inventory is refreshed.

## Diagnostics

`codex-tui doctor store` reports:

- SQLite integrity/schema;
- managed-worktree count;
- recent operation receipt count;
- receipt operation id / kind / state.

It intentionally does not print repository paths, argv or user content in the receipt summary.

## Verification

Cross-platform CI covers:

- fmt;
- Clippy with warnings denied;
- full tests;
- SQLite v1→v2 migration;
- recoverable receipt filtering;
- real Git create/remove with branch preservation;
- dirty removal refusal;
- active mutation-scope removal refusal;
- per-repository lock identity;
- metadata-only adoption;
- recovery reconciliation after simulated crash;
- worktree porcelain parsing on whitespace/CRLF variants;
- M0–M4 regression suite.

## Next slice

M6 adds forge projections, with GitLab Self-Managed first and GitHub behind the same normalized ForgeBackend contract:

- Work Item / Issue;
- MR / PR;
- pipeline / checks;
- comments / approvals;
- issue board / labels / milestones where supported;
- capability negotiation and graceful degradation;
- explicit mutation confirmation and forge receipts for write operations.

M6 does not introduce a shared codex-tui team database.
