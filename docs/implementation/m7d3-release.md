# M7d3: stable/preview release and distributable verification

Status: shipped in v1.0.0; post-release documentation is maintained on main

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

Stable publication is explicitly two-phase. A `stable + publish=true` dispatch must provide `stable_qualification_run`, the run ID of a successful prior `stable + publish=false` release workflow. The publication run downloads that prior run's immutable `release-gate` artifact and verifies that it was a workflow-dispatch run on `main`, succeeded on the same exact source SHA, reported a valid stable verification with `publish=false`, and used the same canonical CI plus retained compatibility, terminal-restoration and performance evidence. The automated qualification receipt may differ because it is regenerated independently on each exact-SHA release run.

Stable qualification also reads the live `main` branch metadata and refuses to continue if `main` has moved away from the workflow's exact source SHA. Stable publication additionally requires `main.protected=true`, so branch protection cannot remain a documentation-only prerequisite. Detailed protection policy (for example force-push/deletion restrictions and required canonical PR checks) still remains repository configuration authority outside the release workflow.

The stable publication authority is the exact retained `release-bundle` produced by the successful prior `stable + publish=false` qualification, not a fresh rebuild. Before creating any stable draft, the publish job downloads that retained bundle, requires its `SHA256SUMS` to cover every bundled file exactly, recomputes every digest, requires exactly three native Linux/macOS/Windows archives, revalidates the embedded stable `release-verification.json` and `release-evidence.json` against the exact version/tag/source SHA, and then replaces the current-run bundle with those already-qualified bytes. The promotion receipt is retained as `codex-tui/stable-bundle-promotion/v1`. This makes the bytes reviewed in `publish=false` the bytes uploaded during `publish=true` without pretending that independent hosted-runner rebuilds are bit-for-bit reproducible. Preview publication continues to use the current-run bundle.

For both preview and stable publication, a successful upload is also verified against GitHub's release-asset REST metadata before the draft can be published and again after publication. The verifier requires the remote asset-name set to equal the complete local bundle, requires every asset state to be `uploaded`, and matches GitHub's API-version-2026-03-10 SHA-256 digest and byte size to the local file. Successful draft and published receipts use `codex-tui/release-asset-verification/v1` and are retained as the `release-asset-integrity` workflow artifact. This makes the published GitHub asset set itself part of the fail-closed release contract rather than treating a successful upload command as sufficient evidence.

The Linux evidence source SHA is not a manual dispatch input: the workflow derives it directly from `github.sha`. This both tightens exact-source binding and keeps the dispatch contract within GitHub's top-level input budget.

The recommended publication path does not manually re-enter the retained inputs. After the successful stable dry-run, use:

```bash
python3 scripts/release/stable_publish.py \
  --stable-qualification-run <successful-stable-publish-false-run-id>
```

This performs a non-publishing preflight: clean/main/origin SHA agreement, absent stable tag, live `main.protected=true`, **GitHub repository immutable releases enabled and verifiable**, released CHANGELOG entry, prior dry-run identity and retained release-evidence validation. It reconstructs the exact publish inputs from the prior immutable `release-gate` artifact. The immutable-release check uses GitHub's repository administration API with the locally authenticated `gh` credential and fails closed if the setting is disabled, inaccessible, or malformed. The workflow repeats the same authoritative check for any direct `stable + publish=true` dispatch using repository secret `CODEX_TUI_ADMIN_READ_TOKEN`, which must be a fine-grained credential with repository **Administration(read)** only. A missing/invalid secret fails publication; preview and stable publish=false do not require it. Release immutability protects future GitHub releases only, so it must be enabled before v1.4 publication. Add `--dispatch` only after reviewing the emitted preflight summary. The helper never accepts compatibility, terminal or performance values as manual command-line inputs.

`publish=true` additionally creates the GitHub Release only after every prior job succeeds and, for stable, the prior dry-run qualification has been validated.

Preview publication uses the same explicit pre-publication recovery discipline as stable publication without requiring the administrative immutable-release credential. At the publish point it rechecks tag absence, creates an unpublished draft carrying a unique preview workflow run/attempt/source-SHA ownership marker, uploads all bundle assets, resolves that draft through the authenticated List releases endpoint, verifies the exact tag/body identity and remote asset digests, then atomically replaces the marker with release notes while setting prerelease=true and draft=false. A pre-publication failure deletes release + tag only when exactly one listed draft, exact tag SHA and owner marker still match this run; missing/ambiguous/foreign identity refuses cleanup. Published preview postconditions require draft=false, prerelease=true and the exact workflow SHA.

