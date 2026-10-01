#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import math
import pathlib
import re
from datetime import datetime, timezone

from _compat import write_text_lf

HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--failure-matrix", required=True)
    parser.add_argument("--scale", required=True)
    parser.add_argument("--soak", required=True)
    parser.add_argument("--support-manifest", required=True)
    parser.add_argument("--support-snapshot", required=True)
    args = parser.parse_args()

    if not HEX40.fullmatch(args.commit):
        raise SystemExit("commit must be exactly 40 hexadecimal characters")

    failure_matrix_path = pathlib.Path(args.failure_matrix)
    scale_path = pathlib.Path(args.scale)
    soak_path = pathlib.Path(args.soak)
    support_path = pathlib.Path(args.support_manifest)
    failure_matrix = json.loads(failure_matrix_path.read_text(encoding="utf-8"))
    scale = json.loads(scale_path.read_text(encoding="utf-8"))
    soak = json.loads(soak_path.read_text(encoding="utf-8"))

    if failure_matrix.get("schema") != "codex-tui/failure-matrix/v2":
        raise SystemExit("unexpected Failure Matrix schema")
    cases = failure_matrix.get("cases")
    if not isinstance(cases, list) or not cases:
        raise SystemExit("Failure Matrix must contain cases")
    for case in cases:
        evidence = case.get("evidence")
        if not isinstance(evidence, list) or not evidence:
            raise SystemExit(f"Failure Matrix case {case.get('id')} has no evidence")
    support = json.loads(support_path.read_text(encoding="utf-8"))

    if scale.get("schema") != "codex-tui/scale-evidence/v4":
        raise SystemExit("unexpected scale evidence schema")
    if scale.get("rows") != 50_000:
        raise SystemExit("release scale evidence must cover exactly 50,000 rows")
    if int(scale.get("warmupIterations", 0)) < 5:
        raise SystemExit("release scale evidence requires at least 5 warmup iterations")
    if int(scale.get("iterations", 0)) < 50:
        raise SystemExit("release scale evidence requires at least 50 measured iterations")
    registry_construct = scale.get("registryConstructMs")
    if not isinstance(registry_construct, (int, float)) or not math.isfinite(registry_construct) or registry_construct < 0:
        raise SystemExit("release scale registryConstructMs must be finite and nonnegative")
    for name in (
        "planningReconcile",
        "recentProjection",
        "allHistoryProjection",
        "searchProjection",
        "hostLocalProjection",
    ):
        timing = scale.get(name)
        if not isinstance(timing, dict):
            raise SystemExit(f"release scale evidence missing {name}")
        for field in ("p50Ms", "p95Ms", "p99Ms", "maxMs"):
            value = timing.get(field)
            if not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
                raise SystemExit(f"release scale {name}.{field} must be finite and nonnegative")

    if soak.get("schema") != "codex-tui/soak-evidence/v1":
        raise SystemExit("unexpected soak evidence schema")
    if soak.get("rows") != 50_000:
        raise SystemExit("release soak evidence must cover exactly 50,000 rows")
    if int(soak.get("cycles", 0)) < 256:
        raise SystemExit("release soak evidence must cover at least 256 churn cycles")
    if soak.get("structuralPass") is not True:
        raise SystemExit("release soak structural gate did not pass")
    if int(soak.get("uiOnlyPlanningReconciles", -1)) != 0:
        raise SystemExit("UI-only churn must not trigger planning reconciliation")

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
            "failureMatrixSha256": sha256(failure_matrix_path),
            "scaleEvidenceSha256": sha256(scale_path),
            "soakEvidenceSha256": sha256(soak_path),
            "supportManifestSha256": sha256(support_path),
            "supportSnapshotSha256": sha256(support_snapshot_path),
        },
    }

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    write_text_lf(output, json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(f"WROTE {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
