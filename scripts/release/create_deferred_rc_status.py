#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
from datetime import datetime, timezone

from _compat import write_text_lf

SCHEMA = "codex-tui/deferred-rc-qualification/v1"
PLAN_SCHEMA = "codex-tui/rc-plan/v1"
AUTOMATED_SCHEMA = "codex-tui/automated-qualification/v3"
HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Create a source-bound RC status receipt for automated qualification while "
            "real-environment evidence remains explicitly deferred."
        )
    )
    parser.add_argument("--output", required=True)
    parser.add_argument("--plan", default="release/v1.1-rc-plan.json")
    parser.add_argument("--automated-qualification", required=True)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()

    commit = args.commit.strip().lower()
    if not HEX40.fullmatch(commit):
        raise SystemExit("--commit must be exactly 40 hexadecimal characters")

    plan_path = pathlib.Path(args.plan)
    automated_path = pathlib.Path(args.automated_qualification)
    plan = load(plan_path)
    automated = load(automated_path)

    if plan.get("schema") != PLAN_SCHEMA:
        raise SystemExit(f"unexpected RC plan schema: {plan.get('schema')!r}")
    if plan.get("phase") != "code-frozen-awaiting-real-environment-qualification":
        raise SystemExit("deferred RC receipt requires the parked real-evidence phase")
    if automated.get("schema") != AUTOMATED_SCHEMA:
        raise SystemExit(
            f"unexpected automated qualification schema: {automated.get('schema')!r}"
        )
    if str(automated.get("sourceSha", "")).lower() != commit:
        raise SystemExit("automated qualification source SHA mismatch")

    required_gates = plan.get("automatedQualification", {}).get("requiredGates")
    gates = automated.get("gates")
    if not isinstance(required_gates, list) or not required_gates:
        raise SystemExit("RC plan automated requiredGates is empty")
    if not isinstance(gates, dict):
        raise SystemExit("automated qualification gates are missing")
    failed = [gate for gate in required_gates if gates.get(gate) != "pass"]
    if failed:
        raise SystemExit(
            "automated qualification is not fully passing: " + ", ".join(failed)
        )

    deferred = plan.get("deferredRealEvidence")
    if not isinstance(deferred, list) or not deferred:
        raise SystemExit("RC plan deferredRealEvidence is empty")

    normalized = []
    for item in deferred:
        if not isinstance(item, dict) or not item.get("id"):
            raise SystemExit("invalid deferred evidence item")
        if item.get("canBeSynthesized") is not False:
            raise SystemExit(f"{item['id']} must remain explicitly non-synthesizable")
        normalized.append(
            {
                "id": item["id"],
                "criteriaGate": item.get("criteriaGate"),
                "requiredForStable": item.get("requiredForStable") is True,
                "requiredForTeamReuseQualification": (
                    item.get("requiredForTeamReuseQualification") is True
                ),
                "status": "deferred-external",
                "canBeSynthesized": False,
            }
        )

    stable_blockers = sorted(
        item["id"] for item in normalized if item["requiredForStable"]
    )
    team_reuse_blockers = sorted(
        item["id"]
        for item in normalized
        if item["requiredForTeamReuseQualification"]
    )
    if not stable_blockers:
        raise SystemExit("deferred RC receipt must retain stable external blockers")

    receipt = {
        "schema": SCHEMA,
        "targetVersion": plan.get("targetVersion"),
        "sourceSha": commit,
        "phase": plan.get("phase"),
        "observedAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "repositoryAutomatedQualification": {
            "status": "pass",
            "schema": automated["schema"],
            "receiptSha256": sha256(automated_path),
            "gates": {gate: gates[gate] for gate in required_gates},
        },
        "deferredEvidence": normalized,
        "stableBlockers": stable_blockers,
        "teamReuseBlockers": team_reuse_blockers,
        "stableReady": False,
        "publicationAllowed": False,
        "authority": "hosted-automated-only",
        "policy": (
            "This receipt proves repository-hosted automated qualification only. "
            "It is not codex-tui/release-evidence/v4 and cannot satisfy stable "
            "real-environment evidence."
        ),
    }

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    write_text_lf(output, json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(f"WROTE {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
