# Final architecture review — Go decision

Date: 2026-09-29
Status: Final review complete
Decision: GO for M0/M1 implementation

## Summary

The codex-tui architecture is sufficiently coherent and bounded to enter implementation.

No P0 architecture blockers remain after the final review.

The product is now consistently:

- personal-first;
- local-first;
- Codex-native;
- team-reuse oriented rather than team-server oriented;
- GitLab Self-Managed first behind a generic forge boundary;
- complementary to official Codex rather than duplicative;
- authority-aware across Codex, Git and the configured forge.

## P0 checks

### Product boundary — PASS

Core solves one developer's multi-project/multi-thread operator problem.

Team functionality is reuse + forge projection.

No mandatory account/RBAC/cloud/collaboration service.

### Authority model — PASS

- Codex owns execution/conversation.
- Git owns code/worktree.
- Forge owns shared work/review/delivery.
- Repo files own reusable engineering convention.
- codex-tui owns local operator state only.

### Official Codex overlap — PASS

The design explicitly complements Agent Center/Agents Overview and prefers upstream Thread/Project/Goal/Section/Queue semantics.

### Workflow model — PASS

Workflow stage and Attention are orthogonal.

Workflow:
Inbox -> Ready -> Working -> Review -> Done

Needs You is an attention overlay/filter/virtual lane.

### WorkCard identity — PASS after review fix

WorkCard has stable local_id, one anchor and links.

Anchor changes are explicit, not discovered automatically.

External source refs are unique across active cards and duplicate merge is explicit.

### Multi-source state — PASS

Projection keeps provenance, reason and freshness.

Stale/unavailable forge state is visible rather than presented as current.

### Git/worktree safety — PASS after review fix

Collision detection uses MutationScope rather than cwd alone, with conservative fallback when precise writable roots are unavailable.

### Forge architecture — PASS

Core uses generic ForgeProvider concepts.

GitLab Self-Managed is first provider; GitHub follows the same contract.

Initial GitLab transport through glab reuses authentication and keeps credentials out of codex-tui.

Forge absence does not break personal core.

### Mutation safety — PASS after review fix

Significant Git/forge mutations use:

Plan -> Confirm -> Execute -> Verify -> Receipt

Unknown outcomes use reconcile-before-retry rather than blind retries.

### Persistence — PASS

LocalStore abstraction exists before choosing long-term storage.

Initial file-backed storage is sufficient; SQLite is introduced only when justified.

No canonical transcript duplication.

### Terminal/runtime — PASS

Dedicated terminal capability boundary, bounded probes and no required codex-tui-owned daemon.

### Maintainability — PASS

Compatibility promises are bounded. Experimental upstream capabilities are optional. Plugin/multi-agent/remote collaboration remain later optional layers.

## P1 implementation decisions — not blockers

These should be resolved through measurement or focused ADRs during implementation:

- exact Rust crate choices for syntax highlighting/diff/error handling;
- final keymap defaults;
- exact file format for the first LocalStore backend;
- exact startup/frame performance budgets after baseline measurement;
- when Saved Views/Scratch scale justifies SQLite;
- when glab subprocess overhead justifies a native GitLab transport;
- exact GitLab version/tier compatibility matrix based on the internal instance;
- exact notification backend;
- whether terminal drawer belongs before or after GitLab integration.

## P2 future decisions — intentionally deferred

- web/mobile;
- shared presence/collaboration;
- plugin runtime;
- multi-agent support;
- jobs/workflows;
- organization analytics;
- cost platform;
- remote control service.

These must not influence M0/M1 core structure beyond preserving clean boundaries.

## Implementation guardrails

M0/M1 PRs should be rejected if they:

- introduce canonical transcript storage;
- put GitHub/GitLab-specific types into core UI/domain;
- make experimental App Server APIs required for startup;
- block UI rendering on Git/glab/network/subprocess work;
- make background refresh change manual selection;
- persist derived external state as authority;
- hide significant mutations behind direct key actions without OperationPlan/Receipt;
- add a codex-tui-owned mandatory daemon;
- add generic multi-agent/plugin infrastructure prematurely;
- duplicate official Codex Agent Center behavior instead of using upstream semantics.

## Implementation sequence

Proceed:

1. M0 architecture skeleton.
2. M1 read-only real Codex registry.
3. M2 daily conversation control.
4. M3 Git/review.
5. M4 Planning/WorkCard.
6. M5 safe managed worktrees.
7. M6 GitLab Self-Managed.
8. M7 scale/polish.
9. optional layers only after core evidence.

## Final decision

GO.

Further architecture brainstorming should no longer block implementation. New architectural work should be triggered by concrete implementation evidence, upstream Codex/GitLab changes, or observed user workflow gaps.
