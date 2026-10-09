<!-- docs-id: install-upgrade -->
<!-- docs-lang: en -->
# Install and upgrade
<!-- docs-section: overview -->

**Language / 语言:** [English](install-upgrade.md) · [简体中文](../zh-CN/release/install-upgrade.md)


This document describes the archive-based codex-tui release contract. It does not introduce an installer service or auto-updater.

## Verify a release bundle
<!-- docs-section: verify -->

Every release bundle contains:

- one native archive for each canonical build platform;
- `SHA256SUMS`;
- `RELEASE_NOTES.md`;
- `release-verification.json`;
- `STABLE-CRITERIA.json` for the release's current major.minor qualification policy;
- stable evidence receipt when the channel is stable.

Verify the checksum for the archive you plan to install before extracting it.

Current bilingual guide files remain usable offline. When a linked historical
design/implementation/source document is not bundled, the packager replaces
that relative link with a GitHub URL pinned to the **exact source commit SHA**.
These historical links require network access; the current bilingual pages
and language switches do not. Archive verification rejects any remaining
broken local Markdown link instead of silently shipping dead navigation.

Each platform archive contains:

- `codex-tui` or `codex-tui.exe`;
- `README.md` and `README.zh-CN.md`;
- `CHANGELOG.md`;
- `INSTALL-UPGRADE.md` and `INSTALL-UPGRADE.zh-CN.md`;
- `TEAM-QUICKSTART.md` and `TEAM-QUICKSTART.zh-CN.md`;
- `THIRD_PARTY_NOTICES.txt`;
- `STABLE-CRITERIA.json`;
- `RELEASE-METADATA.json`;
- `LICENSE` with the Apache License 2.0.

Starting with the v1.4 first-deployment packaging contract, archives also
retain `docs/i18n/manifest.json` and **both locales of every active
operator/governance page** at its original repository-relative path,
including CONTRIBUTING, SECURITY, SUPPORT, CLI and troubleshooting.
The independent archive verifier refuses a missing page, wrong locale or
missing section. Historical v1.0 archives retain their original English-only
layout for verifiability; this addition does not rewrite an old GitHub release.

## Native archive identity
<!-- docs-section: identity -->

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
<!-- docs-section: abi -->

The v1.4 GNU x86-64 distributable is built and archive-smoked in Ubuntu 20.04
(glibc 2.31). `LINUX-ABI.json` records the binary hash, exact source SHA and
versioned imported symbols; strong requirements above glibc 2.31 fail packaging.
Optional weak imports are recorded separately and do not raise that floor.
This is an ABI compatibility floor, not a recommendation to run an unmaintained OS.
Use a security-maintained distribution or the appropriate vendor extended support.
Git command compatibility, Python 3.8 release-helper compatibility, and the GNU
binary ABI are separate tested contracts. Musl/Alpine is not this GNU target.

## Install
<!-- docs-section: install -->

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

## First production deployment (v1.4)
<!-- docs-section: first-deploy -->

The public v1.0.0 GitHub Release is retained as history, but the maintainer
confirms that neither v1.0 nor v1.4 has been deployed. v1.4 is the first
operational installation, **not a v1.0-v1.3 in-place upgrade**. Start with
fresh local state and normal TOML configuration. Do not expect legacy
`state-v1.json` import or old SQLite version migration. Obsolete state
files are neither imported nor deleted: retain them for audit.

New SQLite state initializes at schema v4; only current v4 is accepted for
existing databases and recovery. Predeployment v1-v3 images and unsupported
future versions fail closed without overwriting them. Current Codex App Server,
GitLab/GitHub, Linux ABI and SSH controlling-TTY still require qualification.

## Upgrade
<!-- docs-section: upgrade -->

The following procedure applies **only after an actual v1.4-or-newer
deployment** and an explicit storage/rollback compatibility review. It
does not authorize upgrading v1.0-v1.3 product state.

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
<!-- docs-section: rollback -->

If an upgrade cannot pass Doctor:

1. stop the new binary;
2. restore the previous binary;
3. retain the current state/database for diagnosis instead of repeatedly deleting or recreating it;
4. use the release metadata and compatibility JSON to compare the failing environment.

A rollback must not be represented as a successful stable upgrade until the compatibility blocker is understood.

## Preview versus stable
<!-- docs-section: preview -->

Preview artifacts use tags shaped like:

```text
vX.Y.Z-preview.N
```

Preview is intended for retained validation. It may lack stable-only human evidence.

Stable tags are exactly:

```text
vX.Y.Z
```

The first permitted stable product line is v1.0.0. Stable publication fails closed if any required evidence is absent. Both the supported `stable_publish.py` path and a direct `stable + publish=true` workflow dispatch require GitHub repository **release immutability** to be enabled and verifiable and require the canonical **main branch protection policy** to remain intact: strict canonical checks, each check pinned to the GitHub Actions App dynamically observed on the exact source SHA, administrator enforcement, force pushes disabled and branch deletion disabled. Direct workflow publication additionally requires repository secret `CODEX_TUI_ADMIN_READ_TOKEN` with fine-grained **Administration(read)** permission; absence, inaccessible administration state, app-source drift or policy drift is fail-closed. Preview and stable publish=false remain credential-free. Enable immutability before the v1.4 release. The setting protects future releases and does not retroactively make older releases immutable.

## Project license
<!-- docs-section: license -->

codex-tui is licensed under Apache-2.0. Release packaging requires both `Cargo.toml license = "Apache-2.0"` and the root `LICENSE`; extracted archive smoke verifies that the license is present and identified as Apache License 2.0.

## Consistent backups and recovery
<!-- docs-section: backup -->

Recovery backups use SQLite's online backup API, not a copy of the live main
database. Committed WAL data is included even when an older reader prevents a
full checkpoint. Backup copying has a five-second retry deadline; the destination
is staged, validated, synced and published without replacing an existing file.
Recovery/rollback remains an offline operation: close every codex-tui instance
and retain the previous database/WAL/SHM image before restoring.
