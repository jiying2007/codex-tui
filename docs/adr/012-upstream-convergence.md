<!-- docs-id: upstream-convergence -->
<!-- docs-lang: en -->
# ADR-012: Upstream convergence — Mission Control, not a second Codex CLI
<!-- docs-section: overview -->

**Language / 语言:** [English](012-upstream-convergence.md) · [简体中文](../zh-CN/adr/012-upstream-convergence.md)


Date: 2026-10-08  
Status: Accepted

## Context
<!-- docs-section: context -->

The official Codex CLI is rapidly absorbing interaction features that previously justified
local convenience layers: richer agent views, multi-agent workflows, worktree workflows,
session/thread UX and other daily coding surfaces.

codex-tui already has a stronger and more durable differentiator: a local-first control
plane across repositories, Codex targets, Git state and GitLab/GitHub delivery state.

Continuing to compete with the official CLI on conversation, agent orchestration, queue,
terminal and coding-client UX would increase maintenance cost while creating duplicate
authorities.

## Decision
<!-- docs-section: decision -->

codex-tui is a **thin local control plane**.

The default product surface remains Mission Control. Its durable differentiation is:

- cross-repository and cross-target Codex registry;
- Needs You / Working / Ready attention routing;
- derived Board/WorkCard projections;
- Git repository/worktree identity and safety context;
- GitLab/GitHub Forge projection plus guarded mutations;
- Saved Views and lightweight operator metadata;
- Doctor, headless diagnostics and support-bundle evidence.

Authority remains external:

- Codex owns conversation, agent runtime/orchestration, thread lifecycle semantics and
  queue semantics;
- Git owns repository and worktree state;
- the configured Forge owns delivery, review and pipeline state;
- the user's shell/terminal stack owns general terminal-session management.

## Maintenance-only local surfaces
<!-- docs-section: maintenance -->

Existing local conversation-client, Thread Queue UI and embedded Terminal Drawer code is
retained for compatibility and current users, but is maintenance-only.

Those surfaces may receive defect, security and compatibility fixes. Feature expansion is
not the default. `release/v1.4-plan.json` remains the **single numeric LOC authority**
 via its complete-source-coverage module ratchet. `release/v1.5-convergence.json`
 records only the upstream-overlapping module **identities, categories and policy**.
 Growth must pass the frozen module ratchet; a new guarded source identity additionally
 requires an explicit reviewed convergence-manifest update.

Worktree convergence is now an explicit maintenance-only boundary. Official Codex
v0.162.0 introduced feature-gated managed worktree creation/listing. The existing
local worktree coordinator remains a Git-safety and recovery fallback, **not** a
second authority or a mandate to build competing Worktree UX. Its
`src/worktree*`, `src/operation.rs`, and `src/runtime_commands.rs` boundaries
are tracked as `managed-worktree-operations`. New duplicate creation/list
features stay frozen. Delegation to upstream is permitted only after target
capability detection, identity/safety parity and failure recovery have been
verified using a real Codex target; no fixture alone certifies parity. Git
remains authoritative for repository/worktree identity.

The machine guard is `scripts/architecture/check_upstream_convergence.py` and runs in
canonical CI, development qualification, and the release gate. Explicit guarded prefixes
include local conversation/editor modules, upstream Queue adapter/UI, the PTY engine and
embedded terminal UI; a prefix cannot silently disappear alongside its tracked modules.
The convergence guard reuses `check_module_ratchet.inspect_plan` instead of maintaining
 duplicate LOC ceilings. Negative-fixture tests reject untracked files, growth against
 the shared ratchet, prefix narrowing, authority theft and policy weakening. These are non-publishing architecture checks and
do not replace real Codex, GitLab or controlling-TTY qualification.

## Explicitly frozen duplicate capabilities
<!-- docs-section: freeze -->

codex-tui will not introduce a local agent orchestrator, agent delegation runtime,
agent-to-agent messaging layer, independent task/queue authority, independent
conversation protocol, general terminal manager or generic coding-agent provider layer.

When upstream Codex makes an existing compatibility layer redundant, prefer deleting,
reducing or projecting the upstream capability rather than extending the local layer.

## Consequences
<!-- docs-section: effects -->

This ADR intentionally favors a smaller product:

- Mission Control / Forge / Attention / diagnostics can continue evolving;
- upstream-overlapping interaction surfaces should shrink or stay flat;
- Board remains a projection, not a workflow authority;
- future Codex CLI improvements reduce codex-tui maintenance burden rather than create a
  race to reimplement them.

ADR-009 remains valid; this ADR operationalizes it with a machine-enforced convergence
contract.
