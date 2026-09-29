# ADR-008: Significant mutations use plan, confirm, execute, verify and receipt

Date: 2026-09-29
Status: Accepted

## Decision

Significant Git and forge mutations follow:

Plan -> Confirm when user-impacting -> Execute -> Verify -> Receipt.

Examples include managed-worktree create/remove, branch-changing operations, forge work-item mutation, change-request creation/approval/merge.

## Rationale

Visual actions and keyboard shortcuts should not hide destructive or externally visible side effects. Receipts also improve recovery and diagnostics.

## Consequence

Read-only operations remain lightweight. Mutating providers expose explicit operation plans and results instead of arbitrary UI-triggered shell commands.