Stable publication is recoverable before it becomes public. The workflow writes a unique ownership marker derived from the workflow run/attempt/source SHA into the draft body before asset upload. Draft identity/readback uses the authenticated **List releases** REST contract, which includes draft releases for callers with push access; the published-release-by-tag endpoint is deliberately reserved for post-publication verification. Any pre-publication failure, including an ambiguous client failure while creating the draft, may automatically delete the release and tag only when the listing yields exactly one draft for the tag, its tag still points to the workflow SHA, and its body still equals that workflow ownership marker. Missing or ambiguous draft identity refuses cleanup. Final publication replaces the marker with the reviewed release notes and clears draft status in the same `gh release edit` operation; after that point automatic deletion is disabled. Published stable postconditions require `draft=false`, `prerelease=false`, `immutable=true`, and the release tag to still point to the exact workflow SHA; any failed postcondition is an incident.

An existing tag is treated as a collision and publishing fails closed.

### Final release-notes transition

The active development line intentionally keeps `CHANGELOG.md` at `## [X.Y.Z] - Unreleased` until real qualification is ready to begin. Finalization is explicit and dry-run by default:

```bash
python3 scripts/release/finalize_release.py
```

The preflight requires a clean local `main`, exact `origin/main` agreement, a stable Cargo version and no existing stable tag. It renders the proposed current-UTC release date and validates that the resulting CHANGELOG section is acceptable to `extract_changelog.py --require-released`.

Only when ready to create the final release-notes change:

```bash
python3 scripts/release/finalize_release.py --write
```

The helper changes only the matching `Unreleased` heading; it never commits, pushes, tags or publishes. Review the diff, commit/push normally, then wait for fresh exact-SHA CI, development qualification, performance diagnostics and release preview before collecting real-environment evidence.

## Main branch protection

Stable publication requires live `main.protected=true`. For this personal-first repository, the retained default policy is:

- require the four canonical PR CI checks: `rust-1.88-msrv`, `ubuntu-24.04`, `macos-latest`, `windows-latest`;
- require the branch to be up to date before merge;
- apply protection to administrators as well;
- block force pushes;
- block branch deletion;
- do not require approving reviews, which would make a single-maintainer repository self-blocking.

Preview the exact GitHub REST payload without changing repository settings:

```bash
python3 scripts/release/configure_main_protection.py
```

The helper first requires clean local `main` to equal GitHub `main`, verifies the current exact-SHA canonical checks are successful and produced by GitHub Actions, and validates that the local CI workflow still declares the expected MSRV/platform matrix.

Only after reviewing the emitted plan, apply it with an explicit repository confirmation:

```bash
python3 scripts/release/configure_main_protection.py \
  --apply \
  --confirm-repository jiying2007/codex-tui
```

The apply path uses GitHub's branch-protection REST endpoint and therefore requires a locally authenticated `gh` credential with repository Administration write permission. It re-reads the applied policy and branch metadata and fails unless administrators are protected, all canonical checks are required, force pushes/deletion are disabled, and `main.protected=true`.

## Project license

The release verifier intentionally does not select a project license.

Any `publish=true` run is blocked until the repository contains one of:

- `LICENSE`
- `LICENSE.txt`
- `LICENSE.md`

This gate prevents accidental publication when project license metadata is absent while still allowing retained preview build validation.

## Stable evidence

Stable evidence is represented by `codex-tui/release-evidence/v5`.

It binds:

- release version;
- exact 40-character source SHA;
- canonical successful CI run ID;
- compatibility schema version;
- Linux Tier 1 compatibility report SHA-256 with READY state, observation timestamp and `sourceSha` equal to the release commit;
- Linux Tier 1 terminal-restoration PASS receipt with `sourceSha` equal to the release commit, plus SHA-256 of the exact retained receipt JSON;
- an exact-SHA `codex-tui/automated-qualification/v3` receipt covering Failure Matrix, 50k scale-v4 evidence, 50k structural soak, UI contract, state migration/recovery and support-bundle redaction;
- Linux retained `resident-planning-10k` diagnostic JSON with `sourceSha` equal to the release commit and SHA-256 of the exact retained report;
- optional macOS/Windows Tier 2 retained receipts when available.

