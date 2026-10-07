from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class WorkflowConcurrencyContract(unittest.TestCase):
    def test_development_qualification_cancels_only_superseded_pr_runs(self):
        text = (ROOT / ".github/workflows/development-qualification.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn(
            "group: development-qualification-${{ github.ref }}",
            text,
        )
        self.assertIn(
            "cancel-in-progress: ${{ github.event_name == 'pull_request' }}",
            text,
        )
        self.assertNotIn("cancel-in-progress: true", text)

    def test_canonical_ci_keeps_same_pr_only_cancellation_policy(self):
        text = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn(
            "cancel-in-progress: ${{ github.event_name == 'pull_request' }}",
            text,
        )

    def test_release_publication_remains_non_cancelling(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertIn("group: codex-tui-release-publish", text)
        self.assertIn("cancel-in-progress: false", text)


if __name__ == "__main__":
    unittest.main()
