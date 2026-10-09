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

The runtime exposes a `ForgeProvider` contract. `GitLabProvider` is the M6a implementation and the actor accepts an injected provider, so M6c GitHub can reuse the same reducer/planning/actor boundary instead of cloning it.

The GitLab implementation uses `glab` / `glab api`. It does not add native REST/GraphQL transport and does not pin behavior to a compile-time GitLab version matrix.

Remote selection is deterministic:

1. current branch configured remote;
2. `origin`;
3. the only fetch remote;
4. otherwise fail closed.

HTTPS, SSH URL, and SCP-like Git remote forms are parsed. URI query and fragment fields, and ambiguous token-like suffixes on SCP-style remotes, fail closed before building a project REST path; HTTP userinfo is excluded from derived remote identity and diagnostics. Project identity is resolved through GitLab and then uses the numeric project id.

## Normal refresh budget

A normal user-visible forge refresh is bounded to four `glab api` subprocesses:

1. project identity;
2. recent Issues;
3. open Merge Requests;
4. recent Pipelines.

Subprocesses are shell-free, have a five-second timeout, use bounded captured output, and are isolated behind an async actor so GitLab latency never blocks the TUI event loop.

Probes are coalesced conservatively: threads sharing one checkout share the initial probe; once a numeric forge identity is known, observations with the same provider/host/project identity share later refreshes even across worktrees. A successful or failed observation has a 60-second refresh TTL, while the TUI reconciles freshness every 15 seconds without issuing extra API calls. This prevents per-thread `glab` amplification and retry storms.

This preserves ADR-011's decision to stay on `glab` unless a measured hard trigger justifies native transport.

## Fault-isolated normal refresh (v1.4 pre-release)

Numeric project identity remains a required read; a failed project lookup
fails closed. After identity, Issues, Merge Requests and Pipelines now use
three bounded concurrent glab reads. Each endpoint independently updates
its capability state. A failing endpoint contributes an empty projection
and an unavailable capability, **not** an empty-but-healthy result. Any
successful endpoint keeps the project observation fresh and usable.
If all three fail, the observation is unavailable with a bounded generic
reason, without copying raw CLI stderr into the shared view.

The initial project read plus three data reads remains four subprocesses
per logical refresh. The actor remains bounded and coalesces repository
observations. This does not claim any real internal GitLab server is healthy.

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

Raw authentication output and tokens are never printed. Provider errors now
classify common 401/403/404/network failures without forwarding any raw
subprocess stderr into operator UI or retained reports. For complete native
diagnostics, run the authenticated client directly in a trusted local shell.

## Freshness and degradation

Forge observations carry freshness/provenance separately from Codex and Git.

- a pending probe is explicit;
- Fresh/Aging/Stale is derived from observation age rather than frozen at fetch time;
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

Focused tests cover remote parsing/selection, fail-closed ambiguity, project-path encoding, exact branch matching, actor projection, Git-authoritative probe deduplication, checkout coalescing/TTL refresh, freshness aging, MR/Pipeline planning behavior, review-discussion attention, Issue WorkCard projection, and cross-thread Issue deduplication.
