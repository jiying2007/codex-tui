# codex-tui 最终完整方案 v2

Date: 2026-09-29
Status: Final architecture baseline
Product priority: personal-first, local-first, Codex-native
Team model: repository reuse + code-forge projection
Forge priority: GitLab Self-Managed first, GitHub second

---

## 0. Executive summary

codex-tui is a personal-first, local-first Codex engineering workbench.

It is designed first for one developer who is simultaneously operating many repositories, many Codex threads, several long-running Goals, parallel Git worktrees, approvals, reviews and code-forge items.

Its mature workflow is:

~~~text
Plan
  -> Start / Resume Codex work
  -> Work in repository/worktree
  -> Surface human attention
  -> Review evidence and code
  -> Change Request / Pipeline
  -> Done
~~~

The product must not become a second source of truth for Codex, Git or the team's code forge.

Authority remains:

~~~text
Codex App Server  -> thread / turn / item / goal / approval / runtime
Git               -> repository / branch / worktree / diff / commit
Code Forge        -> team work item / board / MR-or-PR / pipeline / review
Repository files  -> AGENTS.md / .codex config / skills / scripts
codex-tui         -> local operator projection and local UI metadata
~~~

The distinguishing value is lower operator overhead across many concurrent pieces of Codex engineering work.

The mature product is not defined by "more AI" or by replacing Jira/GitLab/GitHub. It is defined by five outcomes:

1. lower context-switching cost;
2. faster human-attention routing;
3. safer parallel development;
4. higher review throughput;
5. clearer operational state.

---

# 1. Product priority

## 1.1 Primary persona: individual developer

The primary optimization target is one developer with:

- multiple active repositories;
- multiple Codex threads per repository;
- research/debug/feature/cleanup work running in parallel;
- Git branches and worktrees;
- approval and user-input requests;
- completed work waiting for review;
- many existing terminal windows and remembered thread IDs.

The application must make this workflow substantially easier before any team feature is considered successful.

## 1.2 Team value: reuse, not shared-session infrastructure

For a small engineering team, the main value is reuse of shared engineering assets:

- Git repository;
- AGENTS.md;
- .codex/config.toml;
- project skills/plugins;
- Makefile / justfile / package scripts;
- test/build/review commands;
- CI definitions;
- GitLab/GitHub work-item and change-request conventions.

Each developer still owns personal:

- drafts;
- scroll position;
- pins;
- aliases;
- snooze state;
- attention acknowledgements;
- saved personal views;
- hot slots;
- local Scratch work.

Normal team adoption must not require:

- codex-tui account system;
- shared session database;
- RBAC;
- presence;
- team chat;
- cloud synchronization;
- central service.

Optional live collaboration can exist later as a separate layer, never as a prerequisite.

---

# 2. Product boundary against official Codex

Official Codex is itself evolving multi-thread and Agent Center / Agents Overview functionality, including thread status groups, search, grouping and thread lifecycle actions.

Therefore codex-tui must not define its long-term differentiation as merely "a multi-session list".

Rule:

> Reuse official Codex thread/project/status semantics and concentrate codex-tui differentiation above that layer.

Long-term differentiation:

- cross-repository personal planning;
- Attention Inbox across all work;
- WorkCard relationship projection;
- Git/worktree safety;
- code-forge integration, especially GitLab Self-Managed;
- review/evidence workflow;
- saved operator views;
- local personal workflow state;
- diagnostics across Codex + Git + forge + terminal.

If official Codex gains a capability that makes a codex-tui compatibility layer redundant, prefer deleting the duplicate layer rather than competing with upstream.

---

# 3. Non-negotiable authority model

## 3.1 Codex owns execution

Codex App Server is authoritative for:

- Thread identity;
- Thread history;
- Turns and items;
- runtime status;
- active flags;
- approvals;
- user-input requests;
- model/provider;
- sandbox/permission state;
- Goal state;
- Thread Queue where available;
- resume/fork semantics;
- upstream Project / Section state where available.

codex-tui does not maintain a second canonical conversation database.

## 3.2 Git owns code state

Git is authoritative for:

- repository identity;
- branches;
- worktrees;
- index/working-tree state;
- diffs;
- commits;
- merge relationships.

codex-tui never treats a cached Git projection as authority.

## 3.3 Code forge owns team planning/review/delivery

Normalized forge concepts:

- ForgeWorkItem;
- ForgeBoard;
- ChangeRequest;
- PipelineSummary;
- ReviewState.

Providers:

- GitLab Self-Managed first;
- GitHub second.

Provider terminology stays at the edge:

- GitLab -> Issue / Work Item / Issue Board / Merge Request / Pipeline;
- GitHub -> Issue / Project / Pull Request / Checks.

## 3.4 Repository files own reusable development convention

Use existing mechanisms first:

- AGENTS.md;
- .codex/config.toml;
- skills;
- plugins;
- scripts;
- CI.

A future .codex-tui.toml is allowed only for truly codex-tui-specific shared presentation/preset data that cannot be represented elsewhere.

## 3.5 codex-tui owns only operator state

Local state can include:

