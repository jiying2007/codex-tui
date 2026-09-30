# M7d3: stable/preview release and distributable verification

Status: implementation slice under #38 / #41

M7d3 is the final M7 hardening slice. It creates a bounded release pipeline; it does not create a generalized workflow system or background release service.

## Release channels

### Preview

Tag contract:

```text
vX.Y.Z-preview.N
```

where `X.Y.Z` exactly matches `Cargo.toml` and `N >= 1`.

Preview runs all machine packaging gates. Stable-only retained human evidence is optional.

### Stable

Tag contract:

```text
vX.Y.Z
```

The package major version must be at least 1. The first permitted stable line is v1.0.0.

Stable validation requires retained evidence before publication.

## Publication model

Release publication is `workflow_dispatch` only.

The same workflow also has a narrow non-publishing self-test trigger: a push to `main` runs `preview.1` with `publish=false` only when the release workflow, release scripts, release verifier, release criteria, CHANGELOG, release docs or Cargo.lock changed. Ordinary product commits do not run the three-platform packaging self-test.

Manual release dispatch must use `main`. The workflow computes the tag from the checked-out Cargo version and selected channel. It does not accept an arbitrary pre-existing tag as authority.

`publish=false` performs validation, three-platform build/package/smoke and emits a retained release-bundle artifact without creating a Git tag or GitHub Release.

`publish=true` additionally creates the GitHub Release only after every prior job succeeds.

An existing tag is treated as a collision and publishing fails closed.

## Project license

The release verifier intentionally does not select a project license.

Any `publish=true` run is blocked until the repository contains one of:

- `LICENSE`
- `LICENSE.txt`
- `LICENSE.md`

This currently prevents accidental publication while still allowing retained preview build validation.

## Stable evidence

Stable evidence is represented by `codex-tui/release-evidence/v1`.

It binds:

- release version;
- exact 40-character source SHA;
- canonical successful CI run ID;
- compatibility schema version;
- Linux/macOS/Windows compatibility report SHA-256 values with READY state and observation timestamps;
- Linux/macOS/Windows terminal-restoration PASS receipts;
- retained `resident-planning-10k` p95/p99 performance receipt.

The stable verifier requires:

- at least 200 retained resident-planning-10k samples;
- p95 <= 50 ms;
- p99 <= 100 ms;
- compatibility report hashes are exactly 64 hexadecimal characters.

The workflow independently calls the GitHub Actions API and verifies the supplied canonical CI run is the `ci` workflow on `main`, succeeded, and is bound to the release source SHA.

## Locked dependency graph

`Cargo.lock` is committed for release candidates.

Canonical CI uses locked Clippy/tests, and the release build uses:

```text
cargo build --release --locked
```

Release verification fails when Cargo.lock is absent.

## Third-party notices

`scripts/release/generate_notices.py` reads locked Cargo metadata and walks the normal runtime dependency closure from the codex-tui root package.

It records:

- package/version;
- declared license expression;
- repository/homepage metadata;
- license/notice files present in package source.

A runtime dependency with neither a license expression nor a discoverable license/notice file fails notice generation.

Dev-only and build-only packages are not represented as shipped runtime dependencies.

## Deterministic archive construction

Each platform builds natively and names the package with the Rust host triple:

```text
codex-tui-X.Y.Z-<host-triple>.tar.gz
codex-tui-X.Y.Z-<host-triple>.zip
```

Linux/macOS use deterministic tar+gzip metadata:

- mtime 0;
- uid/gid 0;
- empty uname/gname;
- sorted paths;
- gzip mtime 0.

Windows zip entries use a fixed timestamp and deterministic ordering.

Each archive contains the native binary, README, CHANGELOG, install/upgrade guidance, third-party notices, v1 stable criteria and release metadata. The project license is included when declared.

## Archive smoke

Every platform extracts its newly built archive and executes the packaged binary:

1. `codex-tui --version` must exactly match the Cargo version;
2. `codex-tui headless threads --fixture-10k --json` must report the v1 headless schema, no degradation and exactly 10,000 rows;
3. required documentation/notice/criteria/metadata files must exist;
4. release metadata version/tag/SHA must match the gate outputs.

This validates the distributable, not merely `target/release`.

## Release bundle

The aggregate job downloads all three verified native archives and gate metadata, generates version release notes from CHANGELOG and writes `SHA256SUMS`.

The retained workflow artifact is the release bundle even when `publish=false`.

For stable channel, the evidence receipt is included in the bundle.

## Retained evidence capture

Canonical performance capture:

```bash
cargo run --release --locked -- release benchmark \
  --warmup 20 \
  --iterations 200 \
  --source <machine-or-retained-runner-id> \
  --json
```

Canonical compatibility capture on each supported platform:

```bash
python scripts/release/capture_compat.py \
  --binary <path-to-v1-candidate-binary> \
  --output compat-<platform>.json
```

The compatibility helper refuses anything other than `readiness=ready` and prints the report SHA-256 and observation timestamp used by stable evidence.

Terminal-restoration evidence remains an explicit real-controlling-TTY smoke receipt; it is not synthesized by CI.

## v1.0 criteria

`release/v1.0-criteria.json` is the machine-readable stable gate list and is shipped inside every archive.

The repository is now Apache-2.0 licensed and the Cargo package line is 1.0.0, so the version/license gates are stable-eligible. This still does not constitute a stable release: exact-commit canonical CI, three-platform READY compatibility captures, three terminal-restoration PASS receipts and retained performance evidence must all be present before stable publication.

## Non-goals

M7d3 does not add:

- automatic self-update;
- package-manager publishing;
- signing/notarization claims that are not implemented;
- a remote release database;
- a generic workflow/job engine;
- a second source of product version truth.
