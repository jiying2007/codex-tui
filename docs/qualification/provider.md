<!-- docs-id: provider -->
<!-- docs-lang: en -->
# Provider qualification
<!-- docs-section: overview -->

**Language / 语言:** [English](provider.md) · [简体中文](../zh-CN/qualification/provider.md)


Provider qualification is observation-driven. codex-tui does not infer capabilities from a GitLab/GitHub version number.

The retained authority is a secret-safe capability fixture captured from the exact candidate binary while it is running inside a representative repository.

## Preconditions
<!-- docs-section: requirements -->

In the **codex-tui source checkout** (not the repository to be probed),
capture its exact source commit and build the binary with that identity:

```bash
cd /path/to/codex-tui
SOURCE_DIR="$(pwd)"
SOURCE_SHA="$(git rev-parse HEAD)"
CODEX_TUI_GIT_SHA="$SOURCE_SHA" cargo build --release --locked
BINARY="$SOURCE_DIR/target/release/codex-tui"
```

Keep these shell variables while changing to the **representative Forge
repository** for each probe. Its `git rev-parse HEAD` is the *business
repository* commit, **not** the codex-tui binary's source SHA.

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

Run from the **actual internal GitLab repository**; the script and binary
remain absolute paths to the codex-tui source checkout:

```bash
cd /path/to/internal-gitlab-repository
python3 "$SOURCE_DIR/scripts/release/capture_forge_capability.py" \
  --binary "$BINARY" \
  --output "$SOURCE_DIR/release/evidence/provider/gitlab.json" \
  --expected-provider gitlab \
  --expected-source-sha "$SOURCE_SHA" \
  --require-authenticated \
  --required-capability issues=available \
  --required-capability merge-requests=available \
  --required-capability pipelines=available
```

Add a capability requirement only when the team actually depends on it. For example, do not make Issue Boards or review discussions release-blocking merely because a particular GitLab edition exposes them.

The fixture records the observed client/server version and edition when discoverable, but those fields are descriptive evidence only. Capability state is authoritative.

## GitHub read-only qualification
<!-- docs-section: github -->

Run from a representative GitHub.com repository using that **same**
codex-tui source build and preserved `SOURCE_SHA`:

```bash
cd /path/to/github-repository
python3 "$SOURCE_DIR/scripts/release/capture_forge_capability.py" \
  --binary "$BINARY" \
  --output "$SOURCE_DIR/release/evidence/provider/github.json" \
  --expected-provider github \
  --expected-source-sha "$SOURCE_SHA" \
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

After a real GitLab capture, validate the retained fixture using the
**original codex-tui source SHA**, never the internal repository's HEAD:

```bash
python3 "$SOURCE_DIR/scripts/release/validate_internal_gitlab.py" \
  --fixture "$SOURCE_DIR/release/evidence/provider/gitlab.json" \
  --source-sha "$SOURCE_SHA" \
  --output "$SOURCE_DIR/release/evidence/provider/internal-gitlab-admission.json"
```

This admission requires fresh (at most 7-day-old) Linux / glab evidence,
authenticated GitLab access, and explicitly requested **and observed**
available Issues, Merge Requests and Pipelines. Its receipt hashes the exact
input fixture, reports only reason codes and cannot overwrite prior evidence.
Unit tests use synthetic fixtures to test rejections; they never establish
real provider PASS. The validation also does not substitute for real Codex,
controlling SSH TTY, administrator protection or publishing authorization.
