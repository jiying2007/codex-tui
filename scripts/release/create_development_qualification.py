#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
from datetime import datetime, timezone

from _compat import write_text_lf

SCHEMA = "codex-tui/development-qualification/v1"
PLAN_SCHEMA = "codex-tui/v1.2-plan/v1"
AUTOMATED_SCHEMA = "codex-tui/automated-qualification/v3"
PROTOCOL_SCHEMA = "codex-tui/protocol-fixtures/v1"
COMPLETION_SCHEMA = "codex-tui/v1.2-completion/v1"
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
        description="Create exact-SHA v1.2 hosted development qualification."
    )
    parser.add_argument("--output", required=True)
    parser.add_argument("--plan", default="release/v1.2-plan.json")
    parser.add_argument("--completion", default="release/v1.2-completion.json")
    parser.add_argument("--automated-qualification", required=True)
    parser.add_argument(
        "--protocol-manifest", default="tests/fixtures/protocol/manifest.json"
    )
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()

    commit = args.commit.strip().lower()
    if not HEX40.fullmatch(commit):
        raise SystemExit("--commit must be exactly 40 hexadecimal characters")

    plan_path = pathlib.Path(args.plan)
    completion_path = pathlib.Path(args.completion)
    automated_path = pathlib.Path(args.automated_qualification)
    protocol_path = pathlib.Path(args.protocol_manifest)
    plan = load(plan_path)
    completion = load(completion_path)
    automated = load(automated_path)
    protocol = load(protocol_path)

    if plan.get("schema") != PLAN_SCHEMA:
        raise SystemExit(f"unexpected v1.2 plan schema: {plan.get('schema')!r}")
    if plan.get("targetVersion") != "1.2.0":
        raise SystemExit("development qualification requires targetVersion=1.2.0")
    if completion.get("schema") != COMPLETION_SCHEMA:
        raise SystemExit(
            f"unexpected v1.2 completion schema: {completion.get('schema')!r}"
        )
    if completion.get("targetVersion") != plan.get("targetVersion"):
        raise SystemExit("scope completion targetVersion drifted from v1.2 plan")
    if completion.get("status") != "development-scope-complete":
        raise SystemExit("scope completion status is not development-scope-complete")
    if completion.get("stableReady") is not False:
        raise SystemExit("scope completion must never claim stableReady")
    if completion.get("publicationAllowed") is not False:
        raise SystemExit("scope completion must never allow publication")

    if automated.get("schema") != AUTOMATED_SCHEMA:
        raise SystemExit(
            f"unexpected automated qualification schema: {automated.get('schema')!r}"
        )
    if str(automated.get("sourceSha", "")).lower() != commit:
        raise SystemExit("automated qualification source SHA mismatch")
    if protocol.get("schema") != PROTOCOL_SCHEMA:
        raise SystemExit(f"unexpected protocol fixture schema: {protocol.get('schema')!r}")

    required_gates = plan.get("automatedQualification", {}).get("requiredGates")
    gates = automated.get("gates")
    if not isinstance(required_gates, list) or not required_gates:
        raise SystemExit("v1.2 automated requiredGates is empty")
    if not isinstance(gates, dict):
        raise SystemExit("automated qualification gates are missing")
    failed = [gate for gate in required_gates if gates.get(gate) != "pass"]
    if failed:
        raise SystemExit(
            "automated qualification is not fully passing: " + ", ".join(failed)
        )

    ratchet = plan.get("moduleRatchet")
    if not isinstance(ratchet, dict) or not ratchet:
        raise SystemExit("v1.2 moduleRatchet is missing")
    modules = {}
    for name, ceiling in sorted(ratchet.items()):
        path = pathlib.Path(name)
        if not path.is_file():
            raise SystemExit(f"ratchet module is missing: {name}")
        lines = len(path.read_text(encoding="utf-8").splitlines())
        if lines > ceiling:
            raise SystemExit(
                f"module ratchet failed for {name}: {lines} LOC > {ceiling}"
            )
        modules[name] = {"lines": lines, "ceiling": ceiling}

    fixtures = protocol.get("fixtures")
    if not isinstance(fixtures, list) or len(fixtures) < 4:
        raise SystemExit("protocol replay manifest must retain at least four fixture classes")
    for fixture in fixtures:
        path = pathlib.Path(str(fixture.get("path", "")))
        if not path.is_file() or path.stat().st_size <= 0:
            raise SystemExit(f"protocol fixture is missing or empty: {path}")

    receipt = {
        "schema": SCHEMA,
        "targetVersion": plan["targetVersion"],
        "sourceSha": commit,
        "phase": plan.get("phase"),
        "observedAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "planSha256": sha256(plan_path),
        "scopeCompletion": {
            "status": "pass",
            "schema": completion["schema"],
            "manifestSha256": sha256(completion_path),
            "featureCompletionBaselineSha": completion["featureCompletionBaselineSha"],
            "stableReady": False,
            "publicationAllowed": False,
        },
        "repositoryAutomatedQualification": {
            "status": "pass",
            "schema": automated["schema"],
            "receiptSha256": sha256(automated_path),
            "gates": {gate: gates[gate] for gate in required_gates},
        },
        "moduleRatchet": {
            "status": "pass",
            "modules": modules,
        },
        "protocolReplay": {
            "status": "pass",
            "schema": protocol["schema"],
            "manifestSha256": sha256(protocol_path),
            "fixtureCount": len(fixtures),
        },
        "securityGovernance": {
            "status": "separate-authority",
            "workflow": "security",
            "releaseGateRechecks": True,
        },
        "stableReady": False,
        "publicationAllowed": False,
        "authority": "hosted-development-only",
        "policy": (
            "This receipt proves repository-hosted v1.2 development qualification only. "
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
