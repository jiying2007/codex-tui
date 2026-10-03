#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re

PLAN_SCHEMA = "codex-tui/v1.3-plan/v1"
COMPLETION_SCHEMA = "codex-tui/v1.3-completion/v1"
CRITERIA_SCHEMA = "codex-tui/stable-criteria/v2"
HEX40 = re.compile(r"^[0-9a-f]{40}$")

EXPECTED_PRIORITY_GROUPS = {
    "P0": ["transcript-search", "thread-queue"],
    "P1": ["remote-app-server-targets", "github-safe-mutations"],
}

SOURCE_TOKENS = {
    "src/transcript_search.rs": [
        "TranscriptSearchResults",
        "parse_thread_search",
        "parse_search_occurrences",
    ],
    "src/thread_queue.rs": [
        "ThreadQueueMutation",
        "parse_queue_list",
        "THREAD_QUEUE_TEXT_LIMIT",
    ],
    "src/app_server_target.rs": [
        "AppServerConfig",
        "ResolvedAppServerTarget",
        "auth_token_env",
    ],
    "src/app_server_transport.rs": [
        "AppServerTransport",
        "WebSocket",
        "UnixSocket",
    ],
    "src/forge_github_mutation.rs": [
        "GitHubPreflight",
        "revalidate_head",
        "commit_id",
    ],
    "src/forge_mutation.rs": [
        "ForgeProviderKind::GitHub",
        "OutcomeUnknown",
    ],
    ".github/workflows/development-qualification.yml": [
        "validate_v1_3_completion.py",
        "release/v1.3-completion.json",
    ],
    ".github/workflows/release.yml": [
        "validate_v1_3_completion.py",
        "release/v1.3-completion.json",
        "release/v1.3-plan.json",
    ],
}

def load(path):
    return json.loads(path.read_text(encoding="utf-8"))

def require_tokens(path, tokens):
    text = path.read_text(encoding="utf-8")
    missing = [token for token in tokens if token not in text]
    if missing:
        raise SystemExit(
            "{} is missing completion contract tokens: {}".format(
                path, ", ".join(repr(token) for token in missing)
            )
        )

def priority_groups(plan):
    groups = {"P0": [], "P1": []}
    for item in plan.get("priorities", []):
        if not isinstance(item, dict):
            raise SystemExit("v1.3 priority entry must be an object")
        priority = item.get("priority")
        identifier = item.get("id")
        if priority not in groups or not isinstance(identifier, str) or not identifier:
            raise SystemExit("unexpected v1.3 priority entry: {!r}".format(item))
        groups[priority].append(identifier)
    return groups

