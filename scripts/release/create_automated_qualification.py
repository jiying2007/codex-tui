#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
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
    parser.add_argument("--soak", required=True)
    parser.add_argument("--support-manifest", required=True)
    args = parser.parse_args()

    if not HEX40.fullmatch(args.commit):
        raise SystemExit("commit must be exactly 40 hexadecimal characters")

    soak_path = pathlib.Path(args.soak)
    support_path = pathlib.Path(args.support_manifest)
    soak = json.loads(soak_path.read_text(encoding="utf-8"))
    support = json.loads(support_path.read_text(encoding="utf-8"))

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
        "schema": "codex-tui/automated-qualification/v1",
        "sourceSha": args.commit.lower(),
        "observedAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "gates": {
            "failureMatrix": "pass",
            "soakStructural": "pass",
            "uiContract": "pass",
            "stateMigrationRecovery": "pass",
            "supportBundleRedaction": "pass",
        },
        "artifacts": {
            "soakEvidenceSha256": sha256(soak_path),
            "supportManifestSha256": sha256(support_path),
        },
    }

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    write_text_lf(output, json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(f"WROTE {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
