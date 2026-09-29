# Kanban and adjacent feature evaluation

Date: 2026-09-29
Status: design decision input

## Executive conclusion

Kanban is useful enough to belong in the long-term product, but it should primarily be a view/projection over existing work objects rather than a new canonical task database.

The useful product loop is:

Plan -> Start Codex work -> Observe -> Needs You -> Review -> Done

The expensive anti-pattern is:

Create a second task authority -> duplicate GitLab/GitHub/Linear/Jira state -> synchronize forever

## Why Kanban is valuable

For AI-assisted development the human bottleneck increasingly shifts toward planning, attention routing and review. Vibe Kanban explicitly centered its product around planning issues, launching agent workspaces, reviewing diffs and shipping.

The board helps when:

- several pieces of work exist before a Codex thread has been created
- multiple threads/worktrees run in parallel
- completed work needs human review before being considered done
- blocked/waiting work must not disappear among idle threads
- an individual developer wants a short horizon view across several repos
- a small team wants a shared view using an existing issue system

## Why a full task system is too expensive

A full task authority implies ownership of:

- task IDs
- workflow/status definitions
- priority
- assignee
- dependencies
- comments
- history
- permissions
- sync/import/export
- team presence
- conflict resolution

GitLab Issue Boards, GitHub Projects, Linear and similar systems already provide these semantics.

The same architectural lesson applies across code-forge/project tools: views should organize existing work rather than create duplicate work records.

codex-tui should follow the same principle.

## Recommended concept: WorkCard

A WorkCard is a UI projection, not necessarily a stored task.

Possible source references:

- Codex Thread
- Codex Thread Goal
- forge Work Item
- forge Change Request
- Git worktree/branch
- Local Scratch item

A card may combine references:

~~~text
WorkCard
  issue: GH-123
  thread: thr_xxx
  goal: "fix decoder boundary regression"
  worktree: ../wt/decoder-boundary
  pr: #456
~~~

The references remain authoritative in their own systems.

## Board columns

Do not blindly use generic Backlog/In Progress/Done.

Recommended developer-oriented board:

### Inbox

Unplanned scratch items, imported issues or threads without an active objective.

### Ready

Work selected for execution but not currently running.

### Working

A live Codex thread/goal is active.

### Review

Execution completed but code/review/PR evidence still needs inspection.

### Done

Goal complete and/or the user explicitly acknowledges delivery/review completion.

Optional Archived is a filter, not a primary column.

## Derived movement

Prefer deriving columns from source state.

Examples:

- Thread active + no blocking flag -> Working
Attention is derived separately from workflow stage. Approval/input requests and blocked/usage-limited/budget-limited Goals produce a Needs You attention overlay while the card remains in its real workflow stage.

- Goal complete + dirty changes/no accepted review -> Review
- change request open with review needed -> Review
- merged change request / explicitly completed scratch item -> Done

Manual drag should only mutate a source when that mutation has a clear authority.

Examples:

- Ready -> Working may create/start a Codex thread
- Working -> Needs You is normally derived and not draggable
- Review -> Done may acknowledge local completion or invoke an explicit GitHub transition
- moving a forge work-item status should require an explicit provider integration and confirmation

Never let a visual drag silently fabricate upstream state.

## Codex-native opportunities

Current Codex has increasingly useful primitives for this layer:

- thread goals with objective/status
- goal states including active, paused, blocked, usage limited, budget limited and complete
- goal update notifications
- thread queue add/list/reorder/delete experimental APIs
- thread project identity
- thread attachments including pull requests
- thread status/active flags
- fork/resume/review

This makes a board projection much more valuable than a generic terminal-task board because it can reflect real Codex state.

## Local Scratch items

A small local scratch list is useful for an individual developer.

Keep it deliberately tiny:

- id
- title
- note/description
- workspace
- priority optional
- status: inbox/ready/done
- linked thread/issue optional

No assignee/RBAC/comments/dependency graph in the local format.

If a scratch item becomes team work, promote/link it to GitHub Issue/Project rather than growing a team task system inside codex-tui.

## Team board

For a small team, the preferred board projects the configured code forge. Current internal priority is GitLab Self-Managed; GitHub follows behind the same provider contract.

The team source of truth remains the configured forge.

Potential views:

- My work
- Needs review
- Agent working
- Blocked on human
- Ready to merge
- Current milestone/project

Do not require a codex-tui collaboration server.

## Saved Views

Kanban should be one layout among saved views, not a separate subsystem.

A saved view is:

- data source scope
- filters
- grouping
- ordering
- layout: list / board
- visible fields

Examples:

- Attention: NeedsYou first, all projects
- Today: pinned + recent + working
- Review: completed threads with dirty changes or open PRs
- KWS: only kws-pipeline workspace
- Board: group by derived workflow stage

This mirrors the mature pattern in GitHub Projects and Linear: one set of work, multiple views.

## Vibe Kanban lesson

Vibe Kanban validates planning + execution workspace + review as a coherent workflow.

Its 2026 shutdown does not prove that Kanban was the wrong product or that complexity caused the shutdown; its announcement says the business could not find a business model it wanted despite substantial free usage.

However, its transition is still instructive: remote kanban issues/comments/projects/organizations were scheduled for removal while local workspaces continued. That strongly supports keeping codex-tui's durable core local and treating hosted/team layers as optional.

## Other useful long-term features

### Tier A: should be in the mature core

- Mission Control / Attention Inbox
- list + board saved views
- Codex Goal integration
- exact thread resume/fork
- quick prompt without losing registry context
- review/diff
- worktree awareness and managed worktree operations
- global metadata search
- command palette
- pin/alias/snooze/mark-unread
- lightweight notifications
- doctor/diagnostics
- terminal-safe keymap/accessibility

### Tier B: strong mature-product additions

- GitLab Work Item/Issue Board/MR/Pipeline projection first
- GitHub Issue/Project/PR/Checks projection through the same forge boundary
- local Scratch tasks
- session/thread notes and bookmarks
- thread hot slots / recent targets
- terminal drawer for dev servers/log tails
- batch actions on selected threads
- reusable launch presets/templates
- optional transcript full-text index
- thread queue UI when upstream API stabilizes
- token/goal budget visibility

### Tier C: optional layers, not core

- web/mobile companion
- remote collaboration service
- generic multi-agent adapters
- agent-to-agent messaging
- workflow/job engine
- universal plugin runtime
- organization analytics
- cost accounting platform
- custom enterprise policy engine

## Feature decision rule

A mature feature belongs in the core when it:

1. directly improves planning, attention, interaction or review of Codex work;
2. can use Codex/Git/configured forge as authority;
3. remains useful to a single local developer;
4. does not require a new always-on service;
5. has a bounded compatibility surface.

Otherwise it should be an optional integration/layer.
