#!/usr/bin/env python3
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PLAN = ROOT / "release" / "v1.3-plan.json"

EXPECTED_P0 = {
    "board-large-dataset-navigation",
    "workcard-relationship-closure",
    "codex-thread-create-fork-worktree-handoff",
    "unified-metadata-search",
}
EXPECTED_P1 = {
    "saved-view-editor",
    "review-evidence-workspace-and-external-open",
    "core-module-decomposition",
    "long-thread-viewport-and-cache",
}
EXPECTED_P2 = {
    "gitlab-issue-board-projection",
    "github-safe-write-parity",
    "fuzzy-command-palette",
    "user-perceived-performance-evidence",
}
REQUIRED_DEFERRED = {
    "thread-queue-until-stable-upstream-capability",
    "transcript-fts-until-measured-need",
    "native-gitlab-transport-until-existing-hard-trigger",
    "remote-app-server-targets",
}

def main() -> None:
    plan = json.loads(PLAN.read_text(encoding="utf-8"))
    assert plan["schema"] == "codex-tui/v1.3-development-plan/v1"
    assert plan["target"] == "v1.3-workflow-completion-ux"
    assert plan["integrationBranch"] == "release/v1.3-development"
    assert plan["authority"] == "development-scope-only"
    assert set(plan["phases"]["P0"]) == EXPECTED_P0
    assert set(plan["phases"]["P1"]) == EXPECTED_P1
    assert set(plan["phases"]["P2"]) == EXPECTED_P2
    assert REQUIRED_DEFERRED <= set(plan["deferred"])
    assert plan["exit"]["openP0Defects"] == 0
    assert plan["exit"]["p0Complete"] is True
    assert plan["exit"]["p1Complete"] is True
    assert plan["exit"]["fakeEvidenceCannotSatisfyRealEnvironmentGates"] is True
    print("PASS v1.3 development plan")

if __name__ == "__main__":
    main()
