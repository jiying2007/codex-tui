from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class WorkflowEvidenceRetention(unittest.TestCase):
    def test_terminal_success_requires_evidence_and_failure_keeps_partial(self):
        text = (ROOT / ".github/workflows/terminal-regression.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("name: automated-pty-${{ github.event.pull_request.head.sha || github.sha }}", text)
        success = text.split(
            "name: automated-pty-${{ github.event.pull_request.head.sha || github.sha }}", 1
        )[1].split(
            "name: automated-pty-partial-${{ github.event.pull_request.head.sha || github.sha }}", 1
        )[0]
        self.assertIn("if-no-files-found: error", success)

        partial = text.split(
            "name: automated-pty-partial-${{ github.event.pull_request.head.sha || github.sha }}", 1
        )[1]
        self.assertIn("if: failure()", text)
        self.assertIn("if-no-files-found: warn", partial)

    def test_retained_soak_success_cannot_omit_receipt(self):
        text = (ROOT / ".github/workflows/retained-soak.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("if: always()", text)
        self.assertIn("if-no-files-found: error", text)

    def test_repository_hygiene_always_requires_plan_or_receipt(self):
        text = (ROOT / ".github/workflows/repository-hygiene.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("if: always()", text)
        self.assertIn("if-no-files-found: error", text)


if __name__ == "__main__":
    unittest.main()