- draft;
- scroll/follow state;
- pin;
- alias;
- note;
- bookmark;
- hot slot;
- snooze;
- seen/unseen;
- SavedView;
- ScratchWork;
- WorkCard relationship mapping;
- managed-worktree metadata;
- recent operation receipts.

---

# 4. Final product architecture

~~~text
┌─────────────────────────────────────────────────────┐
│ Layer 6: Optional Extensions                        │
│ control protocol / declarative integrations/plugin │
├─────────────────────────────────────────────────────┤
│ Layer 5: Optional Remote / Collaboration            │
│ remote target / web-mobile / presence / bridge     │
├─────────────────────────────────────────────────────┤
│ Layer 4: Repository Reuse + Code Forge              │
│ GitLab first / GitHub second / repo conventions    │
├─────────────────────────────────────────────────────┤
│ Layer 3: Personal Planning                          │
│ WorkCard / Board / List / Goal / Saved Views       │
├─────────────────────────────────────────────────────┤
│ Layer 2: Personal Engineering Workspace             │
│ Git / Worktree / Diff / Review / Commands          │
├─────────────────────────────────────────────────────┤
│ Layer 1: Personal Codex Control                     │
│ Threads / Attention / Conversation / Approval      │
└─────────────────────────────────────────────────────┘
                         │
                   Codex App Server
~~~

Layers 1–3 form the mature product core.

Layer 4 is a high-value integration/reuse layer.

Layers 5–6 are optional and may never be required for many users.

---


## 4.1 Runtime-service invariant

The normal local product does not introduce its own mandatory long-running codex-tui daemon.

It may connect to or rely on an official Codex-managed App Server/daemon topology when supported upstream.

Any future codex-tui background service must be optional, independently justified and must not become required for the personal core.

# 5. Core identities

Identity mistakes create the most expensive long-term bugs. Keep these distinct.

## 5.1 CodexTarget

Infrastructure identity, not a project identity.

Represents:

- local embedded/local daemon/explicit app-server target;
- CODEX_HOME;
- backend fingerprint;
- platform;
- capabilities.

The UI may expose this in diagnostics or multi-profile settings later.

## 5.2 Workspace

Technical local project grouping.

Resolution preference:

1. stable upstream Codex Project id when available and trustworthy;
2. local Git repository identity;
3. normalized cwd;
4. explicit local registration.

Workspace is not a task/project-management object.

## 5.3 LocalRepoIdentity

Must distinguish repository from worktree.

Suggested local identity:

- canonical Git common-dir identity;
- primary root;
- current worktree path.

Do not use cwd alone.

## 5.4 ForgeIdentity

Fields:

- provider;
- host;
- stable project/repository id when available;
- namespace/path for display and resolution;
- current web URL.

For GitLab, numeric project ID is preferred as stable remote identity because project paths can change when renamed or transferred.

Path remains metadata, not sole identity.

## 5.5 ThreadIdentity

Canonical Codex thread_id.

Never use:

- display title;
- rollout mtime;
- cwd;
- terminal process id

as a substitute for thread identity.

## 5.6 WorktreeIdentity

At minimum:

- LocalRepoIdentity;
- canonical worktree path;
- branch;
- managed_by_codex_tui flag.

## 5.7 WorkCardIdentity

A WorkCard has:

- stable local_id;
- exactly one primary anchor;
- links[];
- overlays;
- derived workflow stage;
- attention reasons.

Primary anchor is one of:

- ScratchWork;
- ForgeWorkItem;
- CodexThread.

Goal, Worktree and ChangeRequest normally become links.

This prevents duplicate cards when a local idea becomes a GitLab Issue, then a Codex thread, then an MR.

---

# 6. WorkCard model

Suggested normalized model:

~~~text
WorkCard
  local_id

  anchor
    kind
    source_ref

  links[]
    role
    source_ref

  presentation
    local_title_override?
    note?
    pin?
    tags?

  planning
    manual_ready?
    priority?
    snooze_until?

  derived
    workflow_stage
    stage_reason
    attention[]
    freshness
~~~

Useful link roles:

- primary_thread;
- experiment_thread;
- review_thread;
- goal;
- worktree;
- change_request;
- related_work_item.

Do not over-model link roles initially; add only those that improve UX.

---


## 6.1 Anchor promotion and uniqueness

WorkCard `local_id` is permanent local relationship identity.

The anchor never changes automatically because of discovery.

Explicit promotion is allowed, for example:

~~~text
ScratchWork
  -- explicit "promote/link to GitLab Issue" -->
ForgeWorkItem
~~~

Promotion rules:

- preserve the same `local_id`;
- store the previous anchor as a historical/related link when useful;
- require an explicit user operation when authoritative ownership changes;
- enforce that one external source reference maps to at most one active WorkCard;
- never merge two WorkCards only because titles look similar;
- duplicate-card merge is an explicit operation with a preview.

If a Codex thread was the original anchor and a forge WorkItem is later linked, the Thread remains the anchor unless an explicit promotion operation changes it.

This avoids silent identity churn while still allowing a personal Scratch item to graduate into shared team work.

