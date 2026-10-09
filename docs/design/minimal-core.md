# Minimal Core Design

Date: 2026-09-29
Status: historical implementation baseline

> **Archived design context.** For the active v1.4 boundary use
> [ADR-012](../adr/012-upstream-convergence.md) and the
> [first-deployment contract](../implementation/v1.4-first-deployment-baseline.md).
> Historical JSON-state and locally duplicated Codex behaviors are not active
> compatibility promises.


## 1. Scope

The first useful codex-tui is a thin local control plane over Codex App Server and Git.

It is not a universal agent manager and not a team collaboration server.

## 2. Data authorities

| Data | Authority | codex-tui role |
|---|---|---|
| Thread identity/history | Codex App Server | read/project/render |
| Thread runtime status | Codex App Server | normalize into attention UI |
| Approval/sandbox/model | Codex App Server | display and respond |
| Repository/branch/worktree | Git | inspect and warn |
| Team instructions | AGENTS.md / .codex config | display effective context only |
| Team review/delivery | GitHub/Git | link and summarize |
| Draft/pin/alias/scroll | codex-tui | persist locally |

No canonical transcript is copied into codex-tui-owned storage.

## 3. v1 domain model

### Workspace

Derived grouping identity.

Resolution order:

1. stable upstream project id when supported
2. Git repository identity
3. normalized cwd

Fields are deliberately minimal:

- id
- display_name
- roots
- repo_identity?
- recency

### Thread

Projection of one Codex thread:

- id
- workspace_id
- name/preview
- cwd
- status
- active_flags
- model/provider when known
- archived
- recency
- branch/worktree metadata derived from Git

### Attention

UI projection:

- NeedsYou
- Working
- Ready
- Inactive

A separate local seen/unseen marker can retain “completed but not yet inspected” without inventing a new runtime state.

### ViewState

Only local UI metadata:

- selected workspace/thread
- pins
- aliases
- drafts
- transcript scroll/follow state
- last-seen attention revision

## 4. Persistence

v1 should not require SQLite.

Use:

- config file for user preferences
- small versioned local state file written atomically

The state layer must be replaceable by SQLite later without changing domain APIs.

Introduce SQLite only when measured needs require it, such as full-text indexing, large cross-project queries or multiple writers.

## 5. Registry

The registry is derived, not canonical.

```text
App Server thread pages / status notifications
                 +
        Git metadata on demand
                 +
         local ViewState overlay
                 ↓
           SessionRegistry
```

Rules:

- metadata-first startup
- lazy detail hydration
- turns/items only when a thread is opened
- no full-history scan during startup
- upstream notifications update the registry incrementally
- local rollout scanning is recovery/compatibility fallback only

## 6. User interface

### Registry screen

Default screen.

Minimum information per row:

- attention/status marker
- project/workspace
- thread name/preview
- recency
- branch/worktree hint when useful

Primary actions:

- move selection
- open exact thread
- jump to next NeedsYou
- filter/search
- pin/unpin
- create thread
- resume/fork where supported

### Thread screen

- transcript
- composer
- approvals
- interrupt/steer
- compact status footer
- back to registry without losing registry selection

Transcript scrolling and composer editing are independent.

### Review screen/overlay

- file list
- diff
- review findings
- open editor/browser

Review is first-class because it is high-value for both individual and team workflows without introducing another source of truth.

## 7. Git/worktree behavior

v1 is read-mostly:

- detect repository
- detect worktree
- branch
- dirty state
- changed-file summary
- collision warning

Example warning:

```text
! 2 active editing threads share /repo/main
```

Do not make automatic worktree creation a prerequisite for v1.

Managed worktree creation/removal belongs in a later milestone after per-repository mutation serialization and recovery are implemented.

## 8. Team usage

Small teams share conventions using existing repo mechanisms:

- AGENTS.md
- .codex/config.toml
- project skills/plugins when relevant
- Git branches/worktrees
- GitHub issues/PRs/CI

codex-tui local state is not committed.

A future repo-local codex-tui preset may be introduced only for settings not already expressible through Codex/Git conventions.

## 9. Compatibility

Stable baseline cannot require experimental App Server APIs.

Each enhanced feature declares:

- preferred upstream method/capability
- fallback
- unavailable behavior

Example:

```text
Workspace grouping:
  native project id
  -> git repo identity
  -> cwd
```

Unknown upstream events fail soft and are logged.

## 10. Internal architecture

Keep one Cargo crate initially.

Suggested modules:

```text
src/
  app/
  codex/
  registry/
  git/
  state/
  tui/
  ui/
  doctor/
```

Use Action / Reducer / Effect.

Reducers are pure. Effects own async I/O. Rendering performs no blocking work.

## 11. Complexity gates

Before adding a feature:

1. Is Codex already the authority?
2. Is Git/GitHub already the authority?
3. Can the value be derived instead of persisted?
4. Does this require a new service/database?
5. Does it create a public compatibility promise?
6. Can it be optional/later without breaking the core workflow?

If a feature creates another source of truth or permanent service, default to deferring it.