The stable verifier requires at least 200 retained resident-planning-10k samples with finite nonnegative p95/p99 values, but the active v1.4 policy does not fail solely on hosted-runner latency thresholds. Compatibility report hashes, terminal-restoration receipt hashes, retained performance-report hashes and automated-qualification artifact hashes are exact SHA-256 values.

The workflow independently calls the GitHub Actions API and verifies the supplied canonical CI run is the `ci` workflow on `main`, succeeded, and is bound to the release source SHA.

## Hosted development qualification

Every push to `main` retains a source-bound `codex-tui/development-qualification/v1` artifact. It combines the exact-SHA automated hardening receipt with the current module ratchet and retained App Server protocol replay fixtures.

This hosted receipt is development authority only: it always records `stableReady=false` and `publicationAllowed=false`. It is never accepted in place of `codex-tui/release-evidence/v5`, so automated v1.4 development can continue without fabricating Linux compatibility, real controlling-TTY restoration or retained Linux performance evidence.

The release gate independently rechecks the architecture ratchet, protocol replay, cargo-deny policy and RustSec advisories on the exact checkout before packaging. It retains both `development-qualification.json` and `security-governance.json` in the release-gate artifact so the hosted development gates are auditable instead of existing only as workflow logs.

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

Each archive contains the native binary, Apache-2.0 LICENSE, README, CHANGELOG, install/upgrade guidance, third-party notices, v1 stable criteria and release metadata. Packaging fails closed unless Cargo metadata declares `Apache-2.0` and the root `LICENSE` is present; archive smoke revalidates both the license text and release metadata SPDX value.

Release metadata now uses `codex-tui/release-artifact/v2`. In addition to exact
source and binary identity, it records structured rustc/cargo release, full commit
SHA, commit date and native host values collected from `rustc -vV` and
`cargo -Vv`. This is retained build provenance, not a claim that mutable hosted
runner images or system packages make the archive bit-reproducible across future runs.

## Archive smoke

Every platform extracts its newly built archive and executes the packaged binary:

1. `codex-tui --version` must exactly match the Cargo version;
2. `codex-tui headless threads --fixture-10k --json` must report the v1 headless schema, no degradation and exactly 10,000 rows;
3. required documentation/notice/criteria/metadata files must exist;
4. release metadata version/tag/SHA must match the gate outputs;
5. release-artifact/v2 rustc/cargo provenance must contain full commit SHAs and both tool hosts must equal the declared native host triple.

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
python3 scripts/release/capture_compat.py \
  --binary <path-to-v1-candidate-binary> \
  --output compat-<platform>.json \
  --expected-source-sha "$(git rev-parse HEAD)"
```

The compatibility helper refuses anything other than `readiness=ready`, requires the report `sourceSha` to equal the expected candidate SHA, and prints the report SHA-256 and observation timestamp used by stable evidence.

Terminal-restoration evidence remains an explicit real-controlling-TTY smoke receipt; it is not synthesized by CI.

For Linux Tier 1, use the two-phase helper so the receipt is bound to the same real controlling TTY and exact main SHA without manually typing the SHA or terminal identifier:

```bash
python3 scripts/release/terminal_smoke.py prepare
```

The prepare step requires stdin/stdout/stderr plus `/dev/tty` to be real TTYs, requires clean `main == origin/main`, verifies the parent terminal already has `ICANON/ECHO/ISIG`, writes an ignored pending manifest under `release/evidence/linux/`, and prints the exact `CODEX_TUI_GIT_SHA=... cargo run --release --locked --bin codex-tui` candidate command plus the documented interactive procedure.

After completing the real Drawer/resize/Ctrl-C/focus/normal-exit/abnormal-exit observations and verifying cursor, echo and line editing in the parent terminal:

```bash
python3 scripts/release/terminal_smoke.py record-pass --pass
```

`record-pass` must run on the same controlling TTY and exact SHA, rechecks clean/live main plus `ICANON/ECHO/ISIG`, and then delegates to `create_terminal_receipt.py`. The explicit `--pass` remains mandatory because software cannot honestly infer the user-observed cursor and line-editing restoration.

## Versioned stable criteria

Release qualification uses the current major.minor criteria file. For the active v1.4.0 line the authority is `release/v1.4-criteria.json`, and archives expose it as `STABLE-CRITERIA.json`. `release/v1.0-criteria.json` through `release/v1.3-criteria.json` remain historical line authorities; the parked v1.1 candidate is preserved separately on `release/v1.1-parked`.

The repository is Apache-2.0 licensed and v1.0.0 was published on 2026-09-30. v1.4 stable publication is fail-closed on exact-commit automated hardening, architecture ratchet, protocol replay, dependency-security checks, canonical CI, Linux Tier 1 retained compatibility and real terminal restoration. macOS and Windows remain required in canonical CI and native package/archive smoke as Tier 2 automated-compatibility platforms; their real-environment retained receipts are optional.

## Non-goals

M7d3 does not add:

- automatic self-update;
- package-manager publishing;
- signing/notarization claims that are not implemented;
- a remote release database;
- a generic workflow/job engine;
- a second source of product version truth.

## Linux-first v1 stable support policy

For the v1 stable line, Linux is the Tier 1 stable platform.

Stable-blocking real-world retained evidence:

- Linux `compat/v2` readiness = READY;
- Linux real controlling-TTY restoration smoke = PASS.

Repository-internal stable gates are exact-SHA automated hardening plus canonical CI/package smoke. The Linux resident-planning-10k benchmark remains retained with >= 200 samples for diagnosis and trend comparison, but its hosted-runner p95/p99 values are not an independent v1.4 release blocker.

macOS and Windows remain Tier 2 automated-compatibility platforms:

- canonical CI remains required on both;
- native release build, notices, archive construction, Apache-2.0 archive smoke, binary smoke and bundle checks remain required;
- their real Codex-environment compatibility and real-TTY receipts are optional for v1 stable releases.

The support policy is intentionally asymmetric so Linux stable is not blocked by unavailable macOS/Windows real-environment evidence while cross-platform build regressions still fail the release.

### Linux qualification command

After the real-TTY smoke receipt exists, run from a clean `main` checkout of the exact candidate SHA:

```bash
python3 scripts/release/linux_qualify.py \
  --terminal-receipt release/evidence/linux/terminal-linux.json \
  --source <retained-linux-machine-id>