# 7. Workflow and Attention are separate state machines

This is a critical correction from earlier designs.

## 7.1 Workflow stage

Canonical planning projection:

~~~text
Inbox -> Ready -> Working -> Review -> Done
~~~

### Inbox

Unplanned or not-yet-selected work.

Typical anchors:

- ScratchWork;
- imported forge WorkItem;
- a thread with no current active work that the user wants to triage.

### Ready

Selected to do, not currently executing.

### Working

Active execution or active Goal.

### Review

Implementation has produced evidence/code that needs human inspection or forge review.

### Done

Explicitly delivered/acknowledged terminal state.

Do not infer Done merely because a thread is idle or unloaded.

## 7.2 Attention overlay

Attention can apply to any workflow stage.

Types include:

- ApprovalRequired;
- UserInputRequired;
- GoalBlocked;
- SystemError;
- UsageLimited;
- BudgetLimited;
- CompletionUnseen;
- ReviewUnseen;
- ConflictRisk;
- PipelineFailed;
- ChangeRequested.

A card can be:

~~~text
stage = Working
attention = ApprovalRequired
~~~

or:

~~~text
stage = Review
attention = PipelineFailed
~~~

## 7.3 Needs You

Needs You is:

- an Attention Inbox;
- a filter;
- a virtual swimlane;
- an ordering rule.

It is not a canonical workflow column.

## 7.4 Explainability

Every derived stage/attention state must provide a reason.

Example:

~~~text
Working
because: Codex thread status is Active

Needs You
because: WaitingOnApproval from thread 01...

Review
because: Goal Complete + dirty worktree + no accepted review
~~~

The UI should make this reason inspectable.

---

# 8. Provenance and freshness

codex-tui merges observations from different systems with different update semantics.

Every externally derived projection should retain:

- source;
- observed_at;
- source revision/identifier when available;
- freshness state;
- error/degraded reason.

Suggested freshness:

- Fresh;
- Aging;
- Stale;
- Unavailable.

Example:

~~~text
MR !392   Review   Pipeline Passed   4m ago
~~~

If GitLab becomes unavailable:

~~~text
MR !392   Review   Pipeline Passed   STALE · last checked 18m ago
~~~

Do not silently present stale forge data as current.

## 8.1 Refresh policy

Codex:
- event-driven notifications;
- lightweight reconciliation on reconnect.

Git:
- refresh selected/visible workspaces;
- refresh after known mutations;
- bounded periodic or filesystem-triggered refresh if needed.

Forge:
- on-demand;
- visible/pinned item refresh;
- bounded TTL;
- no global aggressive polling.

This protects rate limits and keeps startup fast.

---

# 9. Reconciliation engine

Create a dedicated Projection/Reconciliation layer.

Inputs:

- Codex observations;
- Git observations;
- Forge observations;
- LocalStore overlays.

Output:

- Workspace projection;
- ThreadRef projection;
- WorkCard projection;
- Attention projection;
- Review projection.

The reconciler must be deterministic and pure where possible.

Pseudo-flow:

~~~text
Observations
  -> normalize identities
  -> link known relationships
  -> apply local overlays
  -> derive workflow stage
  -> derive attention
  -> attach provenance/freshness
  -> publish projection
~~~

This layer is more important than adding more UI panels.

---

# 10. Relationship discovery

Relationships are not always explicit.

Use ordered evidence.

## Thread -> Workspace

1. upstream Project;
2. Git identity from cwd;
3. cwd fallback.

## Thread -> Worktree

Resolve cwd against Git worktree inventory.

## Worktree -> Forge repository

1. explicit local mapping;
2. current branch upstream remote;
3. configured preferred remote;
4. origin if uniquely valid;
5. single recognized forge remote;
6. otherwise ambiguous -> require user selection.

Never silently choose between multiple eligible remotes.

## Thread/WorkCard -> ChangeRequest

Evidence order:

1. explicit stored relationship;
2. upstream Codex attachment if compatible;
3. forge lookup by exact source branch/project;
4. otherwise none.

Avoid title-based guessing.

---

# 11. Planning / Kanban

Kanban remains part of the mature core, but as a view.

## 11.1 Board

Default workflow columns:

- Inbox;
- Ready;
- Working;
- Review;
- Done.

Optional virtual lanes:

- Needs You;
- Blocked;
- Pipeline Failed;
- Recently Completed.

## 11.2 Saved Views

A SavedView defines:

- source scope;
- filter;
- group;
- order;
- layout;
- visible fields.

Layouts:

- list;
- board;
- review queue.

Examples:

- Attention;
- Today;
- KWS;
- Audio;
- Active Research;
- Needs Review;
- Pipeline Failed;
- Ready to Merge;
- Recently Completed.

## 11.3 Local ScratchWork

Keep intentionally small:

- id;
- title;
- note;
- workspace?;
- priority?;
- state = inbox/ready/done;
- linked anchor/ref?.

If it becomes team work, promote/link it to a forge WorkItem.

Do not grow local Scratch into Jira.

---

# 12. Goal integration

Goal-aware UX is a mature-core feature because Goals provide persistent thread-scoped completion contracts.

