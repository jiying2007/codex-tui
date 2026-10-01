# Provider qualification

Provider qualification is observation-driven. codex-tui does not infer capabilities from a GitLab/GitHub version number.

The retained authority is a secret-safe capability fixture captured from the exact candidate binary while it is running inside a representative repository.

## Preconditions

Use a clean checkout of the candidate SHA and build it with the SHA embedded:

```bash
SHA="$(git rev-parse HEAD)"
CODEX_TUI_GIT_SHA="$SHA" cargo build --release --locked
```

The repository must have the forge remote that should be qualified. Authentication is owned by the native client:

- GitLab / GitLab Self-Managed: `glab`
- GitHub.com: `gh`

No token is copied into codex-tui state or into the retained fixture.

## Internal GitLab qualification

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
