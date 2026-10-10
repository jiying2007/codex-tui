"""Guard exact pre-merge PTY coverage when terminal-relevant source changes."""
from pathlib import Path
import unittest


WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/terminal-regression.yml"


def event_paths(text: str, event: str) -> set:
    inside = False
    paths = set()
    for line in text.splitlines():
        if line == "  {}:".format(event):
            inside = True
            continue
        if inside and line.startswith("  ") and not line.startswith("    ") and line.strip():
            break
        if inside and line.startswith("      - "):
            paths.add(line.split("- ", 1)[1].strip().strip("\"'"))
    return paths


class TerminalWorkflowScope(unittest.TestCase):
    def test_source_only_pr_still_runs_hosted_pty_regression(self):
        source = WORKFLOW.read_text(encoding="utf-8")
        for event in ("pull_request", "push"):
            with self.subTest(event=event):
                paths = event_paths(source, event)
                self.assertIn("src/**", paths)
                self.assertIn("tests/terminal_restoration.rs", paths)
                self.assertIn("scripts/testing/**", paths)

    def test_missing_pr_source_scope_is_not_accepted(self):
        source = WORKFLOW.read_text(encoding="utf-8")
        omitted = source.replace(
            "  pull_request:\n    paths:\n      - 'src/**'\n",
            "  pull_request:\n    paths:\n",
            1,
        )
        self.assertNotEqual(omitted, source)
        self.assertNotIn("src/**", event_paths(omitted, "pull_request"))


if __name__ == "__main__":
    unittest.main()
