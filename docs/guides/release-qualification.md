<!-- docs-id: release-qualification -->
<!-- docs-lang: en -->
# v1.4 release qualification and external handoff
<!-- docs-section: overview -->

**Language / 语言:** [English](release-qualification.md) · [简体中文](../zh-CN/guides/release-qualification.md)

v1.4 is development-scope-complete, **not** a published Stable version or a proven deployment. Current public GitHub release v1.0.0 is historical and was not adopted in a deployed installation. This page is an operator checklist, not a new release authority. Follow [the canonical criteria](../../release/v1.4-criteria.json) and [handoff #211](https://github.com/jiying2007/codex-tui/issues/211).

## Machine-owned gates
<!-- docs-section: automated -->

On the **same protected main SHA**, require Linux/macOS/Windows Rust CI, Development Qualification, Security/Release Gate, 10k/50k scale, rendered interaction diagnostics, terminal PTY regression and retained structural soak as triggered by current policy. Retain workflow IDs and artifact SHA-256. Release artifacts must pass native metadata, ABI, license/notice and checksum validation. A green PR does not replace fresh-main qualification.

Development Qualification also retains `failure-case-execution.json` for exact individually rerun Failure Matrix test identifiers, measured process wall times and output digests. The receipt sets `recoveryLatencyMeasured=false`: process startup and test-harness time are NOT the application's fault-recovery latency, and no `maxRecoveryMs` target is declared passed without independent fault-injection timing evidence.

Protocol qualification is intentionally tiered: **L1** replays retained registry/unknown/malformed frames; **L2** uses synthetic WebSocket/Unix transport and App Server registry lifecycle tests (including ordered notifications, disconnect and bad JSON). L2 also verifies that a synthetic server request arriving during an awaited RPC is handed back to the event dispatcher promptly (without claiming RPC success or dropping the request). Neither tier authenticates a live Codex target. **L3** requires real current Codex App Server, actual capabilities and terminal/provider evidence, and remains externally blocked until observed on the exact candidate.

## Real environment requirements
<!-- docs-section: external -->

[Device integration runbook](device-validation.md) and [NOT_RUN witness matrix](device-matrix.md) use repository-local helpers for existing fixture execution, read-only process observation and consistent SQLite backup. These helpers do not record real-TTY PASS or trigger publication.

`capture_forge_capability.py` and `validate_internal_gitlab.py` refuse existing output files or symbolic links, including dangling links, and use exclusive creation so a competing writer cannot replace retained evidence. Forge capture validates typed build/time/provider/capability metadata before projecting a fixture; launch, decode, snapshot and write errors never echo arbitrary child output or invalid values. Conflicting duplicate capability requirements are rejected before probing. Missing optional server metadata, provider aliases, well-formed additional capability names and explicitly unqualified diagnostic fixtures remain supported; an unmet requirement is retained with exit code 3, not promoted to admission. The internal GitLab gate still hashes the exact input bytes and retains its existing age, source, authentication and capability checks with no Stable authority. These protections do not make local storage immutable or certify real credentials.

`capture_compat.py` gives the compatibility doctor a 45-second execution timeout and never echoes failed child output or invalid report values. It requires a ready, exact-source report with typed build identity and a positive observation timestamp. Existing output files (including symbolic links) are refused before probing; exclusive creation also protects a report created by another collector during the probe. Failures emit no success summary. Choose a new output path for each capture; this protects retained evidence from accidental replacement, not from later filesystem modification.

`linux_qualify.py` validates the supplied Linux terminal receipt before any network query, compilation or benchmark. Missing, malformed, non-PASS or wrong-source input stops immediately. The accepted UTF-8 bytes are retained verbatim as `terminal-restoration.json` in a fresh `--output-dir`; parsing and SHA-256 use that same snapshot, not a later read of the input path. An existing retained snapshot is never overwritten. This verifies the operator-supplied receipt, not the truth of a real-TTY attestation; it does not synthesize PASS or authorize publication.

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

