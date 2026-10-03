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
V12_PLAN_SCHEMA = "codex-tui/v1.2-plan/v1"
V13_PLAN_SCHEMA = "codex-tui/v1.3-plan/v1"
AUTOMATED_SCHEMA = "codex-tui/automated-qualification/v3"
PROTOCOL_SCHEMA = "codex-tui/protocol-fixtures/v1"
V12_COMPLETION_SCHEMA = "codex-tui/v1.2-completion/v1"
V13_COMPLETION_SCHEMA = "codex-tui/v1.3-completion/v1"
HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def default_plan_path() -> pathlib.Path:
    v13 = pathlib.Path("release/v1.3-plan.json")
    return v13 if v13.is_file() else pathlib.Path("release/v1.2-plan.json")


def default_completion_path(plan: dict) -> pathlib.Path:
    if plan.get("targetVersion") == "1.3.0":
        v13 = pathlib.Path("release/v1.3-completion.json")
        if v13.is_file():
            return v13
    return pathlib.Path("release/v1.2-completion.json")


def validate_plan(plan: dict) -> None:
    schema = plan.get("schema")
    target = plan.get("targetVersion")
    if schema == V12_PLAN_SCHEMA:
        if target != "1.2.0":
            raise SystemExit("v1.2 development plan requires targetVersion=1.2.0")
    elif schema == V13_PLAN_SCHEMA:
        if target != "1.3.0":
            raise SystemExit("v1.3 development plan requires targetVersion=1.3.0")
    else:
        raise SystemExit("unexpected development plan schema: {!r}".format(schema))


def scope_status(plan: dict, completion: dict) -> str:
    plan_target = plan.get("targetVersion")
    completion_target = completion.get("targetVersion")
    completion_schema = completion.get("schema")

    if plan_target == completion_target:
        expected = (
            V13_COMPLETION_SCHEMA
            if plan_target == "1.3.0"
            else V12_COMPLETION_SCHEMA
        )
        if completion_schema != expected:
            raise SystemExit(
                "completion schema {!r} does not match active target {}".format(
                    completion_schema, plan_target
                )
            )
        if completion.get("status") != "development-scope-complete":
            raise SystemExit("active completion status is not development-scope-complete")
        return "pass"

    if plan_target == "1.3.0" and completion_target == "1.2.0":
        if completion_schema != V12_COMPLETION_SCHEMA:
            raise SystemExit("v1.3 predecessor must be the retained v1.2 completion")
        if completion.get("status") != "development-scope-complete":
            raise SystemExit("v1.2 predecessor completion is not complete")
        return "in-progress"

    raise SystemExit(
        "completion target {!r} is not valid for active target {!r}".format(
            completion_target, plan_target
        )
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Create exact-SHA hosted development qualification for the active "
            "codex-tui development plan."
        )
    )
    parser.add_argument("--output", required=True)
    parser.add_argument("--plan", default="")
    parser.add_argument("--completion", default="")
    parser.add_argument("--automated-qualification", required=True)
    parser.add_argument(
        "--protocol-manifest", default="tests/fixtures/protocol/manifest.json"
    )
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()

    commit = args.commit.strip().lower()
    if not HEX40.fullmatch(commit):
        raise SystemExit("--commit must be exactly 40 hexadecimal characters")

    plan_path = pathlib.Path(args.plan) if args.plan else default_plan_path()
    plan = load(plan_path)
    validate_plan(plan)

    completion_path = (
        pathlib.Path(args.completion)
        if args.completion
        else default_completion_path(plan)
    )
    automated_path = pathlib.Path(args.automated_qualification)
    protocol_path = pathlib.Path(args.protocol_manifest)
    completion = load(completion_path)
    automated = load(automated_path)
    protocol = load(protocol_path)
    active_scope_status = scope_status(plan, completion)

    if completion.get("stableReady") is not False:
        raise SystemExit("development completion/predecessor must never claim stableReady")
    if completion.get("publicationAllowed") is not False:
        raise SystemExit(
            "development completion/predecessor must never allow publication"
        )

    if automated.get("schema") != AUTOMATED_SCHEMA:
        raise SystemExit(
            "unexpected automated qualification schema: {!r}".format(
                automated.get("schema")
            )
        )
    if str(automated.get("sourceSha", "")).lower() != commit:
        raise SystemExit("automated qualification source SHA mismatch")
    if protocol.get("schema") != PROTOCOL_SCHEMA:
        raise SystemExit(
            "unexpected protocol fixture schema: {!r}".format(protocol.get("schema"))
        )

    required_gates = plan.get("automatedQualification", {}).get("requiredGates")
    gates = automated.get("gates")
    if not isinstance(required_gates, list) or not required_gates:
        raise SystemExit("active development automated requiredGates is empty")
    if not isinstance(gates, dict):
        raise SystemExit("automated qualification gates are missing")
    failed = [gate for gate in required_gates if gates.get(gate) != "pass"]
    if failed:
        raise SystemExit(
            "automated qualification is not fully passing: " + ", ".join(failed)
        )

    ratchet = plan.get("moduleRatchet")
    if not isinstance(ratchet, dict) or not ratchet:
        raise SystemExit("active development moduleRatchet is missing")
    modules = {}
    for name, ceiling in sorted(ratchet.items()):
        path = pathlib.Path(name)
        if not path.is_file():
            raise SystemExit("ratchet module is missing: {}".format(name))
        lines = len(path.read_text(encoding="utf-8").splitlines())
        if lines > ceiling:
            raise SystemExit(
                "module ratchet failed for {}: {} LOC > {}".format(
                    name, lines, ceiling
                )
            )
        modules[name] = {"lines": lines, "ceiling": ceiling}

    fixtures = protocol.get("fixtures")
    if not isinstance(fixtures, list) or len(fixtures) < 4:
        raise SystemExit(
            "protocol replay manifest must retain at least four fixture classes"
        )
    for fixture in fixtures:
        path = pathlib.Path(str(fixture.get("path", "")))
        if not path.is_file() or path.stat().st_size <= 0:
            raise SystemExit("protocol fixture is missing or empty: {}".format(path))

    scope = {
        "status": active_scope_status,
        "schema": completion["schema"],
        "manifestSha256": sha256(completion_path),
        "targetVersion": completion.get("targetVersion"),
        "featureCompletionBaselineSha": completion.get("featureCompletionBaselineSha"),
        "stableReady": False,
        "publicationAllowed": False,
    }
    if active_scope_status == "in-progress":
        scope["authority"] = "predecessor-development-completion"
        scope["activeTargetVersion"] = plan["targetVersion"]
    else:
        scope["authority"] = "active-development-completion"

    receipt = {
        "schema": SCHEMA,
        "targetVersion": plan["targetVersion"],
        "sourceSha": commit,
        "phase": plan.get("phase"),
        "observedAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "planSchema": plan["schema"],
        "planSha256": sha256(plan_path),
        "scopeCompletion": scope,
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
            "This receipt proves repository-hosted development qualification only. "
            "An in-progress scope status means the active development plan is authorized "
            "and ratcheted but not complete. This receipt is never stable release authority."
        ),
    }

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    write_text_lf(output, json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print("WROTE {}".format(output))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
