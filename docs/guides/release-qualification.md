<!-- docs-id: release-qualification -->
<!-- docs-lang: en -->
# v1.4 release qualification and external handoff
<!-- docs-section: overview -->

**Language / 语言:** [English](release-qualification.md) · [简体中文](../zh-CN/guides/release-qualification.md)

v1.4 is development-scope-complete, **not** a published Stable version or a proven deployment. Current public GitHub release v1.0.0 is historical and was not adopted in a deployed installation. This page is an operator checklist, not a new release authority. Follow [the canonical criteria](../../release/v1.4-criteria.json) and [handoff #211](https://github.com/jiying2007/codex-tui/issues/211).

## Machine-owned gates
<!-- docs-section: automated -->

On the **same protected main SHA**, require Linux/macOS/Windows Rust CI, Development Qualification, Security/Release Gate, 10k/50k scale, rendered interaction diagnostics, terminal PTY regression and retained structural soak as triggered by current policy. Retain workflow IDs and artifact SHA-256. Release artifacts must pass native metadata, ABI, license/notice and checksum validation. A green PR does not replace fresh-main qualification.

Protocol qualification is intentionally tiered: **L1** replays retained registry/unknown/malformed frames; **L2** uses synthetic WebSocket/Unix transport and App Server registry lifecycle tests (including ordered notifications, disconnect and bad JSON). Neither tier authenticates a live Codex target. **L3** requires real current Codex App Server, actual capabilities and terminal/provider evidence, and remains externally blocked until observed on the exact candidate.

## Real environment requirements
<!-- docs-section: external -->

Separate, current-source receipts are required for (1) actual authenticated Codex App Server/provider capabilities, (2) internal GitLab `glab` and required Issues/MRs/Pipelines where the internal deployment profile is used, (3) Linux and Windows→SSH Ubuntu **controlling terminal** focus/resize/Ctrl+C/exit/restore, and (4) operator review of target machine performance, resource and error recovery. Repository CI cannot synthesize these as PASS.

## Administrator-owned release protection
<!-- docs-section: admin -->

Before Stable the administrator must verify strict protected `main` required-checks (including GitHub Actions App ID binding), disabled force-push/deletion, enforcement for admins and **Repository Immutable Releases**. Unauthorized/inaccessible readback is a blocker, not a permission to skip. Never inject admin credentials into public PR workflows.

## Exact-source Stable dry-run
<!-- docs-section: dryrun -->

A Stable `publish=false` run is separate from ordinary nonpublishing Preview. Bind the exact candidate version/tag, canonical CI, real Linux Doctor/TTY/performance and remaining `release-evidence/v5` fields before declaring the dry-run qualified. Do not invent timestamps, redact hashes or reuse prior-candidate evidence.

## Authorized publication only
<!-- docs-section: publication -->

A reviewed changelog release date, immutable-release/readback and branch-head rechecks, exact prior dry-run assets and **explicit owner authorization** are required before any Stable `publish=true`. Publication and GitHub Release verification remain separate security-sensitive operations. No repository docs or synthetic fixture override those gates.

## Handoff and historical records
<!-- docs-section: archive -->

Keep the current [Issue #211](https://github.com/jiying2007/codex-tui/issues/211) open until external/admin evidence is genuinely collected. Do not force-delete divergent historical branches, rewrite v1.0.0 GitHub release history or treat v1.2/v1.3 retained completion files as a currently deployed upgrade path. This is a **first** v1.4 installation, not a compatibility migration.

