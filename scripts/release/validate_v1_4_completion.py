#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re

PLAN_SCHEMA = "codex-tui/v1.4-plan/v1"
COMPLETION_SCHEMA = "codex-tui/v1.4-completion/v1"
CRITERIA_SCHEMA = "codex-tui/stable-criteria/v2"
HEX40 = re.compile(r"^[0-9a-f]{40}$")

EXPECTED_PRIORITY_GROUPS = {
    "P0": [
        "board-large-dataset-navigation",
        "workcard-relationship-closure",
        "codex-thread-lifecycle-handoff",
        "unified-metadata-search",
    ],
    "P1": [
        "saved-view-editor",
        "review-evidence-and-external-open",
        "long-thread-viewport-cache",
        "fuzzy-command-palette",
    ],
    "P2": [
        "core-module-decomposition",
        "user-perceived-performance-evidence",
    ],
}

SOURCE_TOKENS = {
    "src/render_performance.rs": [
        "RENDER_PERFORMANCE_SCHEMA",
        "BOARD_RENDER_FIXTURE",
        "THREAD_RENDER_FIXTURE",
        "RENDER_MIN_ITERATIONS",
    ],
    ".github/workflows/performance-diagnostics.yml": [
        "release render-benchmark",
        "user-perceived-performance-${{ github.sha }}",
        "validate_diagnostics.py render",
    ],
    "scripts/release/validate_diagnostics.py": [
        "codex-tui/render-performance/v1",
        "board-render-10k",
        "thread-render-10k",
        "sourceSha",
        "math.isfinite",
    ],
    ".github/workflows/development-qualification.yml": [
        "validate_v1_4_completion.py",
        "release/v1.4-completion.json",
        "release/v1.4-criteria.json",
    ],
    ".github/workflows/release.yml": [
        "validate_v1_4_completion.py",
        "release/v1.4-completion.json",
        "release/v1.4-plan.json",
    ],
}


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def require_tokens(path: pathlib.Path, tokens: list[str]) -> None:
    text = path.read_text(encoding="utf-8")
    missing = [token for token in tokens if token not in text]
    if missing:
        raise SystemExit(
            "{} is missing completion contract tokens: {}".format(
                path, ", ".join(repr(token) for token in missing)
            )
        )


def package_version() -> str:
    text = pathlib.Path("Cargo.toml").read_text(encoding="utf-8")
    prefix = text.split("[dependencies]", 1)[0]
    match = re.search(r'^version\s*=\s*"([^"]+)"', prefix, re.MULTILINE)
    if not match:
        raise SystemExit("Cargo package version is missing")
    return match.group(1)


