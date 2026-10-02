#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re

PLAN_SCHEMA = "codex-tui/v1.2-plan/v1"
COMPLETION_SCHEMA = "codex-tui/v1.2-completion/v1"
CRITERIA_SCHEMA = "codex-tui/stable-criteria/v2"
HEX40 = re.compile(r"^[0-9a-f]{40}$")

EXPECTED_PRIORITY_GROUPS = {
    "P0": [
        "architecture-decomposition",
        "replay-compatibility",
        "dependency-security-governance",
    ],
    "P1": [
        "headless-contract-completion",
        "lightweight-notifications",
        "syntax-highlighting",
    ],
    "P2": [
        "accessibility-mature-mode",
    ],
}

SOURCE_TOKENS = {
    "src/headless/surfaces.rs": [
        "codex-tui/headless-status/v1",
        "codex-tui/headless-attention/v1",
        "codex-tui/headless-board/v1",
        "codex-tui/headless-forge/v1",
        "codex-tui/headless-worktrees/v1",
    ],
    "src/notification.rs": [
        "pub enum NotificationMode",
        "Off",
        "Terminal",
        "Os",
    ],
    "src/runtime_notifications.rs": [
        "NOTIFICATION_QUEUE_CAPACITY: usize = 32",
        "NOTIFICATION_NOTICE_CAPACITY: usize = 16",
        "OS_NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(3)",
        "mpsc::channel(NOTIFICATION_QUEUE_CAPACITY)",
    ],
    "src/syntax_highlight.rs": [
        "MAX_SYNC_HIGHLIGHT_BYTES: usize = 128 * 1024",
        "CACHE_CAPACITY: usize = 8",
        "LazyLock<Mutex<SyntaxHighlighter>>",
        "pub fn prewarm_review_diff",
        "pub fn cached_review_diff",
    ],
    "src/ui.rs": [
        "cached_review_diff(thread_id, review.observed_at_unix_ms, app.review_word_diff)",
    ],
    "src/main.rs": [
        "prewarm_review_diff(&review)",
        "presentation_mode.should_render",
        "urgent_render = true",
    ],
    "src/presentation.rs": [
        "QUIET_BACKGROUND_REDRAW_MS: u64 = 100",
        "SCREEN_READER_BACKGROUND_REDRAW_MS: u64 = 500",
        "pub enum PresentationMode",
        "Self::Quiet",
        "Self::ScreenReader",
        "urgent || elapsed >= self.background_redraw_interval()",
    ],
    "src/store.rs": [
        "pub presentation: PresentationMode",
        "presentation: PresentationMode::Normal",
    ],
    "Cargo.toml": [
        'two-face = { version = "0.5.2", default-features = false, features = ["syntect-fancy"] }',
    ],
    ".github/workflows/protocol-compatibility.yml": [
        "name: protocol-compatibility",
        "cargo test --locked --test protocol_replay",
    ],
    ".github/workflows/security.yml": [
        "EmbarkStudios/cargo-deny-action@v2.0.20",
        "cargo audit",
    ],
    ".github/dependabot.yml": [
        "package-ecosystem: cargo",
        "package-ecosystem: github-actions",
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
    groups = {"P0": [], "P1": [], "P2": []}
    for item in plan.get("priorities", []):
        if not isinstance(item, dict):
            raise SystemExit("v1.2 priority entry must be an object")
        priority = item.get("priority")
        identifier = item.get("id")
        if priority not in groups or not isinstance(identifier, str) or not identifier:
            raise SystemExit("unexpected v1.2 priority entry: {!r}".format(item))
        groups[priority].append(identifier)
    return groups


def main():
    parser = argparse.ArgumentParser(
        description="Validate v1.2 development-scope completion without claiming stable readiness."
    )
    parser.add_argument("--plan", default="release/v1.2-plan.json")
    parser.add_argument("--completion", default="release/v1.2-completion.json")
    parser.add_argument("--criteria", default="release/v1.2-criteria.json")
    args = parser.parse_args()

    plan_path = pathlib.Path(args.plan)
    completion_path = pathlib.Path(args.completion)
    criteria_path = pathlib.Path(args.criteria)
    plan = load(plan_path)
    completion = load(completion_path)
    criteria = load(criteria_path)

    if plan.get("schema") != PLAN_SCHEMA:
        raise SystemExit("unexpected v1.2 plan schema: {!r}".format(plan.get("schema")))
    if completion.get("schema") != COMPLETION_SCHEMA:
        raise SystemExit(
            "unexpected v1.2 completion schema: {!r}".format(completion.get("schema"))
        )
    if criteria.get("schema") != CRITERIA_SCHEMA:
        raise SystemExit(
            "unexpected v1.2 stable criteria schema: {!r}".format(criteria.get("schema"))
        )

    if plan.get("targetVersion") != "1.2.0":
        raise SystemExit("v1.2 plan targetVersion must be 1.2.0")
    if completion.get("targetVersion") != plan.get("targetVersion"):
        raise SystemExit("completion targetVersion drifted from v1.2 plan")
    if criteria.get("stableVersion") != plan.get("targetVersion"):
        raise SystemExit("stable criteria version drifted from v1.2 plan")
    if completion.get("planSchema") != PLAN_SCHEMA:
        raise SystemExit("completion planSchema drifted")

    baseline = str(completion.get("featureCompletionBaselineSha", "")).lower()
    if not HEX40.fullmatch(baseline):
        raise SystemExit("featureCompletionBaselineSha must be exactly 40 hexadecimal characters")
    if completion.get("status") != "development-scope-complete":
        raise SystemExit("v1.2 completion status must remain development-scope-complete")
    if completion.get("stableReady") is not False:
        raise SystemExit("completion manifest must never claim stableReady")
    if completion.get("publicationAllowed") is not False:
        raise SystemExit("completion manifest must never allow publication")

    groups = priority_groups(plan)
    if groups != EXPECTED_PRIORITY_GROUPS:
        raise SystemExit("v1.2 priority groups drifted: {!r}".format(groups))
    if completion.get("completedPriorities") != groups:
        raise SystemExit("completion manifest does not cover every P0/P1/P2 priority exactly")

    expected_checks = [
        "automated-qualification",
        "module-ratchet",
        "protocol-replay",
        "scope-completion",
    ]
    if plan.get("developmentQualification", {}).get("requiredChecks") != expected_checks:
        raise SystemExit("v1.2 plan development qualification checks drifted")
    if criteria.get("developmentQualification", {}).get("requiredChecks") != expected_checks:
        raise SystemExit("v1.2 stable criteria development qualification checks drifted")

    all_ids = [item for group in groups.values() for item in group]
    evidence = completion.get("evidence")
    if not isinstance(evidence, dict) or set(evidence) != set(all_ids):
        raise SystemExit("completion evidence keys must exactly match v1.2 priority IDs")
    for identifier in all_ids:
        item = evidence.get(identifier)
        paths = item.get("paths") if isinstance(item, dict) else None
        if not isinstance(paths, list) or not paths:
            raise SystemExit("completion evidence paths missing for {}".format(identifier))
        for raw_path in paths:
            path = pathlib.Path(raw_path)
            if not path.is_file():
                raise SystemExit("completion evidence path is missing: {}".format(path))

    if completion.get("deferredUntilEvidence") != plan.get("evidenceDrivenDeferred"):
        raise SystemExit("completion deferredUntilEvidence drifted from v1.2 plan")
    if completion.get("deferredUntilEvidence") != criteria.get("deferredUntilEvidence"):
        raise SystemExit("completion deferredUntilEvidence drifted from stable criteria")
    if completion.get("frozenOptionalLayers") != plan.get("frozenOptionalLayers"):
        raise SystemExit("completion frozenOptionalLayers drifted from v1.2 plan")
    if completion.get("frozenOptionalLayers") != criteria.get("frozenOptionalLayers"):
        raise SystemExit("completion frozenOptionalLayers drifted from stable criteria")

    freeze = completion.get("freezePolicy")
    if not isinstance(freeze, dict):
        raise SystemExit("completion freezePolicy is missing")
    if freeze.get("newCoreFunctionality") != "requires-new-development-plan":
        raise SystemExit("new v1.2 core functionality must require a new development plan")
    expected_change_classes = [
        "defect-fix",
        "security",
        "compatibility",
        "qualification-evidence",
        "release-tooling",
        "documentation",
    ]
    if freeze.get("allowedChangeClasses") != expected_change_classes:
        raise SystemExit("v1.2 freeze allowedChangeClasses drifted")

    if completion.get("stableAuthority") != "release/v1.2-criteria.json":
        raise SystemExit("completion stable authority must remain release/v1.2-criteria.json")
    if "release-evidence/v4" not in str(completion.get("stableEvidencePolicy", "")):
        raise SystemExit("completion policy must retain release-evidence/v4 stable authority")

    dev_qualification = criteria.get("developmentQualification")
    if not isinstance(dev_qualification, dict):
        raise SystemExit("stable criteria developmentQualification is missing")
    if dev_qualification.get("stableAuthority") is not False:
        raise SystemExit("hosted development qualification must not become stable authority")

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
    }:
        if gate not in required_gate_ids:
            raise SystemExit("stable criteria lost required gate: {}".format(gate))

    for raw_path, tokens in SOURCE_TOKENS.items():
        path = pathlib.Path(raw_path)
        if not path.is_file():
            raise SystemExit("completion contract source is missing: {}".format(path))
        require_tokens(path, tokens)

    ui_text = pathlib.Path("src/ui.rs").read_text(encoding="utf-8")
    if "highlight_review_diff" in ui_text:
        raise SystemExit("Review render path must not call the old computing highlighter")
    if "HighlightLines::new" in ui_text or ".highlight_line(" in ui_text:
        raise SystemExit("Review render module must not perform syntect parsing")

    print(
        "VALID v1.2 completion: P0/P1/P2 development scope closed; "
        "stableReady=false; publicationAllowed=false; stable evidence remains separate"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