However it remains capability-optional.

When supported:

- display objective;
- lifecycle status;
- token budget;
- usage;
- elapsed time;
- pause/resume/clear/set actions where valid;
- use Goal state in WorkCard derivation.

When unsupported:

- no local fake Goal engine;
- Board remains functional using Thread/Git/Forge state.

A Goal is not the same thing as a WorkCard:

- WorkCard = operator planning projection;
- Goal = Codex thread-scoped execution objective.

---

# 13. Thread Queue integration

Thread Queue is useful for quick follow-up instructions and queued work, but upstream APIs may remain experimental.

Policy:

- integrate when capability exists;
- do not make it a baseline dependency;
- keep Quick Prompt usable without it;
- queue UI must clearly distinguish queued input from active Goal/WorkCard.

Do not turn Thread Queue into the global planning board.

---

# 14. Mission Control

Default opening surface.

Primary question:

> What needs my attention now?

Suggested row information:

- attention marker;
- workspace;
- thread/card title;
- workflow stage;
- live thread state;
- Goal summary;
- worktree/branch;
- change-request/pipeline hint;
- recency;
- stale/degraded badge.

Primary interactions:

- navigate;
- open exact thread;
- attention jump;
- quick prompt;
- search/filter;
- pin;
- snooze;
- mark unread;
- create/start/fork;
- open Board;
- open Review;
- command palette.

Manual selection is sticky.

Background refresh must never steal the user's selection.

---

# 15. Thread view

Primary question:

> What is this Codex thread doing, and what should I tell it next?

Components:

- header;
- Goal/status line;
- virtualized transcript;
- structured tool items;
- approvals/user-input UI;
- composer;
- footer with cwd/model/permission/worktree.

Per-thread local UI state:

- draft;
- scroll anchor;
- follow-bottom;
- expanded tool items;
- selected detail tab;
- last opened revision.

Switching threads should feel like switching editor buffers.

---

# 16. Quick Prompt

From Mission Control or Board, allow a short message without opening full Thread view.

Example:

~~~text
"tests pass后整理MR描述"
~~~

After submit:

- remain in current list/board;
- show queued/sent state;
- update attention/live status.

This is a high-value personal workflow feature.

---

# 17. Review workspace

Review is first-class.

Primary question:

> What changed and is it ready?

Content:

- changed-file list;
- line diff;
- word diff;
- Codex review findings;
- test/check evidence;
- ChangeRequest;
- Pipeline;
- review/approval status;
- unresolved discussions when provider capability supports it;
- links back to Thread/Goal.

Do not produce a hidden "ready/not ready" verdict unless it derives from explicit user/project criteria.

Prefer showing evidence.

---

# 18. Git/worktree model

## 18.1 Awareness is core

Always support:

- repository detection;
- worktree detection;
- branch;
- dirty state;
- changed-file count/summary.

## 18.2 Collision detection

If two active editing threads share one mutable checkout:

~~~text
! 2 active editing threads share /repo/main
~~~

This is Attention/ConflictRisk.

## 18.3 Managed worktrees

Mature core may create/fork/cleanup worktrees.

Rules:

- Git mutations serialized per LocalRepoIdentity;
- explicit argv/cwd;
- dirty destructive cleanup refused by default;
- distinguish user-owned and codex-tui-managed worktrees;
- interrupted operations reconciled on restart;
- branch deletion separate from worktree removal;
- no force deletion hidden behind "cleanup".

Use Git CLI behind a GitService abstraction initially; avoid unnecessary libgit2 portability burden.

---


## 18.4 Mutation scope and collision detection

Collision detection must not rely only on `thread.cwd`.

A thread can potentially write through additional runtime workspace roots or permission grants.

Define a derived `MutationScope`:

- writable repository/worktree roots known from Codex runtime/permission capabilities;
- runtime workspace roots when available;
- cwd/worktree as the conservative fallback;
- explicit additional writable roots when exposed by upstream permission state.

ConflictRisk is raised when active editing scopes overlap on the same mutable filesystem/repository state.

If precise write scope is unavailable, prefer a conservative warning over a false claim of isolation.

# 19. Code forge abstraction

## 19.1 ForgeProvider

Internal contract:

~~~text
ForgeProvider
  detect(remote)
  health()
  capabilities()
  resolve_repository(remote)

  list_work_items(...)
  get_work_item(...)

  list_boards(...)
  get_board(...)

  list_change_requests(...)
  get_change_request(...)

  get_pipeline_summary(...)
  get_review_state(...)
  list_discussions(...)

  open_web(...)
~~~

Mutations are a separate capability surface.

## 19.2 GitLab Self-Managed first

Initial provider:

~~~text
GitLabForgeProvider
  -> glab / glab api
  -> internal GitLab
~~~

Why:

- reuse existing auth;
- support self-managed hosts;
- support multiple hosts;
- avoid codex-tui token storage;
- structured JSON/NDJSON;
- API escape hatch.

Doctor reports:

- glab path/version;
- authenticated hosts;
- selected host;
- project identity;
- discovered capabilities.