def priority_groups(plan: dict) -> dict[str, list[str]]:
    groups = {"P0": [], "P1": [], "P2": []}
    for item in plan.get("priorities", []):
        if not isinstance(item, dict):
            raise SystemExit("v1.4 priority entry must be an object")
        priority = item.get("priority")
        identifier = item.get("id")
        if priority not in groups or not isinstance(identifier, str) or not identifier:
            raise SystemExit("unexpected v1.4 priority entry: {!r}".format(item))
        groups[priority].append(identifier)
    return groups


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate v1.4 development-scope completion without claiming stable readiness."
    )
    parser.add_argument("--plan", default="release/v1.4-plan.json")
    parser.add_argument("--completion", default="release/v1.4-completion.json")
    parser.add_argument("--criteria", default="release/v1.4-criteria.json")
    args = parser.parse_args()

    plan_path = pathlib.Path(args.plan)
    completion_path = pathlib.Path(args.completion)
    criteria_path = pathlib.Path(args.criteria)
    plan = load(plan_path)
    completion = load(completion_path)
    criteria = load(criteria_path)

    if plan.get("schema") != PLAN_SCHEMA:
        raise SystemExit("unexpected v1.4 plan schema: {!r}".format(plan.get("schema")))
    if completion.get("schema") != COMPLETION_SCHEMA:
        raise SystemExit(
            "unexpected v1.4 completion schema: {!r}".format(completion.get("schema"))
        )
    if criteria.get("schema") != CRITERIA_SCHEMA:
        raise SystemExit(
            "unexpected v1.4 stable criteria schema: {!r}".format(criteria.get("schema"))
        )

    if plan.get("targetVersion") != "1.4.0":
        raise SystemExit("v1.4 plan targetVersion must be 1.4.0")
    if package_version() != "1.4.0":
        raise SystemExit("Cargo package version is not activated at 1.4.0")
    if completion.get("targetVersion") != plan.get("targetVersion"):
        raise SystemExit("completion targetVersion drifted from v1.4 plan")
    if completion.get("planSchema") != PLAN_SCHEMA:
        raise SystemExit("completion planSchema drifted")
    if criteria.get("stableVersion") != plan.get("targetVersion"):
        raise SystemExit("stable criteria version drifted from v1.4 plan")

    baseline = str(completion.get("featureCompletionBaselineSha", "")).lower()
    if not HEX40.fullmatch(baseline):
        raise SystemExit("featureCompletionBaselineSha must be exactly 40 hexadecimal characters")
    if completion.get("status") != "development-scope-complete":
        raise SystemExit("v1.4 completion status must remain development-scope-complete")
    if completion.get("stableReady") is not False:
        raise SystemExit("completion manifest must never claim stableReady")
    if completion.get("publicationAllowed") is not False:
        raise SystemExit("completion manifest must never allow publication")

    groups = priority_groups(plan)
    if groups != EXPECTED_PRIORITY_GROUPS:
        raise SystemExit("v1.4 priority groups drifted: {!r}".format(groups))
    if completion.get("completedPriorities") != groups:
        raise SystemExit("completion manifest does not cover every v1.4 priority exactly")

    all_ids = [identifier for values in groups.values() for identifier in values]
    evidence = completion.get("evidence")
    if not isinstance(evidence, dict) or set(evidence) != set(all_ids):
        raise SystemExit("completion evidence keys must exactly match v1.4 priority IDs")
    for identifier in all_ids:
        item = evidence.get(identifier)
        paths = item.get("paths") if isinstance(item, dict) else None
        if not isinstance(paths, list) or not paths:
            raise SystemExit("completion evidence paths missing for {}".format(identifier))
        for raw_path in paths:
            path = pathlib.Path(raw_path)
            if not path.is_file():
                raise SystemExit("completion evidence path is missing: {}".format(path))

    expected_deferred = [
        "gitlab-issue-board-projection",
        "native-gitlab-rest-graphql",
        "extra-release-architectures",
    ]
    if completion.get("deferredUntilEvidence") != expected_deferred:
        raise SystemExit("v1.4 deferred evidence decisions drifted")
    if criteria.get("deferredUntilEvidence") != expected_deferred:
        raise SystemExit("v1.4 criteria deferred evidence decisions drifted")
    if completion.get("frozenNonGoals") != plan.get("nonGoals"):
        raise SystemExit("completion frozenNonGoals drifted from v1.4 plan")
    if completion.get("frozenOptionalLayers") != criteria.get("frozenOptionalLayers"):
        raise SystemExit("completion frozenOptionalLayers drifted from stable criteria")

    freeze = completion.get("freezePolicy")
    if not isinstance(freeze, dict):
        raise SystemExit("completion freezePolicy is missing")
    if freeze.get("newCoreFunctionality") != "requires-new-development-plan":
        raise SystemExit("new v1.4 core functionality must require a new development plan")
    expected_change_classes = [
        "defect-fix",
        "security",
        "compatibility",
        "qualification-evidence",
        "release-tooling",
        "documentation",
    ]
    if freeze.get("allowedChangeClasses") != expected_change_classes:
        raise SystemExit("v1.4 freeze allowedChangeClasses drifted")

    if completion.get("stableAuthority") != "release/v1.4-criteria.json":
        raise SystemExit("completion stable authority must be release/v1.4-criteria.json")
    if "release-evidence/v5" not in str(completion.get("stableEvidencePolicy", "")):
        raise SystemExit("completion must retain release-evidence/v5 stable authority")

    dev = criteria.get("developmentQualification")
    if not isinstance(dev, dict):
        raise SystemExit("stable criteria developmentQualification is missing")
    if dev.get("planSchema") != PLAN_SCHEMA:
        raise SystemExit("stable criteria development plan schema drifted")
    if dev.get("scopeCompletionSchema") != COMPLETION_SCHEMA:
        raise SystemExit("stable criteria completion schema drifted")
    if dev.get("stableAuthority") is not False:
        raise SystemExit("hosted development qualification must not become stable authority")

    render = criteria.get("performance", {}).get("userPerceivedRender")
    if not isinstance(render, dict):
        raise SystemExit("v1.4 criteria user-perceived render diagnostics are missing")
    if render.get("schema") != "codex-tui/render-performance/v1":
        raise SystemExit("v1.4 render performance schema drifted")
    if render.get("fixtures") != ["board-render-10k", "thread-render-10k"]:
        raise SystemExit("v1.4 render fixture set drifted")
    if render.get("rows") != 10000 or render.get("iterationsMin") != 200:
        raise SystemExit("v1.4 render evidence sample contract drifted")
    if render.get("sourceBound") is not True:
        raise SystemExit("v1.4 render evidence must remain exact-SHA source-bound")

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
        "immutable-releases",
        "main-protection-integrity",
        "prior-stable-dry-run",
        "stable-bundle-promotion",
        "release-asset-integrity",
        "release-ref-integrity",
        "published-release-state",
    }:
        if gate not in required_gate_ids:
            raise SystemExit("stable criteria lost required gate: {}".format(gate))

    publication = criteria.get("publicationGovernance")
    if not isinstance(publication, dict):
        raise SystemExit("v1.4 stable publicationGovernance is missing")
    if publication.get("schema") != "codex-tui/stable-publication-governance/v1":
        raise SystemExit("v1.4 stable publication governance schema drifted")
    for key in (
        "exactSourceSha",
        "exactQualifiedBundlePromotion",
        "remoteAssetDigestVerification",
        "immutableReleaseRequired",
        "successfulPublicationReceiptsRequired",
        "partialFailureEvidenceRetained",
        "phaseStateValidated",
        "assetReceiptsSourceBound",
        "releaseSnapshotsDigestBound",
        "immutableReleaseSnapshotsRetained",
        "immutableReleaseSnapshotsDigestBound",
        "mainProtectionSnapshotsRetained",
        "mainProtectionSnapshotsDigestBound",
        "mainProtectionCheckRunsSnapshotsDigestBound",
        "canonicalMainProtectionRequiredAtPublication",
        "canonicalRequiredChecksAppBound",
        "publishPointMainProtectionRevalidated",
        "releaseTargetSourceBound",
        "publishedTimestampRequired",
        "publishMainStateRetained",
        "tagRefSnapshotsRetained",
        "tagRefsSourceBound",
        "publishPointMainRevalidated",
    ):
        if publication.get(key) is not True:
            raise SystemExit("v1.4 stable publication governance lost {}".format(key))
    if publication.get("immutableReleaseReceiptSchema") != "codex-tui/immutable-releases/v2":
        raise SystemExit("v1.4 immutable-release receipt schema drifted")
    if publication.get("mainProtectionReceiptSchema") != "codex-tui/main-protection-state/v2":
        raise SystemExit("v1.4 main-protection receipt schema drifted")
    if publication.get("releaseAssetVerificationSchema") != "codex-tui/release-asset-verification/v2":
        raise SystemExit("v1.4 release asset verification schema drifted")
    if publication.get("releaseBranchStateSchema") != "codex-tui/release-branch-state/v1":
        raise SystemExit("v1.4 release branch-state schema drifted")
    if publication.get("releaseTagRefSchema") != "codex-tui/release-tag-ref/v1":
        raise SystemExit("v1.4 release tag-ref schema drifted")
    if publication.get("postPublicationRewriteAllowed") is not False:
        raise SystemExit("v1.4 stable publication must not permit post-publication rewrite")

    if set(plan.get("evidenceGatedDecisions", {})) != {"gitlab-issue-board-projection"}:
        raise SystemExit("unexpected v1.4 evidence-gated decision set")

    for raw_path, tokens in SOURCE_TOKENS.items():
        path = pathlib.Path(raw_path)
        if not path.is_file():
            raise SystemExit("completion contract source is missing: {}".format(path))
        require_tokens(path, tokens)

    print(
        "VALID v1.4 completion: P0/P1/P2 development scope closed; "
        "stableReady=false; publicationAllowed=false; stable evidence remains separate"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
