# M6b — Safe explicit GitLab merge-request mutations

Date: 2026-09-30
Target: v0.6.x

## Goal

Add a small set of high-value GitLab writes without weakening the local-first authority model established in M5/M6a.

GitLab remains authoritative. codex-tui stores only local mutation plans/receipts needed for explicit confirmation, recovery, audit, and uncertainty handling.

M6b deliberately excludes experimental Work Items mutation.

## Supported mutations

The v0.6.x M6b surface is limited to:

- create a merge request from the exact current branch to the current GitLab default branch;
- comment on the exact open merge request associated with the current branch;
- approve that exact merge request as the authenticated GitLab user;
- merge that exact merge request.

There is no force merge, policy bypass, automatic approval, automatic merge, or silent retry.

## User flow

Forge mutations are available only from Review / Workspace context actions.

Every mutation follows:

1. resolve current Git + Forge identity;
2. create an immutable local `ForgeMutationPlan`;
3. show the exact plan in a modal;
4. require explicit `y` confirmation;
5. re-read GitLab and revalidate preconditions;
6. persist Executing receipt before the external write;
7. perform exactly one GitLab mutation;
8. re-read GitLab to verify the resulting state;
9. persist Succeeded / Failed / OutcomeUnknown receipt;
10. invalidate and refresh the read-only Forge projection.

`c` or `Esc` cancels the plan before execution.

## Independent coordinator

Forge writes use `ForgeMutationHandle`, separate from the M5 Git/worktree mutation coordinator.

The Forge coordinator owns:

- per-project in-process serialization keyed by provider + host + numeric project id;
- execution-time validation;
- bounded `glab api` mutation transport;
- verification;
- durable receipt transitions;
- restart reconciliation.

Git index/worktree serialization and GitLab project serialization remain separate authority domains.

## Mutation plans

A plan records only the information required to identify and verify the operation:

- operation id;
- mutation kind;
- provider/host/numeric project id/project path;
- cwd;
- exact MR IID when applicable;
- source/target branch;
- MR title for create;
- comment byte length for comment;
- expected side effect;
- explicit preconditions;
- planned timestamp.

The plan never contains authentication credentials.

### Comment privacy

Comment text is carried only by the in-memory `ForgeMutationRequest`.

SQLite stores only `payload_bytes`; the body is intentionally absent from `plan_json` and receipts. If the process dies after a comment request begins, M6b cannot safely prove the outcome after restart and therefore keeps the receipt `OutcomeUnknown` instead of retrying.

## Execution-time preconditions

All operations revalidate:

- GitLab provider;
- numeric project identity;
- project path identity.

Create MR additionally requires:

- GitLab default branch still equals the planned target;
- source branch exists;
- target branch exists;
- no matching open source→target MR already exists.

Comment additionally requires:

- exact MR is still open;
- source and target branches are unchanged.

Approve and Merge additionally require:

- exact MR is still open;
- source/target unchanged;
- current MR HEAD SHA is present and re-read immediately before mutation.

Approve sends that SHA to GitLab's approve endpoint.

Merge sends that SHA to GitLab's merge endpoint. This both prevents stale-head mutations and supports GitLab deployments that require SHA on the merge API.

Merge also fails closed before execution when observable evidence shows:

- draft MR;
- unresolved blocking discussions;
- conflict status;
- failed/canceled head pipeline;
- unsatisfied required approval rules when `/approval_state` is available;
- remaining approvals in the legacy approvals response when rule-level state is unavailable.

GitLab's merge endpoint remains the final server-side policy authority; codex-tui never requests a bypass.

## API transport

M6b keeps ADR-011's `glab` transport choice.

Writes use bounded, shell-free `glab api --method ... -f key=value` subprocesses through the same timeout/output-capped transport used by M6a.

Endpoints:

- `POST /projects/:id/merge_requests`
- `POST /projects/:id/merge_requests/:iid/notes`
- `POST /projects/:id/merge_requests/:iid/approve`
- `PUT /projects/:id/merge_requests/:iid/merge`

Readbacks use project, branch, MR, note, approvals, approval-state and current-user endpoints as needed.

## Receipt lifecycle

`ForgeMutationReceipt` reuses the M5 operation states:

- Planned
- Executing
- Succeeded
- Failed
- OutcomeUnknown

SQLite schema v3 introduces `forge_mutation_receipts` for local audit/recovery metadata only.

A restart handles states conservatively:

- Planned -> Failed because no confirmed execution began;
- Executing -> OutcomeUnknown, then read-only reconciliation;
- OutcomeUnknown -> read-only reconciliation;
- terminal receipts remain terminal.

After an external request may have started, absence of immediate evidence is never converted into a retry-safe Failed result.

Examples:

- uncertain create and no exact MR visible -> OutcomeUnknown;
- uncertain comment -> OutcomeUnknown because the body was not persisted;
- uncertain approve and user not yet visible in `approved_by` -> OutcomeUnknown;
- uncertain merge and MR still open -> OutcomeUnknown.

Only positive readback evidence promotes an uncertain receipt to Succeeded.

## SQLite v3

The v2 -> v3 migration adds only the forge receipt table/index.

Tests verify:

- v1 upgrades through v2 to latest v3;
- v2 -> v3 preserves M5 receipts;
- Forge receipts round-trip;
- only non-terminal Forge receipts are recoverable;
- comment bodies are absent from stored plan JSON.

No GitLab Issue/MR/Pipeline/approval canonical data is copied into SQLite.

## Diagnostics

`codex-tui doctor store` reports recent Forge mutation receipt metadata:

- operation id;
- mutation kind;
- state;
- host/project path;
- MR IID when applicable.

It does not print comment text, tokens, auth output, or other credentials.

## Verification

Repository CI remains:

- `cargo fmt --all -- --check`;
- `cargo clippy --all-targets --all-features -- -D warnings`;
- `cargo test --all-targets --all-features`;

on Linux, macOS, and Windows.

Focused tests cover:

- exact/no-force plans;
- memory-only comment bodies;
- invalid project identity fail-closed;
- explicit-confirm reducer behavior;
- create/comment/approve/merge context action gating;
- projection invalidation after receipts;
- SQLite v3 migration/round-trip/recovery;
- GitLab MR HEAD SHA parsing;
- approval-state rule interpretation;
- required SHA revalidation in approve/merge plans.

A real authenticated GitLab Self-Managed run is environment evidence, not something CI or fake fixtures may impersonate.

## Explicit non-goals

M6b does not add:

- Work Items GraphQL mutation;
- Issue mutation;
- force merge;
- bypass of approval/pipeline/discussion/project policy;
- background automatic mutation;
- blind retry after uncertain writes;
- native GitLab REST/GraphQL transport;
- GitHub mutations.

GitHub remains M6c.
