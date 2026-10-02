#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib

PLAN_SCHEMA = "codex-tui/v1.2-plan/v1"
QUALIFICATION_SCHEMA = "codex-tui/development-qualification/v1"


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate the v1.2 development plan and qualification authority."
    )
    parser.add_argument("--plan", default="release/v1.2-plan.json")
    parser.add_argument("--criteria", default="release/v1.2-criteria.json")
    parser.add_argument(
        "--workflow", default=".github/workflows/development-qualification.yml"
    )
    args = parser.parse_args()

    plan_path = pathlib.Path(args.plan)
    criteria_path = pathlib.Path(args.criteria)
    workflow_path = pathlib.Path(args.workflow)
    plan = load(plan_path)
    criteria = load(criteria_path)

    if plan.get("schema") != PLAN_SCHEMA:
        raise SystemExit(f"unexpected v1.2 plan schema: {plan.get('schema')!r}")
    if plan.get("targetVersion") != "1.2.0":
        raise SystemExit("v1.2 plan targetVersion must be 1.2.0")
    if criteria.get("stableVersion") != plan.get("targetVersion"):
        raise SystemExit("v1.2 plan targetVersion does not match stable criteria")
    if plan.get("phase") != "maintainability-and-terminal-state-completion":
        raise SystemExit("unexpected v1.2 development phase")

    ratchet = plan.get("moduleRatchet")
    if not isinstance(ratchet, dict) or not ratchet:
        raise SystemExit("v1.2 moduleRatchet is missing")
    for name, ceiling in ratchet.items():
        if not isinstance(name, str) or not name:
            raise SystemExit("invalid module ratchet path")
        if not isinstance(ceiling, int) or ceiling <= 0:
            raise SystemExit(f"invalid module ratchet ceiling for {name}: {ceiling!r}")

    automated = plan.get("automatedQualification")
    expected = criteria.get("automatedQualification")
    if not isinstance(automated, dict) or not isinstance(expected, dict):
        raise SystemExit("v1.2 automated qualification contract is missing")
    if automated.get("schema") != expected.get("schema"):
        raise SystemExit("v1.2 automated qualification schema drifted")
    if automated.get("exactSourceSha") is not True:
        raise SystemExit("v1.2 automated qualification must be exact-SHA")
    if automated.get("requiredOnEveryMainSha") is not True:
        raise SystemExit("v1.2 automated qualification must run for every main SHA")
    if automated.get("requiredGates") != expected.get("requiredGates"):
        raise SystemExit("v1.2 automated gates drifted from stable criteria")

    qualification = plan.get("developmentQualification")
    if not isinstance(qualification, dict):
        raise SystemExit("v1.2 developmentQualification is missing")
    if qualification.get("schema") != QUALIFICATION_SCHEMA:
        raise SystemExit("unexpected v1.2 development qualification schema")
    if qualification.get("generatedForEveryMainSha") is not True:
        raise SystemExit("development qualification must run for every main SHA")
    if qualification.get("authority") != "hosted-development-only":
        raise SystemExit("development qualification authority must remain hosted-development-only")
    if qualification.get("canSatisfyStable") is not False:
        raise SystemExit("development qualification must never satisfy stable publication")
    if qualification.get("artifactName") != "development-qualification":
        raise SystemExit("development qualification artifact name drifted")
    if qualification.get("requiredChecks") != [
        "automated-qualification",
        "module-ratchet",
        "protocol-replay",
    ]:
        raise SystemExit("development qualification required checks drifted")

    p0_ids = {
        item.get("id")
        for item in plan.get("priorities", [])
        if isinstance(item, dict) and item.get("priority") == "P0"
    }
    if p0_ids != {
        "architecture-decomposition",
        "replay-compatibility",
        "dependency-security-governance",
    }:
        raise SystemExit("v1.2 P0 plan drifted")

    deferred = plan.get("evidenceDrivenDeferred")
    frozen = plan.get("frozenOptionalLayers")
    if not isinstance(deferred, list) or not deferred:
        raise SystemExit("v1.2 evidenceDrivenDeferred is missing")
    if not isinstance(frozen, list) or not frozen:
        raise SystemExit("v1.2 frozenOptionalLayers is missing")
    overlap = set(deferred).intersection(frozen)
    if overlap:
        raise SystemExit("v1.2 deferred/frozen layers overlap: " + ", ".join(sorted(overlap)))

    workflow = workflow_path.read_text(encoding="utf-8")
    required_tokens = (
        "name: development-qualification",
        "branches: [main]",
        "validate_v1_2_plan.py",
        "check_module_ratchet.py",
        "protocol_replay",
        "create_development_qualification.py",
        "development-qualification",
        "stableReady",
        "publicationAllowed",
    )
    missing = [token for token in required_tokens if token not in workflow]
    if missing:
        raise SystemExit(
            "development qualification workflow contract missing: " + ", ".join(missing)
        )

    print(
        "VALID v1.2 plan: exact-SHA hosted development qualification; "
        "stable release authority remains separate"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
