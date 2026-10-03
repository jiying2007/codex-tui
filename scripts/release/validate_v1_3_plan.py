#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import re

PLAN = pathlib.Path("release/v1.3-plan.json")
V12_COMPLETION = pathlib.Path("release/v1.2-completion.json")
V13_COMPLETION = pathlib.Path("release/v1.3-completion.json")
EXPECTED = [
    ("transcript-search", "P0"),
    ("thread-queue", "P0"),
    ("remote-app-server-targets", "P1"),
    ("github-safe-mutations", "P1"),
]


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def package_version() -> str:
    text = pathlib.Path("Cargo.toml").read_text(encoding="utf-8")
    prefix = text.split("[dependencies]", 1)[0]
    match = re.search(r'^version\s*=\s*"([^"]+)"', prefix, re.MULTILINE)
    if not match:
        raise SystemExit("Cargo package version is missing")
    return match.group(1)


def main() -> int:
    plan = load(PLAN)
    v12 = load(V12_COMPLETION)

    if plan.get("schema") != "codex-tui/v1.3-plan/v1":
        raise SystemExit("unexpected v1.3 plan schema")
    if plan.get("targetVersion") != "1.3.0":
        raise SystemExit("v1.3 targetVersion must be 1.3.0")
    if plan.get("theme") != "search-and-multi-target":
        raise SystemExit("unexpected v1.3 theme")
    if not re.fullmatch(r"[0-9a-f]{40}", str(plan.get("entryMainSha", ""))):
        raise SystemExit("v1.3 entryMainSha must be an exact Git SHA")

    activation = plan.get("versionActivation") or {}
    current = package_version()
    if current not in {
        activation.get("developmentPackageVersion"),
        activation.get("finalPackageVersion"),
    }:
        raise SystemExit(
            f"Cargo version {current!r} is outside the v1.3 activation contract"
        )

    if V13_COMPLETION.is_file() and current != activation.get("finalPackageVersion"):
        raise SystemExit(
            "v1.3 completion exists but Cargo is not at finalPackageVersion"
        )

    ratchet = plan.get("moduleRatchet")
    if not isinstance(ratchet, dict):
        raise SystemExit("v1.3 moduleRatchet is missing")
    for required_path in (
        "src/app.rs",
        "src/app_server.rs",
        "src/sqlite_store.rs",
        "src/transcript_search.rs",
    ):
        ceiling = ratchet.get(required_path)
        if not isinstance(ceiling, int) or ceiling <= 0:
            raise SystemExit(f"v1.3 moduleRatchet missing {required_path}")

    priorities = plan.get("priorities")
    if not isinstance(priorities, list):
        raise SystemExit("v1.3 priorities are missing")
    actual = [(item.get("id"), item.get("priority")) for item in priorities]
    if actual != EXPECTED:
        raise SystemExit(f"v1.3 priority/order drifted: {actual!r}")

    order = plan.get("completionOrder")
    if order != [identifier for identifier, _ in EXPECTED]:
        raise SystemExit("v1.3 completion order drifted")

    if v12.get("status") != "development-scope-complete":
        raise SystemExit("v1.2 must remain development-scope-complete before v1.3 core work")
    if v12.get("stableReady") is not False or v12.get("publicationAllowed") is not False:
        raise SystemExit("v1.3 must not reinterpret v1.2 external evidence as stable PASS")

    non_goals = set(plan.get("nonGoals") or [])
    required_non_goals = {
        "team-server-rbac",
        "web-mobile-companion",
        "generic-plugin-runtime",
        "multi-agent-providers",
        "job-workflow-engine",
    }
    missing = required_non_goals - non_goals
    if missing:
        raise SystemExit("v1.3 non-goals missing: " + ", ".join(sorted(missing)))

    print(
        "VALID v1.3 plan: Search & Multi-Target core work is authorized; "
        f"Cargo={current}; v1.2 checkpoint remains separate"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
