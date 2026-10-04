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

The first permitted stable product line is v1.0.0. Stable publication fails closed if any required evidence is absent.

## Project license

codex-tui is licensed under Apache-2.0. Release packaging requires both `Cargo.toml license = "Apache-2.0"` and the root `LICENSE`; extracted archive smoke verifies that the license is present and identified as Apache License 2.0.

## Consistent backups and recovery

Recovery backups use SQLite's online backup API, not a copy of the live main
database. Committed WAL data is included even when an older reader prevents a
full checkpoint. Backup copying has a five-second retry deadline; the destination
is staged, validated, synced and published without replacing an existing file.
Recovery/rollback remains an offline operation: close every codex-tui instance
and retain the previous database/WAL/SHM image before restoring.