## 19.3 GitLab version/edition capability

Do not assume internal GitLab equals latest GitLab.com.

Capability examples:

- supports_issue_boards;
- supports_work_items;
- supports_mr_approval_state;
- supports_mr_discussions;
- supports_pipeline_details.

Prefer stable APIs for baseline.

Experimental GitLab WorkItem GraphQL fields and experimental glab commands are enhancements, not required baseline.

## 19.4 GitHub second

GitHub implements the same normalized contract.

No core UI code checks provider == github or provider == gitlab.

---


## 19.5 Forge provider availability

The forge layer is optional for core personal use.

If `glab` is absent, unauthenticated, incompatible or the GitLab host is unreachable:

- Codex/Git/Planning remain usable;
- forge features are disabled or marked unavailable/stale;
- Doctor provides exact remediation;
- startup does not fail.

GitLab tier-specific capabilities are discovered and surfaced rather than assumed.

# 20. Safe mutation model

All significant Git/Forge mutations use:

~~~text
Plan
  -> Confirm when user-impacting
  -> Execute
  -> Verify
  -> Receipt
~~~

Examples:

- create worktree;
- remove worktree;
- create branch;
- create/update forge WorkItem;
- create ChangeRequest;
- approve;
- merge.

OperationPlan includes:

- provider/service;
- target identity;
- exact command/API operation;
- expected side effect;
- safety preconditions.

OperationReceipt includes:

- timestamp;
- operation;
- target;
- resulting ID/SHA/ref;
- verification result;
- failure details.

Recent receipts are useful for diagnostics and recovery.

Read-only actions do not need confirmation.

---


## 20.1 Idempotency, retries and unknown outcomes

External mutations can fail after the remote side has already applied the operation.

Therefore a mutation must never be blindly retried.

Each significant mutation receives a local `operation_id` and a lifecycle such as:

- Planned;
- Executing;
- Succeeded;
- Failed;
- OutcomeUnknown.

Before retrying an OutcomeUnknown operation, reconcile the target system first.

Examples:

- after an MR-create timeout, search for an MR matching the exact source branch/project before creating another;
- after worktree-create interruption, inspect Git worktree state before retrying;
- after forge WorkItem creation timeout, reconcile using returned/stored identifiers or carefully bounded matching rather than immediately creating a duplicate.

OperationReceipt records the reconciliation result.

Where an upstream API provides an idempotency mechanism, use it. Otherwise codex-tui implements reconcile-before-retry semantics.

# 21. LocalStore

Do not let JSON/SQLite shape leak into domain code.

Interface concepts:

- load/save ViewState;
- SavedView CRUD;
- ScratchWork CRUD;
- WorkCard relationship mapping;
- managed-worktree metadata;
- recent operation receipts.

Initial backend:

- atomic JSON/TOML;
- file lock if needed.

Mature backend:

- SQLite when complexity justifies it.

Migration requirements:

- schema version;
- transaction;
- backup before destructive migration;
- integrity check;
- safe read-only/repair mode on failure.

Still never own canonical Codex transcript.

---


## 21.1 Local state privacy

LocalStore can contain project paths, notes and unsent drafts.

Requirements:

- user-only filesystem permissions where the platform supports them;
- atomic writes;
- no automatic cloud synchronization;
- secrets/tokens are never stored;
- diagnostics redact sensitive paths/content by default;
- encryption-at-rest is not a baseline requirement, but the storage abstraction must not prevent a future secure backend.

# 22. Search

## 22.1 Metadata search is core

Search:

- workspace;
- thread title/preview;
- alias;
- Goal objective;
- branch/worktree;
- WorkCard title/note;
- forge work item;
- change request;
- status/stage;
- pin/tag.

Query examples:

~~~text
status:needs-you
stage:review
project:kws
branch:research
forge:gitlab
mr:open
goal:decoder
~~~

## 22.2 Full transcript search is optional

Add only when measured need justifies index complexity.

If added:

- local optional FTS index;
- canonical transcript stays in Codex;
- index is disposable/rebuildable.

---

# 23. Personal productivity features

Mature core:

## Notes

Small local note attached to Thread/WorkCard.

## Bookmarks

Bookmark turn/item/review point.

## Hot slots

Fast mappings to frequently used threads/work cards.

## Pin

Persistent importance overlay.

## Mark unread

Local attention overlay.

## Snooze

Suppress attention rotation until deadline without changing real status.

## Recent targets

Fast switching history.

These features are high personal value and low system-authority risk.

---

# 24. Responsive TUI design

Do not assume a 120-column terminal.

Suggested modes:

## Compact: < 80 columns

- one primary pane;
- overlays/details on demand;
- no persistent side panel.

## Standard: 80–119

- registry/thread with compact contextual footer;
- review/file list switches rather than permanent columns.

## Wide: >= 120

- optional secondary context/review pane;
- richer metadata.

Layout is responsive by capability/width, not by platform name.

---

# 25. Terminal subsystem

Dedicated ownership:

~~~text
terminal/
  identity
  capabilities
  keyboard
  palette
  screen
  mouse
  clipboard
  probes
  quirks
