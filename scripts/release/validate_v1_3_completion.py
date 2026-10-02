#!/usr/bin/env python3
import json
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[2]
PLAN_PATH = ROOT / "release" / "v1.3-plan.json"
COMPLETION_PATH = ROOT / "release" / "v1.3-completion.json"

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
EXPECTED_P2_IMPLEMENTED = {
    "fuzzy-command-palette",
    "user-perceived-performance-evidence",
}
EXPECTED_P2_GATED = {
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
FEATURE_BASELINE = "8ea619fcbceba514ccc5f8f906446a3726968eae"


def read(path):
    return (ROOT / path).read_text(encoding="utf-8")


def require_tokens(path, tokens):
    text = read(path)
    missing = [token for token in tokens if token not in text]
    if missing:
        raise SystemExit("%s missing required token(s): %s" % (path, missing))
    return text


def phase_map(completion, phase):
    return {entry["id"]: entry for entry in completion["completed"][phase]}


def validate_manifest():
    plan = json.loads(PLAN_PATH.read_text(encoding="utf-8"))
    completion = json.loads(COMPLETION_PATH.read_text(encoding="utf-8"))

    assert plan["schema"] == "codex-tui/v1.3-development-plan/v1"
    assert completion["schema"] == "codex-tui/v1.3-completion/v1"
    assert completion["target"] == plan["target"]
    assert completion["featureCompletionBaselineSha"] == FEATURE_BASELINE
    assert completion["status"] == "development-scope-complete"
    assert completion["stableReady"] is False
    assert completion["publicationAllowed"] is False
    assert completion["releaseAuthority"] == "not-opened"

    p0 = phase_map(completion, "P0")
    p1 = phase_map(completion, "P1")
    p2 = phase_map(completion, "P2")
    assert set(p0) == EXPECTED_P0
    assert set(p1) == EXPECTED_P1
    assert set(p2) == EXPECTED_P2_IMPLEMENTED | EXPECTED_P2_GATED
    assert all(entry["status"] == "implemented" for entry in p0.values())
    assert all(entry["status"] == "implemented" for entry in p1.values())
    assert all(p2[item]["status"] == "implemented" for item in EXPECTED_P2_IMPLEMENTED)
    assert all(p2[item]["status"] == "evidence-gated" for item in EXPECTED_P2_GATED)

    assert completion["freezePolicy"]["newCoreFunctionality"] == "requires-new-development-plan"
    assert set(completion["freezePolicy"]["allowedChangeClasses"]) == EXPECTED_FREEZE_CLASSES
    assert completion["freezePolicy"] == plan["freezePolicy"]
    assert set(plan["evidenceGatedDecisions"]) == EXPECTED_P2_GATED
    assert all(
        plan["evidenceGatedDecisions"][item]["status"] == "evidence-gated"
        for item in EXPECTED_P2_GATED
    )

    evidence_paths = []
    for phase in ("P0", "P1", "P2"):
        for entry in completion["completed"][phase]:
            evidence_paths.extend(entry.get("evidencePaths", []))
    for path in evidence_paths:
        if not (ROOT / path).is_file():
            raise SystemExit("completion evidence path missing: %s" % path)

    ux = completion["uxPerformanceEvidence"]
    assert ux["sourceSha"] == FEATURE_BASELINE
    assert ux["workflowRunId"] == 37039083082
    assert ux["artifactId"] == 11240639945
    assert ux["source"] == "github-hosted:ubuntu-24.04"
    assert ux["rows"] == 10000
    assert ux["warmupIterations"] >= 20
    assert ux["iterations"] >= 200
    assert ux["sampleQualified"] is True
    assert ux["diagnosticOnly"] is True
    assert ux["terminal"] == {"width": 160, "height": 50}
    for key in ("board10kRender", "thread10kRender"):
        metrics = ux[key]
        values = [
            metrics["p50Ms"],
            metrics["p95Ms"],
            metrics["p99Ms"],
            metrics["maxMs"],
        ]
        assert all(isinstance(value, (int, float)) and value >= 0 for value in values)
        assert values == sorted(values)

    assert completion["architecture"]["v1_2CeilingsNotRaised"] is True
    assert plan["moduleRatchet"]["src/app.rs"] <= 6650
    assert plan["moduleRatchet"]["src/ui.rs"] <= 3100
    assert plan["moduleRatchet"]["src/app_server.rs"] <= 2780
    assert plan["moduleRatchet"]["src/main.rs"] <= 1900
    return plan, completion


def validate_implemented_contracts():
    require_tokens(
        "src/ui/board.rs",
        [
            "fn board_viewport(",
            "Scrollbar::new(ScrollbarOrientation::VerticalRight)",
            "planning_card_line(",
            "visible_fields",
        ],
    )

    planning = require_tokens(
        "src/planning.rs",
        [
            "LinkRole::Goal",
            "LinkRole::Worktree",
            "LinkRole::ChangeRequest",
            "pub fn card_matches_filter",
            "validate_saved_view_filter",
        ],
    )
    if "transcript" in read("src/app/search.rs").lower():
        raise SystemExit("unified metadata search must not depend on transcript hydration")
    require_tokens(
        "src/app/search.rs",
        [
            "thread_search_extra_fields",
            "planning_cards_for_active_view",
            "commit_metadata_search",
            "cancel_metadata_search",
        ],
    )

    require_tokens(
        "src/app/lifecycle.rs",
        [
            'lifecycle_capability_available("thread/start")',
            'lifecycle_capability_available("thread/fork")',
            "plan_start_thread",
            "plan_fork_thread",
        ],
    )
    require_tokens(
        "src/app_server/lifecycle.rs",
        [
            '"thread/start"',
            '"thread/fork"',
            "is_method_unsupported",
            "optional_capabilities_missing",
        ],
    )

    require_tokens(
        "src/app/saved_view.rs",
        [
            "SavedViewEditField::Name",
            "SavedViewEditField::Source",
            "SavedViewEditField::Filter",
            "SavedViewEditField::Layout",
            "SavedViewEditField::GroupBy",
            "SavedViewEditField::OrderBy",
            "SavedViewEditField::VisibleFields",
            "validate_saved_view_filter",
        ],
    )

    require_tokens(
        "src/ui/review.rs",
        [
            "fn review_evidence_lines",
            "cached_review_diff(",
            "Pipeline #",
            "approvals=",
        ],
    )
    require_tokens(
        "src/runtime_external.rs",
        [
            "external target must be an HTTP(S) URL",
            "CODEX_TUI_BROWSER",
            "xdg-open",
            "rundll32.exe",
        ],
    )

    require_tokens(
        "src/conversation.rs",
        [
            "pub revision: u64",
            "self.revision = self.revision.saturating_add(1)",
        ],
    )
    require_tokens(
        "src/ui/thread.rs",
        [
            "MAX_WINDOW_ITEMS: usize = 512",
            "PRESENTATION_CACHE",
            "fn item_window(",
            "render_item_scrollbar",
        ],
    )

    require_tokens(
        "src/app/palette.rs",
        [
            "command_palette_query",
            "fuzzy_subsequence",
            "palette_label(simplified_chinese)",
            "input_command_palette_text",
        ],
    )
    require_tokens(
        "src/runtime_input.rs",
        [
            "Action::CommandPaletteInputText(text)",
            "Action::CommandPaletteInputChar(character)",
            "KeyCode::Backspace => reduce(app, Action::CommandPaletteBackspace)",
        ],
    )

    require_tokens(
        "src/bin/ux-performance-evidence.rs",
        [
            '"codex-tui/ux-performance/v1"',
            "board10kRender",
            "thread10kRender",
            "warmup",
            "iterations",
        ],
    )
    workflow = require_tokens(
        ".github/workflows/ux-performance-diagnostics.yml",
        [
            "release/v1.3-development",
            "--source-sha",
            "--warmup 20",
            "--iterations 200",
            "retention-days: 90",
        ],
    )
    if "p95Ms" not in workflow or "p99Ms" not in workflow:
        raise SystemExit("UX diagnostics must validate retained percentile metrics")


def validate_evidence_gates():
    forge = read("src/forge.rs")
    marker = "pub(crate) async fn probe_gitlab_with_remote("
    start = forge.find(marker)
    end = forge.find("pub async fn probe_issue_boards", start)
    if start < 0 or end < 0:
        raise SystemExit("GitLab normal/board probe boundaries are missing")
    normal_probe = forge[start:end]
    if "probe_issue_boards(" in normal_probe:
        raise SystemExit("GitLab issue boards must not silently enter the four-call normal refresh")
    required_comment = "Keep the normal user-visible refresh at four glab API subprocesses"
    if required_comment not in normal_probe:
        raise SystemExit("GitLab four-subprocess hard boundary is no longer documented in code")
    if normal_probe.count("glab_api_json(") != 4:
        raise SystemExit("GitLab normal refresh no longer has exactly four glab API calls")

    forge_types = read("src/forge_types.rs")
    observation_start = forge_types.find("pub struct ForgeObservation")
    observation_end = forge_types.find("impl ForgeObservation", observation_start)
    observation = forge_types[observation_start:observation_end]
    if "boards:" in observation or "IssueBoardSummary" in observation:
        raise SystemExit("normal ForgeObservation must not claim GitLab board projection")

    mutation = require_tokens(
        "src/forge_mutation.rs",
        [
            "identity.provider == ForgeProviderKind::GitLab",
            "M6b supports GitLab mutations only",
            "plan.provider == ForgeProviderKind::GitLab",
        ],
    )
    if "ForgeProviderKind::GitHub" in mutation[mutation.find("fn ensure_identity"):mutation.find("fn required_text")]:
        raise SystemExit("GitHub mutations must remain fail-closed until provider-specific preflight exists")

    app = read("src/app.rs")
    target = app.find("fn current_forge_mutation_target")
    if target >= 0:
        snippet = app[target:target + 2600]
        if "identity.provider != ForgeProviderKind::GitLab" not in snippet:
            raise SystemExit("App mutation target must reject non-GitLab provider identity")


def main():
    validate_manifest()
    validate_implemented_contracts()
    validate_evidence_gates()
    print("PASS v1.3 development completion/freeze contract")


if __name__ == "__main__":
    main()
