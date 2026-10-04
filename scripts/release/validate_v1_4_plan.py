#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import re

PLAN = pathlib.Path("release/v1.4-plan.json")
V13_COMPLETION = pathlib.Path("release/v1.3-completion.json")
V14_COMPLETION = pathlib.Path("release/v1.4-completion.json")
EXPECTED = [
    ("board-large-dataset-navigation", "P0"),
    ("workcard-relationship-closure", "P0"),
    ("codex-thread-lifecycle-handoff", "P0"),
    ("unified-metadata-search", "P0"),
    ("saved-view-editor", "P1"),
    ("review-evidence-and-external-open", "P1"),
    ("long-thread-viewport-cache", "P1"),
    ("fuzzy-command-palette", "P1"),
    ("core-module-decomposition", "P2"),
    ("user-perceived-performance-evidence", "P2"),
]

def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))

def package_version() -> str:
    text = pathlib.Path("Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text.split("[dependencies]",1)[0], re.MULTILINE)
    if not match:
        raise SystemExit("Cargo package version is missing")
    return match.group(1)

def main() -> int:
    plan=load(PLAN)
    v13=load(V13_COMPLETION)
    if plan.get("schema")!="codex-tui/v1.4-plan/v1":
        raise SystemExit("unexpected v1.4 plan schema")
    if plan.get("targetVersion")!="1.4.0":
        raise SystemExit("v1.4 targetVersion must be 1.4.0")
    if plan.get("entryMainSha")!="01e1f3d20d7b234b1ff17f85cc05321450e9fcf5":
        raise SystemExit("v1.4 entry main SHA drifted")
    current = package_version()
    if V14_COMPLETION.is_file():
        if current != "1.4.0":
            raise SystemExit("v1.4 completion requires package version 1.4.0")
    elif current != "1.3.0":
        raise SystemExit("v1.4 integration must retain stable predecessor package version 1.3.0 until completion")
    if v13.get("schema")!="codex-tui/v1.3-completion/v1" or v13.get("status")!="development-scope-complete":
        raise SystemExit("v1.3 predecessor completion is not retained")
    if v13.get("stableReady") is not False or v13.get("publicationAllowed") is not False:
        raise SystemExit("v1.4 must not reinterpret hosted v1.3 evidence as stable PASS")
    actual=[(x.get("id"),x.get("priority")) for x in plan.get("priorities",[])]
    if actual!=EXPECTED:
        raise SystemExit("v1.4 priority/order drifted: {!r}".format(actual))
    if plan.get("completionOrder")!=[x for x,_ in EXPECTED]:
        raise SystemExit("v1.4 completion order drifted")
    if set(plan.get("evidenceGatedDecisions",{}))!={"gitlab-issue-board-projection"}:
        raise SystemExit("unexpected v1.4 evidence-gated decision set")
    ratchet=plan.get("moduleRatchet")
    if not isinstance(ratchet,dict) or not ratchet:
        raise SystemExit("v1.4 module ratchet is missing")
    for path in ("src/app.rs","src/ui.rs","src/app_server.rs","src/main.rs","src/conversation.rs"):
        if not isinstance(ratchet.get(path),int) or ratchet[path] <= 0:
            raise SystemExit("v1.4 ratchet missing {}".format(path))
    state = "completion-active" if V14_COMPLETION.is_file() else "integration-active"
    print("VALID v1.4 plan: workflow-completion reconciliation authorized; " + state)
    return 0

if __name__=="__main__":
    raise SystemExit(main())
