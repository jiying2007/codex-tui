# codex-tui target state

Date: 2026-09-29
Status: target architecture

## Product definition

codex-tui is a local-first Codex engineering workbench that connects planning, parallel execution, attention routing, conversation control, Git/worktree context and review in one terminal.

It is not a replacement for Codex, Git or GitHub. It is the operator surface across them.

## Target user outcomes

An individual developer or small team member should be able to:

1. see all relevant work across repositories;
2. distinguish planned, running, blocked, reviewable and completed work;
3. know immediately what needs human attention;
4. jump into the exact Codex thread without remembering IDs or terminals;
5. keep drafts/reading position while switching among threads;
6. run parallel implementation safely in worktrees;
7. inspect diffs and review agent work;
8. connect a thread to its issue/PR/goal;
9. continue planning without leaving the terminal;
10. recover and diagnose problems without understanding codex-tui internals.

## Authority model

~~~text
Codex App Server
  threads / turns / items / goals / approvals / runtime / model
             |
             v
          codex-tui
             ^
             |
Git ---------------- code / worktrees / branch / diff
GitHub ------------- issue / PR / CI / team planning
Repo config -------- AGENTS.md / .codex configuration
Local ViewState ---- drafts / pins / aliases / views / scratch
~~~

No duplicated canonical transcript or Git state.

## Target layers

### Layer 1 — Codex Control Core

Always present.

Capabilities:

- backend compatibility/capability negotiation
- workspace/thread registry
- attention state
- conversation
- approvals
- interrupt/steer
- resume/fork
- goals
- model/sandbox/permission visibility
- diagnostics

This layer must work with no network service owned by codex-tui.

### Layer 2 — Developer Workspace

Always present in the mature product.

Capabilities:

- Git repository/worktree context
- worktree collision detection
- managed worktree lifecycle
- changed files and diff
- review workspace
- external editor
- lightweight terminal drawer / commands
- notifications
- saved views/search

### Layer 3 — Planning

Mature core, but authority-aware.

Capabilities:

- list/board views over WorkCards
- Codex Goal projection
- lightweight local Scratch items
- optional thread queue UI
- priorities/pins/snooze
- review queue
- saved custom views

Planning is a view layer over Codex/GitHub/local scratch sources, not a second Jira.

### Layer 4 — Team Integrations

Optional but expected for small-team mature use.

Capabilities:

- GitHub Issues
- GitHub Projects views
- PR/CI/review status
- link issue <-> thread <-> worktree <-> PR
- repository-shared launch/review presets where Codex config does not already cover the need

No team server is required.

### Layer 5 — Remote / Collaboration

Optional separate layer.

Possible capabilities:

- explicit remote App Server targets
- SSH-aware opening
- optional browser/mobile companion
- optional collaboration service
- notifications outside the terminal

The local core cannot depend on this layer.

### Layer 6 — Extensions

Only after stable product semantics exist.

Preferred order:

1. custom commands/actions
2. stable local control protocol
3. declarative integrations
4. only then consider a versioned plugin runtime

No internal Rust types are a public plugin API.

## Primary views

### Mission Control

High-density list of projects/threads with Attention Inbox.

Primary question:

What needs me now?

### Board

Planning/execution/review projection.

Primary question:

What stage is each piece of work in?

### Thread

Transcript + composer + tools/approvals.

Primary question:

What is this Codex thread doing and what should I tell it next?

### Review

Files, diff, findings, PR status.

Primary question:

What changed and is it ready?

### Workspace

Repository/worktrees/branches/dev commands.

Primary question:

Where is this work happening?

### Search / Command Palette

Global navigation and actions.

Primary question:

How do I reach anything in a few keystrokes?

### Doctor

Environment/backend/terminal/Git diagnostics.

Primary question:

Why is this not working?

## WorkCard relationship model

A WorkCard is a projection with references, not necessarily a stored task.

~~~text
WorkCard
  id: local view identity
  title
  source refs:
    issue?
    thread?
    goal?
    worktree?
    pr?
    scratch?
  derived stage
  priority overlay?
  snooze/seen state?
~~~