~~~

All external probes are:

- bounded;
- cancellable;
- traced;
- non-critical.

Baseline supports standard keyboard/input without Kitty/CSI-u requirements.

Enhanced keyboard protocols are opportunistic.

No Nerd Font required.

---

# 26. Command/keymap architecture

~~~text
Raw event
 -> normalized KeyChord
 -> Command ID
 -> Action
~~~

Command ID examples:

- registry.next_attention;
- registry.open;
- thread.quick_prompt;
- thread.interrupt;
- board.open;
- review.open;
- workspace.open;
- forge.refresh;
- app.command_palette.

Command registry drives:

- key binding;
- help;
- command palette;
- footer hints;
- remapping.

Do not scatter raw key comparisons across widgets.

---

# 27. Application architecture

Use Action / Reducer / Effect.

~~~text
External event
  -> Action
  -> domain Reducer
  -> AppState
      -> render
      -> Effect
          -> async IO
          -> Action
~~~

Reducers are pure.

Effects own:

- App Server RPC;
- Git;
- glab/gh;
- filesystem;
- LocalStore;
- notifications;
- process spawning.

Rendering never blocks.

---

# 28. Module shape

Start as one Cargo crate.

Target module structure:

~~~text
src/
  app/
    action
    reducer
    effect
    navigation

  domain/
    workspace
    thread
    attention
    workcard
    observation
    operation

  codex/
    client
    protocol
    compat
    capability

  projection/
    reconcile
    workflow
    attention
    relationships

  planning/
    saved_view
    scratch

  git/
    service
    repo
    worktree
    diff

  forge/
    provider
    gitlab
    github

  review/

  store/

  terminal/

  ui/
    components
    layout
    keymap
    theme
    compositor

  doctor/
  cli/
~~~

Do not create empty modules merely to match the target tree. Extract as implementation pressure appears.

Module-size ratchet:

- target < ~500 LOC per implementation module;
- at ~800 LOC, new feature work should usually extract a boundary.

---

# 29. Backend separation

CodexBackend implementations:

- AppServerBackend;
- FakeBackend;
- ReplayBackend.

ForgeProvider implementations:

- GitLabGlabProvider;
- GitHub provider later;
- fixture provider for tests.

GitService:

- command-backed initial implementation;
- fake/fixture implementation for tests.

LocalStore:

- file backend initially;
- SQLite backend later.

UI only consumes normalized domain/projections.

---

# 30. Compatibility strategy

## Codex

- capability negotiation;
- stable baseline without experimental API requirement;
- per-feature fallback;
- unknown event fail-soft;
- fixture matrix;
- backend fingerprint;
- preview channel for future Codex changes.

## GitLab

- detect server version/capabilities when practical;
- stable Issues/MR/Pipeline baseline;
- experimental WorkItem GraphQL features gated;
- experimental glab discussions gated;
- handle Free/Premium/Ultimate feature differences;
- handle custom host and custom CA through glab/system config.

## GitHub

- same ForgeProvider contract;
- optional when not used.

---

# 31. Degraded mode

A failure in one source must not collapse the whole product.

Examples:

## Codex unavailable

Git/Forge planning/review may remain visible where cached/current data exists; conversation controls disabled.

## Forge unavailable

Codex/Git remain fully usable; forge fields show stale/unavailable.

## Git unavailable/not a repo

Conversation and planning still work; workspace shows no Git context.

## LocalStore damaged

Start in safe diagnostic/read-only mode when possible; never destroy Codex/Git state.

Feature degradation is explicit, not hidden.

---

# 32. Performance model

## Startup

Critical path:

1. terminal ready;
2. load tiny LocalStore;
3. initialize Codex;
4. render thread metadata.

Optional work:

- forge refresh;
- Git detail scan;
- update check;
- remote probes.

Optional work must not delay first usable frame.

## Registry

Metadata first.

No transcript hydration at startup.

## Transcript

Paginated and virtualized.

## Tool output

- bounded preview;
- lazy expand;
- virtualized;
- never pre-render multi-MB output.

## Forge

Visible/pinned refresh only plus TTL.

## Targets

Refine after measurement, but initial engineering goals:

- key-to-frame p95 around <50 ms under normal load;
- 10k metadata rows remain navigable;
- no unbounded async queues;
- no external subprocess without timeout policy.

---

# 33. Backpressure

Classify event channels.

Lossless/bounded:

- approvals;
- user commands;
- state transitions;
- completion;
- errors;
- operation receipts.

Coalescible:

- render dirty;
- spinner/animation ticks;
- streaming visual refresh;
- repeated same-source refresh requests.

Never coalesce/reorder semantic Codex events merely for rendering convenience.

---

# 34. Accessibility

Baseline:

- no state conveyed only by color;
- reduced motion;
- static fallback for animation;
- keyboard-first;
- CJK;
- combining marks;
- representative emoji;
- no Nerd Font requirement;
- dark/light/no-color snapshots.

Mature:

- screen-reader/quiet presentation mode;
- reduced redraw strategy;
- no shimmer.

---

# 35. Security and privacy

## Credentials

