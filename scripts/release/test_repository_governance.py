"""Repository governance stays reviewable and requires no paid tooling."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class GovernanceContract(unittest.TestCase):
    def read(self, path):
        return (ROOT / path).read_text(encoding="utf-8")

    def test_utf8_lf_and_editor_defaults_are_explicit(self):
        editor = self.read(".editorconfig")
        attributes = self.read(".gitattributes")
        self.assertIn("charset = utf-8", editor)
        self.assertIn("end_of_line = lf", editor)
        self.assertIn("*.md text eol=lf", attributes)
        self.assertIn("*.rs text eol=lf", attributes)
        self.assertIn("*.py text eol=lf", attributes)

    def test_local_evidence_and_credentials_are_ignored(self):
        ignore = self.read(".gitignore")
        for sensitive in ("/target", "/release/evidence/", "/codex-tui-support/",
                          "/.env", "/qualification/"):
            with self.subTest(pattern=sensitive):
                self.assertIn(sensitive, ignore)

    def test_issue_forms_disallow_unsanitized_input(self):
        bug = self.read(".github/ISSUE_TEMPLATE/bug_report.yml")
        self.assertIn("privacy", bug)
        self.assertIn("required: true", bug)
        self.assertIn("Version and source SHA", bug)
        self.assertIn("Windows SSH to Ubuntu", bug)
        proposal = self.read(".github/ISSUE_TEMPLATE/feature_request.yml")
        self.assertIn("Upstream alternatives", proposal)
        self.assertIn("Evidence and acceptance", proposal)
        config = self.read(".github/ISSUE_TEMPLATE/config.yml")
        self.assertIn("SECURITY.md", config)

    def test_pr_template_requires_bilingual_review_and_protected_ci(self):
        pull = self.read(".github/pull_request_template.md")
        self.assertIn("English + 简体中文", pull)
        self.assertIn("Protected PR checks", pull)
        self.assertIn("Stable publishing remains unauthorized", pull)

    def test_private_security_policy_in_both_languages(self):
        for name in ("SECURITY.md", "SECURITY.zh-CN.md"):
            policy = self.read(name)
            self.assertIn("docs-id: security", policy)
            self.assertTrue("private" in policy.lower() or "私下" in policy)


if __name__ == "__main__":
    unittest.main()
