# Install and upgrade

This document describes the archive-based codex-tui release contract. It does not introduce an installer service or auto-updater.

## Verify a release bundle

Every release bundle contains:

- one native archive for each canonical build platform;
- `SHA256SUMS`;
- `RELEASE_NOTES.md`;
- `release-verification.json`;
- `STABLE-CRITERIA.json` for the release's current major.minor qualification policy;
- stable evidence receipt when the channel is stable.

Verify the checksum for the archive you plan to install before extracting it.

Each platform archive contains:

- `codex-tui` or `codex-tui.exe`;
- `README.md`;
- `CHANGELOG.md`;
- `INSTALL-UPGRADE.md`;
- `TEAM-QUICKSTART.md`;
- `THIRD_PARTY_NOTICES.txt`;
- `STABLE-CRITERIA.json`;
- `RELEASE-METADATA.json`;
- `LICENSE` with the Apache License 2.0.

## Native archive identity

Every newly built platform archive uses `codex-tui/release-artifact/v2` and
records `binarySha256` plus exact rustc/cargo build-toolchain provenance in
`RELEASE-METADATA.json`. Native archive smoke verifies the schema, declared native
platform/host-triple family, executable name, exact source SHA, actual payload hash,
and that both compiler/tool identities contain full commit SHAs and the same native
host triple before exercising the extracted executable. An existing backend-free
one-sample diagnostic also checks the binary's embedded source SHA; that sample
is identity verification, not qualified performance evidence.

Toolchain provenance makes later artifact drift diagnosable; it does not claim
bit-for-bit reproducibility across mutable hosted-runner images, system packages,
or container tags.

Extraction preflight rejects duplicate/case-colliding or aliased member paths,
links/special files, multiple roots and file/directory collisions. It accepts at
most 4,096 members and 256 MiB of declared extracted content. These checks do not
authenticate an arbitrary executable: use only artifacts from the trusted,
exact-source release workflow and verify the supplied artifact/checksum evidence.
The verifier intentionally refuses older metadata lacking a payload hash rather
than silently weakening the current release contract.

## Linux ABI baseline

The v1.4 GNU x86-64 distributable is built and archive-smoked in Ubuntu 20.04
(glibc 2.31). `LINUX-ABI.json` records the binary hash, exact source SHA and
versioned imported symbols; strong requirements above glibc 2.31 fail packaging.
Optional weak imports are recorded separately and do not raise that floor.
This is an ABI compatibility floor, not a recommendation to run an unmaintained OS.
Use a security-maintained distribution or the appropriate vendor extended support.
Git command compatibility, Python 3.8 release-helper compatibility, and the GNU
binary ABI are separate tested contracts. Musl/Alpine is not this GNU target.

## Install

1. Extract the archive for the current platform/host triple.
2. Move the binary to a directory on `PATH`, or run it directly from the extracted directory.
3. Confirm the binary:
   ```text
   codex-tui --version
   ```
4. Run compatibility diagnostics:
   ```text
   codex-tui doctor compat
   codex-tui doctor compat --json
   ```
5. For a repository that uses Forge integration, run the explicit repository-scoped probe:
   ```text
   codex-tui doctor forge
   ```

No Nerd Font is required.

## Upgrade

Before replacing an existing binary:

1. close active codex-tui sessions and Terminal Drawer children;
2. retain a backup of the codex-tui local application/state directory if the installation is important;
3. replace only the binary;
4. run `codex-tui doctor store` and `codex-tui doctor compat`;
5. if diagnostics are degraded, capture `codex-tui doctor bundle --output <empty-dir>` before changing state;
6. open the normal Registry and one representative repository before deleting the previous binary.

Canonical Codex, Git and Forge data remain owned by those systems. codex-tui local state is operator metadata and planning state.

Do not copy a SQLite database between two simultaneously running codex-tui instances.

## Rollback

If an upgrade cannot pass Doctor:

1. stop the new binary;
2. restore the previous binary;
3. retain the current state/database for diagnosis instead of repeatedly deleting or recreating it;
4. use the release metadata and compatibility JSON to compare the failing environment.

A rollback must not be represented as a successful stable upgrade until the compatibility blocker is understood.

## Preview versus stable

Preview artifacts use tags shaped like:

```text
vX.Y.Z-preview.N
```

Preview is intended for retained validation. It may lack stable-only human evidence.

Stable tags are exactly:

```text
vX.Y.Z
```

The first permitted stable product line is v1.0.0. Stable publication fails closed if any required evidence is absent. Both the supported `stable_publish.py` path and a direct `stable + publish=true` workflow dispatch require GitHub repository **release immutability** to be enabled and verifiable and require the canonical **main branch protection policy** to remain intact: strict canonical checks, administrator enforcement, force pushes disabled and branch deletion disabled. Direct workflow publication additionally requires repository secret `CODEX_TUI_ADMIN_READ_TOKEN` with fine-grained **Administration(read)** permission; absence, inaccessible administration state or policy drift is fail-closed. Preview and stable publish=false remain credential-free. Enable immutability before the v1.4 release. The setting protects future releases and does not retroactively make older releases immutable.

## Project license

codex-tui is licensed under Apache-2.0. Release packaging requires both `Cargo.toml license = "Apache-2.0"` and the root `LICENSE`; extracted archive smoke verifies that the license is present and identified as Apache License 2.0.

## Consistent backups and recovery

Recovery backups use SQLite's online backup API, not a copy of the live main
database. Committed WAL data is included even when an older reader prevents a
full checkpoint. Backup copying has a five-second retry deadline; the destination
is staged, validated, synced and published without replacing an existing file.
Recovery/rollback remains an offline operation: close every codex-tui instance
and retain the previous database/WAL/SHM image before restoring.
