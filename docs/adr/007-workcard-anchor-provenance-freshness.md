# ADR-007: WorkCard has one anchor and explicit provenance/freshness

Date: 2026-09-29
Status: Accepted

## Decision

Each WorkCard has one stable local_id and exactly one primary anchor: ScratchWork, ForgeWorkItem or CodexThread.

Goal, worktree and ChangeRequest are normally links.

Derived stage and attention keep source/reason/freshness metadata.

## Rationale

A single piece of work can evolve from scratch note -> forge issue -> Codex thread -> worktree -> MR/PR. Without one anchor/relationship identity, these become duplicate cards.

Codex, Git and forge observations also refresh at different times, so the UI must distinguish fresh, stale and unavailable evidence.

## Consequence

Titles and paths are presentation metadata, not stable identity. GitLab stable project ID is preferred over path where available. The UI can explain why a card has its current stage or attention state.