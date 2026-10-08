#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import sys

SCHEMA = "codex-tui/upstream-convergence/v1"
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
    ],
    "thread-queue-ui": ["src/thread_queue", "src/app/queue"],
    "embedded-terminal": ["src/terminal", "src/ui/terminal", "src/pty"],
}
REQUIRED_CAPABILITY_POLICIES = {
    "conversation-client": "bugfix-compatibility-security-only",
    "thread-queue-ui": "upstream-projection-only",
    "embedded-terminal": "bugfix-compatibility-security-only",
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
    "maintenanceOnlyGrowth": "requires-explicit-manifest-ratchet-update-and-rationale",
    "newProductCapability": "must-strengthen-control-plane-differentiation-or-replace-local-duplication",
    "upstreamOverlap": "prefer-delete-reduce-or-project",
    "sourceOfTruth": "never-create-second-authority",
}


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
    if manifest.get("authorities") != REQUIRED_AUTHORITIES:
        failures.append("upstream authorities drifted; do not establish competing local state")

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
        if not isinstance(modules, dict) or not modules:
            failures.append(f"{capability}: modules must be non-empty")
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

        for path, ceiling in sorted(modules.items()):
            if not isinstance(ceiling, int) or isinstance(ceiling, bool) or ceiling <= 0:
                failures.append(f"{capability}: invalid LOC ceiling for {path}: {ceiling!r}")
                continue
            file_path = pathlib.Path(path)
            if not file_path.is_file():
                continue
            lines = len(file_path.read_text(encoding="utf-8").splitlines())
            print(f"{capability}: {path}: {lines}/{ceiling} LOC")
            if lines > ceiling:
                failures.append(
                    f"{capability}: {path}: {lines} LOC exceeds maintenance-only ceiling "
                    f"{ceiling}; reduce the overlap or explicitly ratchet the manifest with rationale"
                )

    if manifest.get("changePolicy") != REQUIRED_CHANGE_POLICY:
        failures.append("changePolicy drifted or lost fail-closed controls")

    if failures:
        raise SystemExit("upstream convergence guard failed:\n" + "\n".join(failures))

    print("PASS upstream convergence guard: Mission Control control-plane role retained")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
