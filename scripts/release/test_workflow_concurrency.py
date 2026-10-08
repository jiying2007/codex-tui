"""Preserve exact-SHA evidence for main/dispatch and cancel only superseded PRs."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]

EVIDENCE_WORKFLOWS = (
    "ci",
    "development-qualification",
    "performance-diagnostics",
    "protocol-compatibility",
    "scale-evidence",
    "security",
    "terminal-regression",
)


class WorkflowConcurrencyContract(unittest.TestCase):
    def test_evidence_workflows_isolate_non_pr_shas_and_cancel_only_prs(self):
        for name in EVIDENCE_WORKFLOWS:
            with self.subTest(workflow=name):
                source = (ROOT / ".github/workflows" / (name + ".yml")).read_text(
                    encoding="utf-8"
                )
                self.assertIn(
                    "group: " + name
                    + "-${{ github.event_name == 'pull_request' && github.ref || github.sha }}",
                    source,
                )
                self.assertIn(
                    "cancel-in-progress: ${{ github.event_name == 'pull_request' }}",
                    source,
                )
                self.assertNotIn("cancel-in-progress: true", source)

    def test_convergence_manifest_only_pr_triggers_development_qualification(self):
        source = (ROOT / ".github/workflows/development-qualification.yml").read_text(
            encoding="utf-8"
        )
        pr_paths = source.split("  pull_request:\n", 1)[1].split(
            "  workflow_dispatch:\n", 1
        )[0]
        self.assertIn('      - "release/v1.5-convergence.json"', pr_paths)
        self.assertIn(
            "scripts/architecture/check_upstream_convergence.py",
            source,
        )

    def test_release_gate_preserves_serial_pending_runs(self):
        source = (ROOT / ".github/workflows/release.yml").read_text(
            encoding="utf-8"
        )
        _, marker, rest = source.partition("\nconcurrency:\n")
        self.assertTrue(marker, "release workflow-level concurrency missing")
        policy = rest.split("\njobs:\n", 1)[0]
        self.assertIn("group: release-", policy)
        self.assertIn("github.ref", policy)
        self.assertIn("cancel-in-progress: false", policy)
        self.assertIn("queue: max", policy)

    def test_release_publication_remains_non_cancelling(self):
        source = (ROOT / ".github/workflows/release.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("group: codex-tui-release-publish", source)
        self.assertIn("cancel-in-progress: false", source)


if __name__ == "__main__":
    unittest.main()
