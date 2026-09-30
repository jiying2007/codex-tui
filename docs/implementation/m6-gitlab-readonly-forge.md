# M6a — GitLab Self-Managed read-only forge projection

Date: 2026-09-30
Target: v0.6.x

## Goal

Add useful GitLab context without turning codex-tui into a second forge authority.

GitLab remains authoritative for Issues, Merge Requests, Pipelines, approvals, discussions, and Issue Boards. codex-tui stores only personal operator state; forge observations are rebuilt from GitLab and carry provenance/freshness.

## Provider boundary

M6a introduces a forge boundary with:

- provider identity;
- host;
- numeric project id;
- namespace path;
- runtime capability states;
- observation timestamp/freshness;
- degraded/unavailable reason.

The GitLab implementation uses `glab` / `glab api`. It does not add native REST/GraphQL transport and does not pin behavior to a compile-time GitLab version matrix.

Remote selection is deterministic:

1. current branch configured remote;
2. `origin`;
3. the only fetch remote;
4. otherwise fail closed.

HTTPS, SSH URL, and SCP-like Git remote forms are parsed. Project identity is resolved through GitLab and then uses the numeric project id.

## Normal refresh budget

A normal user-visible forge refresh is bounded to four `glab api` subprocesses:

1. project identity;
2. recent Issues;
3. open Merge Requests;
4. recent Pipelines.

Subprocesses are shell-free, have a five-second timeout, use bounded captured output, and are isolated behind an async actor so GitLab latency never blocks the TUI event loop.

This preserves ADR-011's decision to stay on `glab` unless a measured hard trigger justifies native transport.

## Planning projection

### Issues

The normal query keeps the most recently updated Issues across open and closed state. They are projected into personal WorkCards using stable identity:

`gitlab://<host>/projects/<numeric-project-id>/issues/<iid>`

The same Issue observed by multiple Codex threads is deduplicated by that identity.

- open Issue -> Inbox by default;
- locally selected ready -> Ready;
- closed Issue -> Done;
- explicit local completion acknowledgement -> Done.

Local title/note/pin/priority/snooze overlays may decorate the card, but GitLab state is not copied into SQLite as canonical data.

### Merge Requests and Pipelines

The current Git branch is matched exactly against Merge Request source branches and Pipelines.

An open MR can move an otherwise Inbox/Ready thread card to Review. A failed current-branch Pipeline adds `pipeline-failed` attention.

MR identity is attached as a relationship, not persisted as a duplicated canonical object.

### Review metadata

Approvals and discussions are deliberately not in the normal refresh budget. When the user enters Review for a branch with a matching MR, M6a performs two targeted read-only calls:

- approvals;
- discussions.

Unresolved resolvable discussions add `change-requested` attention. The Review header shows approval/discussion summary when available. Unsupported endpoints degrade per capability rather than failing the whole forge projection.

### Issue Boards

Issue Boards are capability-probed by `doctor forge`. codex-tui does not mirror GitLab board column state into a second local board database; the personal Board remains a derived WorkCard view.

## Doctor

`codex-tui doctor forge` reports secret-safe diagnostics:

- glab version;
- authentication success for the resolved host;
- GitLab server version when available;
- selected Git remote;
- host and namespace path;
- numeric project id;
- provider/project URL;
- runtime capability states;
- Issue/MR/Pipeline/Issue Board counts;
- degraded error when present.

Raw authentication output and tokens are never printed.

## Freshness and degradation

Forge observations carry freshness/provenance separately from Codex and Git.

- a pending probe is explicit;
- missing `glab`, auth failure, unsupported remote, timeout, or API failure becomes an unavailable/degraded observation;
- Codex and Git remain usable when Forge is unavailable;
- capability-specific probes can fail without declaring unrelated capabilities unavailable.

## Explicit non-goals

M6a does not:

- mutate GitLab;
- create/update Issues or Work Items;
- create/comment/approve/merge MRs;
- persist forge canonical data in SQLite;
- introduce native GitLab REST/GraphQL transport;
- introduce Terminal Drawer;
- implement the GitHub provider.

Those remain M6b, M6c, or M7 work.

## Verification

Required CI remains the repository's three-platform matrix:

- `cargo fmt --all -- --check`;
- `cargo clippy --all-targets --all-features -- -D warnings`;
- `cargo test --all-targets --all-features`;

on Linux, macOS, and Windows.

Focused tests cover remote parsing/selection, fail-closed ambiguity, project-path encoding, exact branch matching, actor projection, Git-authoritative probe deduplication, MR/Pipeline planning behavior, review-discussion attention, Issue WorkCard projection, and cross-thread Issue deduplication.