codex-tui does not store Codex/GitLab/GitHub tokens.

Reuse:

- Codex auth;
- glab auth;
- gh auth;
- Git credential mechanisms.

## Logs

Default logs exclude:

- prompt plaintext;
- tool output;
- credentials;
- secret environment values.

## Approval UI

Show exact relevant data:

- operation;
- command/path/host;
- cwd;
- reason;
- escalation;
- decision scope.

## Forge mutations

Explicit and attributable.

## Local control protocol, if later

- local-user only by default;
- versioned;
- explicit method allowlist;
- no internal Action enum on wire.

---

# 36. CLI/headless companion

The same binary should eventually expose useful read-only/headless commands.

Examples:

~~~text
codex-tui status
codex-tui status --json
codex-tui thread list
codex-tui attention list
codex-tui board list
codex-tui doctor
codex-tui forge status
codex-tui worktree list
~~~

Benefits:

- scripting;
- CI/debugging;
- future Agent Skill integration;
- easier tests;
- no need for a plugin system merely to automate common workflows.

Mutating headless commands follow the same OperationPlan/Receipt rules.

A future Codex skill can teach the agent to use this CLI after semantics stabilize.

---

# 37. Notifications

Keep lightweight.

Sources:

- approval required;
- user input required;
- Goal blocked;
- completion;
- pipeline failure;
- review requested.

Controls:

- off;
- terminal only;
- OS notification;
- optional external bridge later.

Snooze affects routing, not source status.

Do not build a notification platform.

---

# 38. Testing strategy

## Pure unit tests

- reducers;
- identity normalization;
- relationship linking;
- stage derivation;
- attention derivation;
- freshness;
- SavedView filtering;
- capability negotiation;
- keymap;
- operation plans;
- migration logic.

## Protocol fixtures

Codex payload generations.

## Forge fixtures

GitLab:

- Issues;
- Boards;
- MR;
- Pipeline;
- approval states;
- version/edition capability cases.

GitHub later.

## Contract tests

Every ForgeProvider must satisfy a provider conformance suite.

## Snapshot tests

Widths:

- 40;
- 80;
- 120;
- 160.

Content:

- CJK;
- long path;
- long branch;
- stale forge data;
- Board;
- Review;
- Approval;
- error/degraded states.

## PTY E2E

- startup/exit;
- Ctrl-C;
- panic cleanup;
- resize;
- paste;
- editor handoff;
- tmux;
- Zellij;
- Windows/WSL where possible.

## Scheduled compatibility

Codex:

- minimum supported;
- previous stable;
- current;
- optional prerelease.

GitLab:

- fixture compatibility first;
- optional scheduled integration against representative Self-Managed version if infrastructure exists.

Do not require paid live model inference for basic protocol compatibility.

---

# 39. Doctor

Doctor is a core product surface.

Commands:

~~~text
codex-tui doctor
codex-tui doctor --json
codex-tui doctor codex
codex-tui doctor git
codex-tui doctor forge
codex-tui doctor terminal
codex-tui doctor store
~~~

Report:

- codex-tui version;
- Codex path/version;
- App Server target/fingerprint;
- CODEX_HOME;
- capability table;
- Git version/repo;
- remotes and forge resolution;
- glab path/version/authenticated hosts;
- GitLab project id/path/server capability;
- terminal environment;
- state-store schema/integrity;
- recent source errors;
- recent operation receipts.

Redact secrets and personal prompt/tool content.

---

# 40. Release and maintenance

Targets:

- Linux x86_64 GNU;
- Linux x86_64 musl;
- macOS arm64;
- macOS x86_64;
- Windows x86_64;
- Linux arm64 when useful.

Channels:

- stable;
- preview.

Preview is especially useful for Codex protocol changes.

Dependency governance:

- cargo-deny;
- RustSec/cargo-audit;
- dependency updater;
- minimal GitHub Actions permissions;
- license registry;
- Actions static analysis.

Reference projects with restrictive licenses remain conceptual references only.

---


# 40.1 Documentation authority

To prevent design drift:

1. `docs/design/final-plan.md` is the canonical current architecture baseline.
2. Accepted ADRs are binding decisions and override older descriptive documents.
3. Current implementation design documents must conform to the final plan and ADRs.
4. `docs/research/` is historical evidence, not normative architecture.
5. Superseded design documents should carry a clear superseded/non-authoritative notice rather than silently coexist as competing specifications.
6. Any implementation PR that intentionally violates a binding ADR must update/supersede that ADR in the same change.

# 41. Feature tiers

## Tier A — Mature personal core

- Mission Control;
- Attention Inbox;
- exact Thread control;
- Goal-aware UI;
- Quick Prompt;
- Board/List Saved Views;
- WorkCard projection;
- Git/worktree awareness;
- Review/Diff;
- Search/Command Palette;
- pin/alias/snooze/unread;
- notes/bookmarks;
- hot slots;
- lightweight notifications;
- Doctor;
- read-only/headless CLI.

## Tier B — Personal engineering maturity

- managed worktree lifecycle;
- richer review evidence;
- local ScratchWork;
- terminal drawer;
- batch local actions;
- launch presets;
- optional Thread Queue;
- optional transcript FTS.

