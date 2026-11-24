#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import re
import sys

from check_module_ratchet import inspect_plan

SCHEMA = "codex-tui/upstream-convergence/v2"
RATCHET_AUTHORITY = "release/v1.4-plan.json"
REQUIRED_ROLE = "thin-local-control-plane"
REQUIRED_DEFAULT_SURFACE = "mission-control"
REQUIRED_DIFFERENTIATION = {
    "cross-repository-mission-control",
    "cross-target-codex-registry",
    "attention-routing",
    "derived-board-projection",
    "gitlab-github-forge-projection",
    "safe-forge-mutations",
    "saved-views",
    "doctor-headless-support-bundle",
}
REQUIRED_MAINTENANCE_CAPABILITIES = {
    "conversation-client",
    "thread-queue-ui",
    "embedded-terminal",
    "managed-worktree-operations",
}
REQUIRED_FROZEN_DUPLICATES = {
    "local-agent-orchestrator",
    "local-agent-delegation-runtime",
    "local-agent-to-agent-messaging",
    "independent-task-authority",
    "independent-thread-queue-authority",
    "independent-conversation-protocol",
    "independent-terminal-manager",
    "generic-coding-agent-provider-layer",
    "independent-managed-worktree-authority",
}

# These are exact observed source boundaries, not user-extensible selectors.
# A weakened prefix list must not silently exempt a duplicate local subsystem.
REQUIRED_PREFIXES = {
    "conversation-client": [
        "src/runtime_prompt",
        "src/runtime_user_response",
        "src/user_response",
        "src/transcript_search",
        "src/ui/thread",
        "src/app/prompt",
        "src/app/user_input",
        "src/app/user_response",
        "src/conversation",
        "src/app/thread_navigation",
        "src/app/reading_snapshot",
        "src/app/request_details",
    ],
    "thread-queue-ui": [
        "src/thread_queue",
        "src/app/queue",
        "src/app/thread_queue",
        "src/app_server/queue",
    ],
    "embedded-terminal": ["src/terminal", "src/ui/terminal", "src/pty"],
    "managed-worktree-operations": ["src/worktree", "src/operation", "src/runtime_commands"],
}
REQUIRED_CAPABILITY_POLICIES = {
    "conversation-client": "bugfix-compatibility-security-only",
    "thread-queue-ui": "retired-upstream-only",
    "embedded-terminal": "bugfix-compatibility-security-only",
    "managed-worktree-operations": "upstream-projection-and-git-safety-only",
}
REQUIRED_AUTHORITIES = {
    "codex": [
        "conversation",
        "agent-runtime",
        "agent-orchestration",
        "thread-lifecycle-semantics",
        "thread-queue",
    ],
    "git": ["repository-state", "worktree-state"],
    "forge": ["delivery", "change-review", "pipeline-state"],
    "shell": ["terminal-session"],
}
REQUIRED_CHANGE_POLICY = {
    "maintenanceOnlyGrowth": "requires-reviewed-shared-module-ratchet-and-convergence-identity-update",
    "newProductCapability": "must-strengthen-control-plane-differentiation-or-replace-local-duplication",
    "upstreamOverlap": "prefer-delete-reduce-or-project",
    "sourceOfTruth": "never-create-second-authority",
}

UPSTREAM_SNAPSHOT_SCHEMA = "codex-tui/upstream-snapshot/v1"
REQUIRED_UPSTREAM_REPOSITORY = "openai/codex"
REQUIRED_UPSTREAM_APP_SERVER_STATUS = "experimental-not-l3-qualified"
REQUIRED_UPSTREAM_SIGNALS = {
    "tui-workspace-default",
    "native-subagent-transcripts",
    "native-document-image-previews",
    "codex-agents-stable",
    "codex-queue-stable",
    "attached-thread-relations",
    "remote-cli-operations",
}
REQUIRED_UPSTREAM_DECISION = (
    "mission-control-only; no duplicated conversation, agent, queue or terminal authority"
)


def validate_upstream_snapshot(manifest, failures):
    snapshot = manifest.get("upstreamSnapshot")
    if not isinstance(snapshot, dict):
        failures.append("upstreamSnapshot must pin the current Codex CLI/App Server review")
        return
    if snapshot.get("schema") != UPSTREAM_SNAPSHOT_SCHEMA:
        failures.append("upstreamSnapshot schema drifted")
    if snapshot.get("repository") != REQUIRED_UPSTREAM_REPOSITORY:
        failures.append("upstreamSnapshot repository must remain openai/codex")
    version = snapshot.get("cliVersion")
    if not isinstance(version, str) or not re.fullmatch(r"0\.[0-9]+\.[0-9]+", version):
        failures.append("upstreamSnapshot cliVersion must be an exact stable 0.x.y version")
    elif snapshot.get("releaseTag") != "rust-v" + version:
        failures.append("upstreamSnapshot releaseTag must match the exact CLI version")
    if not isinstance(snapshot.get("releaseDate"), str) or not re.fullmatch(
        r"20[0-9]{2}-[0-9]{2}-[0-9]{2}", snapshot.get("releaseDate", "")
    ):
        failures.append("upstreamSnapshot releaseDate must be an ISO date")
    if not isinstance(snapshot.get("releaseCommit"), str) or not re.fullmatch(
        r"[0-9a-f]{40}", snapshot.get("releaseCommit", "")
    ):
        failures.append("upstreamSnapshot releaseCommit must be an exact lowercase Git SHA")
    if (
        not isinstance(snapshot.get("protocolAssessmentPath"), str)
        or not snapshot["protocolAssessmentPath"].startswith("tests/fixtures/protocol/")
        or not snapshot["protocolAssessmentPath"].endswith("/assessment.json")
    ):
        failures.append("upstreamSnapshot must point to a retained protocol assessment")
    if snapshot.get("appServerStatus") != REQUIRED_UPSTREAM_APP_SERVER_STATUS:
        failures.append("upstream App Server must not be claimed as L3-qualified")
    signals = snapshot.get("overlapSignals")
    if (
        not isinstance(signals, list)
        or any(not isinstance(signal, str) for signal in signals)
        or len(signals) != len(set(signals))
        or set(signals) != REQUIRED_UPSTREAM_SIGNALS
    ):
        failures.append("upstream overlap evidence set drifted")
    if snapshot.get("productDecision") != REQUIRED_UPSTREAM_DECISION:
        failures.append("upstream product decision must preserve Mission Control-only authority")


