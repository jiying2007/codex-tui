<!-- docs-id: provider -->
<!-- docs-lang: en -->
# Provider qualification
<!-- docs-section: overview -->

**Language / 语言:** [English](provider.md) · [简体中文](../zh-CN/qualification/provider.md)


Provider qualification is observation-driven. codex-tui does not infer capabilities from a GitLab/GitHub version number.

The retained authority is a secret-safe capability fixture captured from the exact candidate binary while it is running inside a representative repository.

## Preconditions
<!-- docs-section: requirements -->

Use a clean checkout of the candidate SHA and build it with the SHA embedded:

```bash
SHA="$(git rev-parse HEAD)"
CODEX_TUI_GIT_SHA="$SHA" cargo build --release --locked
```

The repository must have the forge remote that should be qualified. Authentication is owned by the native client:

- GitLab / GitLab Self-Managed: `glab`
- GitHub.com: `gh`

No token is copied into codex-tui state or into the retained fixture.
The capture wrapper limits doctor bundle to 45 seconds and never echoes raw
subprocess stdout/stderr on failure; only a bounded exit/timeout reason is
displayed, so token-bearing CLI diagnostics cannot escape to CI logs through
this capture path.

## Internal GitLab qualification
<!-- docs-section: gitlab -->

Run inside a representative internal GitLab repository:

```bash
python3 scripts/release/capture_forge_capability.py \
  --binary target/release/codex-tui \
  --output release/evidence/provider/gitlab.json \
  --expected-provider gitlab \
  --expected-source-sha "$(git rev-parse HEAD)" \
  --require-authenticated \
  --required-capability issues=available \
  --required-capability merge-requests=available \
  --required-capability pipelines=available
```

Add a capability requirement only when the team actually depends on it. For example, do not make Issue Boards or review discussions release-blocking merely because a particular GitLab edition exposes them.

The fixture records the observed client/server version and edition when discoverable, but those fields are descriptive evidence only. Capability state is authoritative.

## GitHub read-only qualification
<!-- docs-section: github -->

Run inside a representative GitHub.com repository:

```bash
python3 scripts/release/capture_forge_capability.py \
  --binary target/release/codex-tui \
  --output release/evidence/provider/github.json \
  --expected-provider github \
  --expected-source-sha "$(git rev-parse HEAD)" \
  --require-authenticated \
  --required-capability issues=available \
  --required-capability merge-requests=available \
  --required-capability pipelines=available
```

This qualifies the read-only GitHub provider. It does not introduce GitHub write mutations.

## Fixture contract
<!-- docs-section: fixture -->

The output schema is `codex-tui/forge-capability-fixture/v1`. It contains:

- exact product/source build identity;
- provider and native client version;
- authenticated = true/false/unknown;
- server version/edition/tier when discoverable;
- observed capability states;
- bounded object counts;
- the explicit requirements used for this qualification;
- qualification blockers, if any.

It deliberately excludes authentication tokens, repository paths, remote URLs, prompts/transcripts, comment bodies and raw error text.

A nonzero exit code means the fixture was still written, but one or more requested requirements did not match the observed environment.

## Internal first-deployment admission (provider-specific)
<!-- docs-section: internal -->

Public Stable releases remain provider-neutral. Internal GitLab adoption
additionally requires a real, authenticated capability fixture from a
representative internal repository using the exact candidate binary.

After running the GitLab capture command above, validate the retained fixture:

```bash
SHA="$(git rev-parse HEAD)"
python3 scripts/release/validate_internal_gitlab.py \
  --fixture release/evidence/provider/gitlab.json \
  --source-sha "$SHA" \
  --output release/evidence/provider/internal-gitlab-admission.json
```

This admission requires fresh (at most 7-day-old) Linux / glab evidence,
authenticated GitLab access, and explicitly requested **and observed**
available Issues, Merge Requests and Pipelines. Its receipt hashes the exact
input fixture, reports only reason codes and cannot overwrite prior evidence.
Unit tests use synthetic fixtures to test rejections; they never establish
real provider PASS. The validation also does not substitute for real Codex,
controlling SSH TTY, administrator protection or publishing authorization.