def main():
    parser = argparse.ArgumentParser(
        description="Validate v1.3 development-scope completion without claiming stable readiness."
    )
    parser.add_argument("--plan", default="release/v1.3-plan.json")
    parser.add_argument("--completion", default="release/v1.3-completion.json")
    parser.add_argument("--criteria", default="release/v1.3-criteria.json")
    args = parser.parse_args()

    plan_path = pathlib.Path(args.plan)
    completion_path = pathlib.Path(args.completion)
    criteria_path = pathlib.Path(args.criteria)
    plan = load(plan_path)
    completion = load(completion_path)
    criteria = load(criteria_path)

    if plan.get("schema") != PLAN_SCHEMA:
        raise SystemExit("unexpected v1.3 plan schema: {!r}".format(plan.get("schema")))
    if completion.get("schema") != COMPLETION_SCHEMA:
        raise SystemExit("unexpected v1.3 completion schema: {!r}".format(completion.get("schema")))
    if criteria.get("schema") != CRITERIA_SCHEMA:
        raise SystemExit("unexpected v1.3 stable criteria schema: {!r}".format(criteria.get("schema")))

    if plan.get("targetVersion") != "1.3.0":
        raise SystemExit("v1.3 plan targetVersion must be 1.3.0")
    if plan.get("versionActivation", {}).get("finalPackageVersion") != "1.3.0":
        raise SystemExit("v1.3 final package version drifted")
    if completion.get("targetVersion") != plan.get("targetVersion"):
        raise SystemExit("completion targetVersion drifted from v1.3 plan")
    if criteria.get("stableVersion") != plan.get("targetVersion"):
        raise SystemExit("stable criteria version drifted from v1.3 plan")
    if completion.get("planSchema") != PLAN_SCHEMA:
        raise SystemExit("completion planSchema drifted")

    cargo = pathlib.Path("Cargo.toml").read_text(encoding="utf-8")
    if 'version = "1.3.0"' not in cargo.split("[dependencies]", 1)[0]:
        raise SystemExit("Cargo package version is not activated at 1.3.0")

    baseline = str(completion.get("featureCompletionBaselineSha", "")).lower()
    if not HEX40.fullmatch(baseline):
        raise SystemExit("featureCompletionBaselineSha must be exactly 40 hexadecimal characters")
    if completion.get("status") != "development-scope-complete":
        raise SystemExit("v1.3 completion status must remain development-scope-complete")
    if completion.get("stableReady") is not False:
        raise SystemExit("completion manifest must never claim stableReady")
    if completion.get("publicationAllowed") is not False:
        raise SystemExit("completion manifest must never allow publication")

    groups = priority_groups(plan)
    if groups != EXPECTED_PRIORITY_GROUPS:
        raise SystemExit("v1.3 priority groups drifted: {!r}".format(groups))
    if completion.get("completedPriorities") != groups:
        raise SystemExit("completion manifest does not cover every v1.3 priority exactly")

    all_ids = [item for group in groups.values() for item in group]
    evidence = completion.get("evidence")
    if not isinstance(evidence, dict) or set(evidence) != set(all_ids):
        raise SystemExit("completion evidence keys must exactly match v1.3 priority IDs")
    for identifier in all_ids:
        item = evidence.get(identifier)
        paths = item.get("paths") if isinstance(item, dict) else None
        if not isinstance(paths, list) or not paths:
            raise SystemExit("completion evidence paths missing for {}".format(identifier))
        for raw_path in paths:
            path = pathlib.Path(raw_path)
            if not path.is_file():
                raise SystemExit("completion evidence path is missing: {}".format(path))

    freeze = completion.get("freezePolicy")
    if not isinstance(freeze, dict):
        raise SystemExit("completion freezePolicy is missing")
    if freeze.get("newCoreFunctionality") != "requires-new-development-plan":
        raise SystemExit("new v1.3 core functionality must require a new development plan")
    expected_change_classes = [
        "defect-fix",
        "security",
        "compatibility",
        "qualification-evidence",
        "release-tooling",
        "documentation",
    ]
    if freeze.get("allowedChangeClasses") != expected_change_classes:
        raise SystemExit("v1.3 freeze allowedChangeClasses drifted")

    if completion.get("frozenNonGoals") != plan.get("nonGoals"):
        raise SystemExit("completion frozenNonGoals drifted from v1.3 plan")
    if completion.get("deferredUntilEvidence") != criteria.get("deferredUntilEvidence"):
        raise SystemExit("completion deferredUntilEvidence drifted from stable criteria")
    if completion.get("frozenOptionalLayers") != criteria.get("frozenOptionalLayers"):
        raise SystemExit("completion frozenOptionalLayers drifted from stable criteria")
    if completion.get("stableAuthority") != "release/v1.3-criteria.json":
        raise SystemExit("completion stable authority must be release/v1.3-criteria.json")
    if "release-evidence/v4" not in str(completion.get("stableEvidencePolicy", "")):
        raise SystemExit("completion must retain release-evidence/v4 stable authority")

    dev = criteria.get("developmentQualification")
    if not isinstance(dev, dict):
        raise SystemExit("stable criteria developmentQualification is missing")
    if dev.get("planSchema") != PLAN_SCHEMA:
        raise SystemExit("stable criteria development plan schema drifted")
    if dev.get("scopeCompletionSchema") != COMPLETION_SCHEMA:
        raise SystemExit("stable criteria completion schema drifted")
    if dev.get("stableAuthority") is not False:
        raise SystemExit("hosted development qualification must not become stable authority")
    expected_checks = [
        "automated-qualification",
        "module-ratchet",
        "protocol-replay",
        "scope-completion",
    ]
    if dev.get("requiredChecks") != expected_checks:
        raise SystemExit("v1.3 stable criteria development checks drifted")

    required_gate_ids = {
        gate.get("id")
        for gate in criteria.get("requiredGates", [])
        if isinstance(gate, dict)
    }
    for gate in {
        "canonical-ci",
        "compatibility",
        "terminal-restoration",
        "performance-diagnostics",
        "archive-smoke",
        "checksums",
        "dependency-security",
        "architecture-ratchet",
        "protocol-replay",
    }:
        if gate not in required_gate_ids:
            raise SystemExit("stable criteria lost required gate: {}".format(gate))

    for raw_path, tokens in SOURCE_TOKENS.items():
        path = pathlib.Path(raw_path)
        if not path.is_file():
            raise SystemExit("completion contract source is missing: {}".format(path))
        require_tokens(path, tokens)

    print(
        "VALID v1.3 completion: P0/P1 development scope closed; "
        "stableReady=false; publicationAllowed=false; stable evidence remains separate"
    )
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
