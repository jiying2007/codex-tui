#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib


PLAN_SCHEMA = "codex-tui/rc-plan/v1"


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate the v1.1 RC freeze/handoff contract against stable criteria."
    )
    parser.add_argument("--plan", default="release/v1.1-rc-plan.json")
    parser.add_argument("--criteria", default="release/v1.1-criteria.json")
    args = parser.parse_args()

    plan_path = pathlib.Path(args.plan)
    criteria_path = pathlib.Path(args.criteria)
    plan = load(plan_path)
    criteria = load(criteria_path)

    if plan.get("schema") != PLAN_SCHEMA:
        raise SystemExit(f"unexpected RC plan schema: {plan.get('schema')!r}")
    if plan.get("targetVersion") != criteria.get("stableVersion"):
        raise SystemExit("RC targetVersion does not match stable criteria")

    candidate = plan.get("candidatePolicy")
    if not isinstance(candidate, dict) or candidate.get("codeFreeze") is not True:
        raise SystemExit("RC plan must explicitly freeze product code")
    if candidate.get("candidateInvalidatedByProductCodeChange") is not True:
        raise SystemExit("product-code changes must invalidate the current RC candidate")
    if candidate.get("publicationRequiresFreshEvidenceForCandidateSha") is not True:
        raise SystemExit("publication must require fresh evidence for the candidate SHA")

    automated = plan.get("automatedQualification")
    expected_automated = criteria.get("automatedQualification")
    if not isinstance(automated, dict) or not isinstance(expected_automated, dict):
        raise SystemExit("missing automated qualification contract")
    if automated.get("schema") != expected_automated.get("schema"):
        raise SystemExit("automated qualification schema mismatch")
    if automated.get("requiredOnEveryCandidateSha") is not True:
        raise SystemExit("automated qualification must run on every candidate SHA")
    if automated.get("requiredGates") != expected_automated.get("requiredGates"):
        raise SystemExit("RC automated gates drifted from stable criteria")

    criteria_gates = {
        gate.get("id")
        for gate in criteria.get("requiredGates", [])
        if isinstance(gate, dict)
    }
    evidence = plan.get("deferredRealEvidence")
    if not isinstance(evidence, list) or not evidence:
        raise SystemExit("RC plan must list deferred real-environment evidence")

    by_id = {}
    for item in evidence:
        if not isinstance(item, dict) or not item.get("id"):
            raise SystemExit("invalid deferred evidence item")
        if item["id"] in by_id:
            raise SystemExit(f"duplicate deferred evidence id: {item['id']}")
        if item.get("canBeSynthesized") is not False:
            raise SystemExit(f"{item['id']} must explicitly forbid synthesized evidence")
        gate = item.get("criteriaGate")
        if gate is not None and gate not in criteria_gates:
            raise SystemExit(f"{item['id']} references unknown criteria gate {gate!r}")
        by_id[item["id"]] = item

    required_stable = {
        item["id"] for item in evidence if item.get("requiredForStable") is True
    }
    if required_stable != {"linux-compatibility", "linux-terminal-restoration"}:
        raise SystemExit(
            "stable real-environment blockers must be exactly Linux compatibility and terminal restoration"
        )

    gitlab = by_id.get("internal-gitlab-provider")
    if not isinstance(gitlab, dict):
        raise SystemExit("internal GitLab provider qualification must remain explicit")
    if gitlab.get("requiredForStable") is not False:
        raise SystemExit("internal GitLab provider evidence must not block personal-first stable release")
    if gitlab.get("requiredForTeamReuseQualification") is not True:
        raise SystemExit("internal GitLab provider evidence must gate team-reuse qualification")

    frozen = plan.get("frozenNonGoals")
    if frozen != criteria.get("frozenNonGoals"):
        raise SystemExit("RC frozen non-goals drifted from stable criteria")

    resume = plan.get("resumeSequence")
    if not isinstance(resume, list) or len(resume) < 5:
        raise SystemExit("RC resume sequence is incomplete")

    print(
        "VALID RC plan "
        f"{plan['targetVersion']}: automated gates bound; "
        "stable real evidence deferred without synthesis"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