def matches_prefix(path: str, prefix: str) -> bool:
    return (
        path == prefix + ".rs"
        or path.startswith(prefix + "/")
        or path.startswith(prefix + "_")
    )


def main() -> int:
    manifest_path = pathlib.Path(
        sys.argv[1] if len(sys.argv) > 1 else "release/v1.5-convergence.json"
    )
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))

    failures = []
    if manifest.get("schema") != SCHEMA:
        failures.append("unexpected convergence schema")
    if manifest.get("status") != "active":
        failures.append("convergence policy must remain active")
    if manifest.get("productRole") != REQUIRED_ROLE:
        failures.append("productRole must remain thin-local-control-plane")
    if manifest.get("defaultSurface") != REQUIRED_DEFAULT_SURFACE:
        failures.append("Mission Control must remain the default product surface")
    if manifest.get("upstreamFirst") is not True:
        failures.append("upstreamFirst must remain true")
    validate_upstream_snapshot(manifest, failures)
    if manifest.get("authorities") != REQUIRED_AUTHORITIES:
        failures.append("upstream authorities drifted; do not establish competing local state")
    if manifest.get("moduleRatchetAuthority") != RATCHET_AUTHORITY:
        failures.append("shared module ratchet authority drifted")

    # Reuse the already enforced v1.4 module-ratchet implementation and LOC
    # ceilings: convergence only owns capability identity, never duplicate limits.
    governed = set()
    try:
        rows, ratchet_failures, complete = inspect_plan(pathlib.Path(RATCHET_AUTHORITY))
    except (OSError, ValueError, json.JSONDecodeError) as error:
        failures.append(f"shared module ratchet unavailable: {error}")
    else:
        governed = {path for path, _, _ in rows}
        if not complete:
            failures.append("shared module ratchet lost complete source coverage")
        failures.extend(f"shared module ratchet: {message}" for message in ratchet_failures)

    differentiation = manifest.get("differentiation")
    if not isinstance(differentiation, list) or set(differentiation) != REQUIRED_DIFFERENTIATION:
        failures.append("control-plane differentiation set drifted")

    frozen = manifest.get("frozenDuplicateCapabilities")
    if not isinstance(frozen, list) or set(frozen) != REQUIRED_FROZEN_DUPLICATES:
        failures.append("frozen duplicate capability set drifted")

    maintenance = manifest.get("maintenanceOnlyCapabilities")
    if not isinstance(maintenance, dict) or set(maintenance) != REQUIRED_MAINTENANCE_CAPABILITIES:
        failures.append("maintenance-only capability set drifted")
        maintenance = {}

    source_files = sorted(
        path.relative_to(pathlib.Path(".")).as_posix()
        for path in pathlib.Path("src").rglob("*.rs")
        if path.is_file()
    )

    for capability, policy in sorted(maintenance.items()):
        if not isinstance(policy, dict):
            failures.append(f"{capability}: policy must be an object")
            continue
        prefixes = policy.get("modulePrefixes")
        modules = policy.get("modules")
        if prefixes != REQUIRED_PREFIXES[capability]:
            failures.append(f"{capability}: guarded source prefixes drifted")
            continue
        if policy.get("policy") != REQUIRED_CAPABILITY_POLICIES[capability]:
            failures.append(f"{capability}: maintenance-only policy drifted")
        if (
            not isinstance(modules, list)
            or (not modules and capability != "thread-queue-ui")
            or (modules and capability == "thread-queue-ui")
            or any(not isinstance(name, str) or not name for name in modules)
            or len(modules) != len(set(modules))
        ):
            failures.append(f"{capability}: modules must be distinct source paths")
            continue

        discovered = {
            path
            for path in source_files
            if any(matches_prefix(path, prefix) for prefix in prefixes)
        }
        tracked = set(modules)
        missing = sorted(discovered - tracked)
        stale = sorted(tracked - discovered)
        for path in missing:
            failures.append(
                f"{capability}: new maintenance-only source {path} is untracked; "
                "update the convergence manifest explicitly before growing this surface"
            )
        for path in stale:
            failures.append(f"{capability}: tracked maintenance-only source {path} is missing")

        for path in sorted(tracked):
            if path not in governed:
                failures.append(
                    f"{capability}: {path} is not governed by the shared v1.4 module ratchet"
                )
            else:
                print(f"{capability}: {path}: shared v1.4 module ratchet")


    if manifest.get("changePolicy") != REQUIRED_CHANGE_POLICY:
        failures.append("changePolicy drifted or lost fail-closed controls")

    if failures:
        raise SystemExit("upstream convergence guard failed:\n" + "\n".join(failures))

    print("PASS upstream convergence guard: Mission Control control-plane role retained")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
