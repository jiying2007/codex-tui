# v1.3 Workflow Completion & UX

Date: 2026-10-02
Status: active development plan
Base: v1.2 development-complete checkpoint `cfed175ed1f3488c3a987f01d8c6b07292bf2553`
Integration branch: `release/v1.3-development`

v1.3 is not a new breadth release. It completes the highest-value workflow gaps left after the v1.2 hardening line while preserving the existing authority model.

## P0

1. Large Board usability
   - selection-follow viewport;
   - overflow scrollbar/affordance;
   - wide/narrow/List/Review Queue behavior;
   - large-fixture regression coverage.

2. WorkCard relationship closure
   - project Goal and Worktree links;
   - explicit local relationship editing;
   - consolidate duplicate local projections without changing Codex/Git/Forge authority.

3. Codex thread lifecycle
   - create/fork through App Server capability detection when the connected backend supports it;
   - explicit worktree handoff;
   - no local fake thread authority.

4. Unified metadata search
   - Thread + Goal + Git/worktree + Forge + WorkCard metadata;
   - Scratch and forge-only items included in planning search;
   - no transcript hydration in the search hot path.

## P1

- Saved View editor for source/filter/layout/group/order/visible fields.
- Review evidence workspace plus a real external-open action.
- Continue splitting large core modules instead of raising v1.2 ceilings.
- Long-thread viewport/materialization hardening.

## P2

- GitLab Issue Board projection when the provider exposes it.
- Safe GitHub write parity behind the normalized Forge contract.
- Fuzzy command palette filtering.
- User-perceived performance evidence for render/first-paint/search/forge process overhead.

## Explicit deferrals

These remain outside the v1.3 completion contract unless new evidence changes the decision:

- Thread Queue until upstream capability is stable enough;
- transcript FTS until real usage proves metadata search insufficient;
- native GitLab REST/GraphQL until the existing glab hard trigger fires;
- remote App Server targets;
- team server/RBAC;
- web/mobile;
- generic plugin runtime;
- multi-agent abstraction;
- workflow/job engine;
- cloud sync.

## Exit

v1.3 development is complete only when:

- all P0 and P1 outcomes are implemented and covered;
- P2 items are implemented where provider/runtime evidence makes them valid, otherwise explicitly evidence-gated without a fake fallback;
- Linux/macOS/Windows canonical CI and Rust 1.88 remain green;
- the integration branch has no open P0 defect;
- v1.2 stable evidence remains independently reproducible from its frozen checkpoint.
