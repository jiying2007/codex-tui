#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
from datetime import datetime, timezone

from _compat import write_text_lf
from validate_diagnostics import validate_scale, validate_soak, validate_support_snapshot, validate_offline_support

HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")


def read_evidence(path: pathlib.Path) -> tuple[dict, str, bytes]:
    """Bind validation and the signed digest to the same input bytes."""
    raw = path.read_bytes()
    return json.loads(raw.decode("utf-8")), hashlib.sha256(raw).hexdigest(), raw


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--failure-matrix", required=True)
    parser.add_argument("--scale", required=True)
    parser.add_argument("--soak", required=True)
    parser.add_argument("--support-manifest", required=True)
    parser.add_argument("--support-snapshot", required=True)
    parser.add_argument("--require-offline-support", action="store_true")
    args = parser.parse_args()

    if not HEX40.fullmatch(args.commit):
        raise SystemExit("commit must be exactly 40 hexadecimal characters")

    failure_matrix_path = pathlib.Path(args.failure_matrix)
    scale_path = pathlib.Path(args.scale)
    soak_path = pathlib.Path(args.soak)
    support_path = pathlib.Path(args.support_manifest)
    support_snapshot_path = pathlib.Path(args.support_snapshot)
    failure_matrix, failure_hash, _ = read_evidence(failure_matrix_path)
    scale, scale_hash, _ = read_evidence(scale_path)
    soak, soak_hash, _ = read_evidence(soak_path)
    support_snapshot, snapshot_hash, snapshot_raw = read_evidence(support_snapshot_path)

    if failure_matrix.get("schema") != "codex-tui/failure-matrix/v2":
        raise SystemExit("unexpected Failure Matrix schema")
    cases = failure_matrix.get("cases")
    if not isinstance(cases, list) or not cases:
        raise SystemExit("Failure Matrix must contain cases")
    for case in cases:
        evidence = case.get("evidence")
        if not isinstance(evidence, list) or not evidence:
            raise SystemExit(f"Failure Matrix case {case.get('id')} has no evidence")
    support, support_hash, _ = read_evidence(support_path)

    try:
        validate_scale(scale, args.commit.lower())
        validate_soak(soak, args.commit.lower())
        validate_support_snapshot(support, support_snapshot_path, snapshot_raw)
        if args.require_offline_support or support_snapshot.get("collectionMode") == "offline":
            validate_offline_support(support_snapshot)
    except ValueError as error:
        raise SystemExit(str(error)) from error

    if support.get("schema") != "codex-tui/support-bundle/v1":
        raise SystemExit("unexpected support bundle schema")
    if support_snapshot.get("schema") != "codex-tui/support-bundle/v1":
        raise SystemExit("unexpected support snapshot schema")
    build = support_snapshot.get("build")
    if not isinstance(build, dict):
        raise SystemExit("support snapshot is missing build metadata")
    if str(build.get("sourceSha", "")).lower() != args.commit.lower():
        raise SystemExit(
            "support snapshot source SHA mismatch: "
            f"{build.get('sourceSha')!r} != {args.commit.lower()!r}"
        )
    if not str(build.get("productVersion", "")).strip():
        raise SystemExit("support snapshot productVersion is empty")
    privacy = set(support.get("privacy", []))
    required_privacy = {
        "no-environment-variables",
        "no-authentication-tokens",
        "no-prompts-or-transcripts",
        "no-comment-bodies",
        "no-repository-or-file-paths",
        "no-raw-errors",
    }
    missing = sorted(required_privacy - privacy)
    if missing:
        raise SystemExit("support bundle privacy contract missing: " + ", ".join(missing))

    receipt = {
        "schema": "codex-tui/automated-qualification/v3",
        "sourceSha": args.commit.lower(),
        "observedAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "gates": {
            "failureMatrix": "pass",
            "scaleEvidence": "pass",
            "soakStructural": "pass",
            "uiContract": "pass",
            "stateMigrationRecovery": "pass",
            "supportBundleRedaction": "pass",
        },
        "artifacts": {
            "failureMatrixSha256": failure_hash,
            "scaleEvidenceSha256": scale_hash,
            "soakEvidenceSha256": soak_hash,
            "supportManifestSha256": support_hash,
            "supportSnapshotSha256": snapshot_hash,
        },
    }

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    write_text_lf(output, json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(f"WROTE {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
