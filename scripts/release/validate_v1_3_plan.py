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
REQUIRED_RATCHET = {
    "src/app.rs",
    "src/ui.rs",
    "src/app_server.rs",
    "src/main.rs",
    "src/app/context.rs",
    "src/app/lifecycle.rs",
    "src/app/review.rs",
    "src/app/saved_view.rs",
    "src/app/search.rs",
    "src/app/palette.rs",
    "src/app_server/lifecycle.rs",
    "src/ui/board.rs",
    "src/ui/review.rs",
    "src/ui/thread.rs",
    "src/runtime_external.rs",
    "src/runtime_planning.rs",
    "src/bin/ux-performance-evidence.rs",
}
EXPECTED_GATED = {
    "gitlab-issue-board-projection",
    "github-safe-write-parity",
}
EXPECTED_FREEZE_CLASSES = {
    "defect-fix",
    "security",
    "compatibility",
    "qualification-evidence",
    "release-tooling",
    "documentation",
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
    assert REQUIRED_RATCHET <= set(plan["moduleRatchet"])
    assert all(
        isinstance(value, int) and value > 0
        for value in plan["moduleRatchet"].values()
    )
    assert set(plan["evidenceGatedDecisions"]) == EXPECTED_GATED
    assert all(
        plan["evidenceGatedDecisions"][item]["status"] == "evidence-gated"
        for item in EXPECTED_GATED
    )
    assert plan["freezePolicy"]["newCoreFunctionality"] == "requires-new-development-plan"
    assert set(plan["freezePolicy"]["allowedChangeClasses"]) == EXPECTED_FREEZE_CLASSES
    assert plan["exit"]["openP0Defects"] == 0
    assert plan["exit"]["p0Complete"] is True
    assert plan["exit"]["p1Complete"] is True
    assert plan["exit"]["fakeEvidenceCannotSatisfyRealEnvironmentGates"] is True
    print("PASS v1.3 development plan")

if __name__ == "__main__":
    main()
