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

## Retry and unknown-outcome rule

Every significant mutation receives a local operation_id.

Lifecycle includes Planned, Executing, Succeeded, Failed and OutcomeUnknown.

A timeout or transport failure after execution begins can produce OutcomeUnknown. Such an operation is never blindly retried. codex-tui first reconciles the target system to determine whether the operation already succeeded.

Where an upstream idempotency mechanism exists, use it. Otherwise use reconcile-before-retry semantics.