One source may map to more than one card only when explicitly modeled.

Do not use display titles as stable identity.

## Target board workflow

~~~text
Inbox -> Ready -> Working -> Needs You -> Review -> Done
                   |             |
                   +-------------+
                        resume
~~~

Not every transition is manually draggable. Derived state wins where source authority exists.

## Attention model

Attention is independent from workflow stage.

Examples:

- Working + approval request = Needs You attention
- Review + unseen completion = attention
- Ready + no unseen event = no attention
- Working + snoozed = still Working, omitted from attention rotation until snooze expires

This follows the useful Fleet pattern: snooze is not a fake status.

## Search model

Global search spans metadata first:

- workspace
- thread
- alias
- goal objective
- branch/worktree
- issue/PR
- status/stage
- tags/pins

Optional full transcript search is an index accelerator and can introduce SQLite later.

## Persistence target

Phase 1 uses a tiny versioned state file.

Mature product may use SQLite when required by:

- saved views
- scratch work
- optional full-text index
- more complex local relationships
- concurrent local clients

Even then, Codex transcript and Git data remain external authorities.

## Team model

Small-team target:

~~~text
shared Git repo / GitHub Project
  |
  +-- AGENTS.md / .codex config
  +-- issues / PRs / CI
  |
Developer A codex-tui     Developer B codex-tui
   local sessions            local sessions
   local drafts/views         local drafts/views
~~~

Team-shared work comes from GitHub/repo state. Personal attention/UI state stays local.

A future collaboration layer can add presence or shared live notes, but is not required for normal team adoption.

## Compatibility target

Core baseline must survive upstream change through:

- App Server adapter boundary
- capability negotiation
- stable baseline without experimental APIs
- per-feature fallbacks
- unknown-event fail-soft behavior
- protocol fixture matrix
- backend fingerprint/doctor

Experimental upstream capabilities such as projects/goals/queues can progressively replace local fallbacks as they stabilize.

## Terminal target

Support baseline:

- Linux
- macOS
- Windows
- common terminals

Hardened combinations:

- tmux
- Zellij
- SSH
- WSL
- VS Code terminal
- Ghostty/Kitty/WezTerm/iTerm-style enhanced keyboard where available

All probes are bounded and non-critical.

Accessibility:

- no state conveyed by color alone
- reduced motion
- no Nerd Font requirement
- keyboard-first
- CJK/grapheme tests
- optional screen-reader-friendly presentation as the product matures

## Mature feature matrix

| Capability | Target |
|---|---|
| Multi-project registry | Core |
| Attention Inbox | Core |
| Thread conversation | Core |
| Goal visibility/control | Core |
| List/Board saved views | Core |
| Quick prompt | Core |
| Review/diff | Core |
| Git/worktree | Core |
| Worktree lifecycle | Core |
| Search/palette | Core |
| Pins/aliases/snooze/unread | Core |
| Lightweight notifications | Core |
| Diagnostics | Core |
| GitHub Issue/PR/CI | Team integration |
| Local Scratch work | Planning |
| Notes/bookmarks | Planning |
| Terminal drawer | Developer workspace |
| Thread queue | Progressive, upstream-dependent |
| Full-text history search | Optional local index |
| Token/goal budgets | Informational |
| Web/mobile | Optional layer |
| Remote collaboration | Optional layer |
| Multi-agent | Optional/separate |
| Plugin runtime | Late optional |
| Job engine | Optional/separate |

## Non-goals even in the target state

codex-tui should still avoid becoming:

- its own model runtime
- its own sandbox/policy engine
- a replacement for Git
- a replacement for GitHub/Linear/Jira
- an enterprise identity/RBAC server
- a cloud transcript warehouse
- a universal coding-agent compatibility shim

## Product shape

The mature product should feel like:

~~~text
          Planning / Board
                 |
Mission Control -+- Thread / Conversation
                 |
              Review
                 |
          Git / Worktrees
                 |
            GitHub / PR
~~~

with Codex App Server underneath all conversation/runtime operations.

The distinguishing value is not "more AI". It is reducing operator overhead across many Codex tasks while keeping each existing authority intact.