## Tier C — Team reuse / forge

- GitLab Self-Managed;
- Work Items/Issues;
- Issue Boards;
- Merge Requests;
- Pipelines;
- approval/review state;
- explicit mutations;
- GitHub provider.

## Tier D — Optional external layers

- remote App Server;
- web/mobile companion;
- shared presence/collaboration;
- multi-agent providers;
- agent-to-agent messaging;
- plugin runtime;
- job/workflow engine;
- organization analytics;
- cost platform.

---

# 42. Explicit non-goals

Even in target state, codex-tui does not become:

- model runtime;
- independent agent loop;
- custom sandbox/policy engine;
- Git replacement;
- GitLab/GitHub replacement;
- Jira/Linear replacement;
- enterprise IAM/RBAC server;
- cloud transcript warehouse;
- universal terminal shim for every AI tool;
- mandatory team collaboration service.

---

# 43. Delivery roadmap

## M0 — Architecture skeleton

Goal:
prove the local application architecture.

Deliver:

- Rust/Ratatui/Tokio/Crossterm;
- Action/Reducer/Effect;
- FakeBackend;
- domain identities;
- LocalStore interface + file backend;
- static Mission Control;
- Thread view;
- Attention projection;
- terminal guard;
- snapshot harness;
- cross-platform CI.

## M1 — Real read-only Codex registry

Deliver:

- App Server initialize;
- capabilities;
- thread/list pagination;
- loaded/status notifications;
- workspace derivation;
- sticky selection;
- search/filter;
- pin/alias;
- Doctor Codex.

Acceptance:
useful before a single prompt is sent.

## M2 — Daily conversation control

Deliver:

- thread read/resume;
- paginated turns/items;
- composer;
- start/steer/interrupt;
- approvals;
- user input;
- per-thread draft/scroll;
- Quick Prompt.

## M3 — Git and Review

Deliver:

- LocalRepoIdentity;
- worktree detection;
- branch/dirty;
- changed files;
- collision detection;
- Review/Diff;
- external editor.

## M4 — Personal Planning

Deliver:

- WorkCard anchor/link model;
- workflow derivation;
- Attention orthogonality;
- provenance/freshness;
- Goal projection;
- Saved Views;
- Board/List;
- ScratchWork;
- notes/bookmarks;
- snooze/unread;
- hot slots.

## M5 — Safe parallel engineering

Deliver:

- managed worktrees;
- per-repo mutation lock;
- OperationPlan/Receipt;
- recovery/reconciliation;
- notifications;
- optional terminal drawer.

## M6 — GitLab Self-Managed

### M6a read-only

- forge detection;
- host/project resolution;
- glab doctor;
- Issues/Work Items baseline;
- Issue Boards;
- Merge Requests;
- Pipelines;
- approval/review summaries;
- WorkCard relationships;
- stale/degraded behavior.

### M6b explicit mutations

Only high-value mutations:

- create/update WorkItem where justified;
- create MR;
- comment;
- approve;
- merge.

Every mutation uses plan/verify/receipt.

### M6c GitHub

Implement same ForgeProvider contract.

## M7 — Scale and polish

- optional FTS;
- richer SavedView query language;
- batch actions;
- launch presets;
- performance budgets;
- accessibility;
- compatibility matrix;
- stable/preview release process;
- CLI/headless surface stabilization.

## Optional later tracks

- remote targets;
- collaboration service;
- web/mobile;
- plugins;
- multi-agent;
- jobs/workflows.

---

# 44. Architecture gates for future proposals

Any new feature must answer:

1. What user problem does it solve for one developer?
2. Which existing system is authoritative?
3. Can it be derived rather than persisted?
4. Does it require a new service?
5. Does it create a stable compatibility promise?
6. What happens offline or when the source is stale?
7. How does it degrade when capability is unavailable?
8. How is identity preserved across rename/move/restart?
9. What are the tests?
10. How is it deprecated?
11. Does official Codex already provide this capability?
12. Is this better as a Forge/repository integration rather than core?

If these answers are weak, defer the feature.

---

# 45. Final success criteria

## Personal

A developer can operate 5–20 concurrent pieces of Codex work across multiple repositories without relying on remembered terminals, thread IDs or manual status tracking.

The application makes:

- attention obvious;
- switching cheap;
- parallel edits safe;
- review fast;
- recovery understandable.

## Team

A team can adopt codex-tui without deploying a new team service.

Shared value comes through:

- repository instructions/config/skills/scripts;
- Git;
- GitLab/GitHub;
- common conventions.

## Maintenance

The project can evolve alongside Codex and GitLab without permanent version pinning or duplicated authority.

---

# 46. Final positioning

> codex-tui is a personal-first, local-first Codex engineering workbench for planning, monitoring, controlling and reviewing many concurrent pieces of AI-assisted development across repositories.

It uses official Codex state for execution, Git for code, and GitLab/GitHub for shared engineering workflow.

Its job is not to replace those systems.

Its job is to make one engineer dramatically better at operating all of them together.
