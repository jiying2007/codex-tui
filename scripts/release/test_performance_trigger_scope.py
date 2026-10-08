"""Ensure PR-only performance diagnostics skip unrelated release script churn."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
PERF = ROOT / ".github/workflows/performance-diagnostics.yml"
RELEASE = ROOT / ".github/workflows/release.yml"

def pr_paths():
    content = PERF.read_text(encoding="utf-8")
    marker = "  pull_request:\n    paths:\n"
    if content.count(marker) != 1 or "  workflow_dispatch:\n" not in content:
        raise ValueError("performance PR trigger structure requires review")
    section = content.split(marker, 1)[1].split("  workflow_dispatch:\n", 1)[0]
    result = []
    for line in section.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        match = re.fullmatch(r'      - "([^"\n]+)"', line)
        if not match or match.group(1).startswith("!"):
            raise ValueError("unsupported performance path: " + line)
        result.append(match.group(1))
    if not result:
        raise ValueError("no PR diagnostics paths")
    return result

class PerformanceTriggerScopeTests(unittest.TestCase):
    def test_main_push_always_retains_exact_sha_evidence(self):
        text = PERF.read_text(encoding="utf-8")
        self.assertIn("  push:\n    branches: [main]\n", text)
        self.assertIn("CODEX_TUI_GIT_SHA:", text)
        self.assertIn("github.sha", text)

    def test_pr_skips_unrelated_release_helpers(self):
        paths = pr_paths()
        self.assertNotIn("scripts/release/**", paths)
        self.assertIn("scripts/release/validate_diagnostics.py", paths)
        self.assertIn(".github/workflows/performance-diagnostics.yml", paths)

    def test_runtime_and_policy_changes_still_trigger_pr_diagnostics(self):
        paths = set(pr_paths())
        required = {
            "Cargo.toml", "Cargo.lock", "src/main.rs", "src/app.rs",
            "src/ui.rs", "src/ui/**", "src/planning.rs",
            "src/conversation.rs", "src/render_performance.rs",
            "src/interaction_performance.rs", "src/release_benchmark.rs",
            "release/v1.4-criteria.json",
        }
        self.assertFalse(required - paths)

    def test_release_gate_retains_broad_main_push_helper_coverage(self):
        release = RELEASE.read_text(encoding="utf-8")
        prefix = release.split("  workflow_dispatch:\n", 1)[0]
        self.assertIn('      - "scripts/release/**"', prefix)

if __name__ == "__main__":
    unittest.main()
