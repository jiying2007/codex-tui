# M6c — GitHub read-only ForgeProvider

Date: 2026-09-30  
Target: v0.6.x

## Goal

Complete the M6 forge abstraction by adding a GitHub read-only provider without duplicating the core planning/review UI or weakening the existing GitLab Self-Managed path.

GitHub remains canonical for Issues, Pull Requests, Actions runs, reviews and review threads. codex-tui projects only the bounded state needed by a single developer and keeps the same normalized `ForgeObservation`, `WorkCard`, freshness and attention model used by GitLab.

## Provider routing

`ForgeHandle::start()` now runs a `RoutingForgeProvider`.

The routing rule for M6c is deliberately narrow:

- exact `github.com` -> `GitHubProvider`;
- every other host -> existing `GitLabProvider`.

The remote is resolved once through the existing deterministic Git remote authority and then passed to the selected provider. Routing therefore does not add a forge subprocess/API probe.

GitHub Enterprise Server is not auto-detected in M6c. The GitHub CLI supports explicit custom hostnames, but probing every unknown host to discover whether it is GitLab or GHES would add latency and subprocesses to the internal GitLab baseline. A future explicit provider override or retained GHES requirement can add that capability without changing the normalized provider contract.

## GitHub transport

The provider uses the authenticated `gh` CLI:

- `gh api --hostname <host> <REST endpoint>`;
- `gh api --hostname <host> graphql ...` for review threads;
- shell-free argv execution;
- the same five-second subprocess timeout and bounded stdout/stderr capture as the existing forge layer.

Authentication remains owned by `gh`; codex-tui never reads or stores GitHub tokens.

## Normal refresh budget

A normal GitHub user-visible forge refresh uses exactly four `gh api` subprocesses:

1. repository identity;
2. recent Issues;
3. open Pull Requests;
4. recent GitHub Actions workflow runs.

The repository identity call is required. Issues, Pull Requests and Actions are capability-isolated: if one endpoint is unavailable, the others still project normally and the failed capability becomes `Unavailable`.

The Issues REST endpoint also returns Pull Requests; items carrying the GitHub `pull_request` marker are filtered out before Issue WorkCards are created.

## Normalized identity and relationships

The provider maps GitHub into the same normalized model:

- repository numeric ID -> `ForgeIdentity.project_id`;
- `owner/repo` -> `path_with_namespace`;
- repository URL -> `web_url`;
- default branch -> `default_branch`;
- Pull Request number -> change-request IID;
- Actions workflow-run ID -> normalized pipeline ID;
- Actions head branch -> pipeline reference.

Provider-specific external relationship refs are created only behind `ForgeIdentity`:

- GitLab compatibility refs remain unchanged:
  - `gitlab://<host>/projects/<id>/issues/<iid>`
  - `gitlab://<host>/projects/<id>/merge-requests/<iid>`
- GitHub refs:
  - `github://<host>/repositories/<id>/issues/<number>`
  - `github://<host>/repositories/<id>/pull-requests/<number>`

This preserves existing GitLab-local overlays while keeping Planning provider-neutral.

## Actions normalization

GitHub Actions workflow runs are projected through the existing `PipelineSummary`.

Important status normalization:

- conclusion `failure` -> `failed`;
- `cancelled` / `canceled` -> `canceled`;
- other terminal conclusions keep their conclusion string;
- non-terminal runs use their workflow status.

This intentionally reuses the existing Planning rule that `failed` adds `pipeline-failed` attention.

## Review projection

Review detail remains on-demand when Review is opened.

M6c performs two bounded calls:

1. REST Pull Request reviews, up to 100;
2. GraphQL `reviewThreads(first:100)`.

### Reviewer state

GitHub can contain multiple historical reviews from one reviewer. codex-tui keeps only the latest review ID per user for normalized state.

It exposes:

- latest-user `APPROVED` count;
- latest-user `CHANGES_REQUESTED` count.

A nonzero changes-requested count maps to the existing `change-requested` planning attention.

GitHub does not provide the same baseline `approvals_required / approvals_left` semantics as GitLab, so those normalized fields remain unknown rather than being invented.

### Review threads

GraphQL review threads expose `isResolved`. Unresolved threads map to the same normalized discussion/change-requested attention used by GitLab.

M6c deliberately does not paginate beyond 100 review threads. If `hasNextPage=true`, discussion capability becomes unavailable with an explicit bounded-projection diagnostic rather than presenting a partial unresolved count as authoritative.

REST reviews and GraphQL review threads degrade independently.

## Doctor

`doctor forge` is provider-neutral.

It now reports:

- forge client name (`glab` or `gh`);
- client version;
- authentication status for the selected host;
- server version where the provider can retrieve one;
- selected Git remote;
- provider/host/repository identity;
- runtime capability states;
- normalized Issue/change-request/pipeline counts.

GitLab Issue Boards continue to be probed only for GitLab. GitHub Projects v2 is not treated as an Issue Board substitute in M6c.

## Read-only boundary

M6c introduces no GitHub mutation path.

In particular, it does not:

- create/update Issues;
- create/comment/review/merge Pull Requests;
- mutate Actions;
- mutate Projects;
- add a GitHub mutation coordinator;
- persist GitHub canonical state into SQLite.

The M6b GitLab mutation path remains explicitly provider-gated to GitLab.

## GHES policy

GitHub Enterprise Server support is transport-capable through `gh --hostname`, but automatic provider discovery for non-`github.com` hosts is intentionally deferred.

A future GHES extension should use one of:

- explicit repository/provider configuration;
- a retained enterprise-host fixture/config mapping;
- another zero-extra-probe signal.

It should not make every internal GitLab refresh pay a provider-detection subprocess.

## Verification

Canonical CI remains:

- `cargo fmt --all -- --check`;
- `cargo clippy --all-targets --all-features -- -D warnings`;
- `cargo test --all-targets --all-features`;

on Linux, macOS and Windows.

Focused coverage includes:

- exact GitHub.com provider routing and GitLab-first fallback for other hosts;
- provider-specific external refs;
- exact `owner/repo` parsing;
- Issue-vs-PR filtering;
- latest-review-per-user normalization;
- `CHANGES_REQUESTED` planning attention;
- Actions failure normalization;
- bounded review-thread pagination;
- GitHub capability isolation;
- retained GitLab URI compatibility.

## Result

M6 now has one normalized forge projection architecture with:

- GitLab Self-Managed read-only projection;
- safe explicit GitLab MR mutations;
- GitHub.com read-only projection.

M7 can therefore focus on scale/polish instead of carrying unfinished forge abstraction work.