# Optional one-shot mode: add --dispatch to the qualification command.
# Dispatch happens only after every local gate above succeeds.
```

It performs:

1. clean-main / exact-SHA validation;
2. automatic discovery and validation of the latest successful canonical `ci` push run bound to the exact current `main` SHA; `--canonical-ci-run <id>` remains an explicit diagnostic override;
3. locked all-target tests plus release build;
4. Linux READY compatibility capture + SHA-256;
5. exact-SHA 50k scale-v4 capture with registry construction, planning reconcile and recent/all-history/search/host-local distributions;
6. exact-SHA 50k/256 structural soak and secret-safe Doctor Bundle capture;
7. `automated-qualification/v3` assembly with SHA-256 bindings for Failure Matrix, scale, soak, support manifest and support snapshot; the support snapshot source SHA must equal the candidate SHA;
8. 20 warmup + 200 measured resident-planning-10k diagnostic samples;
9. Linux terminal receipt validation including exact candidate `sourceSha`, followed by SHA-256 over the exact retained receipt bytes;
10. `release-evidence/v5` assembly with source-bound compatibility report SHA-256 and terminal-restoration receipt SHA-256;
11. local stable release verification for the current Cargo package version.

The result is retained under `release/evidence/linux/` and includes a complete `workflowInputs` object. `--dispatch` submits those exact values to the GitHub `release.yml` workflow with `channel=stable` and `publish=false`; it never publishes a release.


### Mission Control host-local sessions

Mission Control classifies each Codex thread cwd against the current host:

- `L` / `local`: cwd is a directory that exists on this host; Terminal Drawer is allowed.
- `F` / `foreign-windows` or `foreign-unix`: cwd belongs to another OS; Terminal Drawer is blocked.
- `!` / `stale`: native absolute cwd no longer exists.
- `?`: cwd is empty or relative.

Press `/`, type `local`, then Enter to show only sessions whose cwd exists on the current host.

Codex app-server may normalize a stored Windows cwd while running on Linux, yielding a value such as `/linux/current/dir/C:\\Users\\...`. codex-tui detects the embedded foreign Windows path, displays the Windows portion as foreign, and never uses that value as a Linux PTY cwd.

Run `codex-tui doctor codex` to see the active Codex home plus local/foreign/stale session counts and sample cwd values.
\n\nStable `publish=true` skips the redundant current-run three-platform package/bundle rebuild because publication authority is the exact validated prior `publish=false` release-bundle. The current exact-SHA gate still reruns before publication; preview and stable dry-runs retain the full Linux/macOS/Windows packaging and archive-smoke matrix.\n