"""Negative fixtures prevent bilingual docs and link drift."""
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / "scripts/docs/check_docs.py"
SPEC = importlib.util.spec_from_file_location("check_docs", TOOL)
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)


class BilingualDocsContract(unittest.TestCase):
    def read(self, path):
        return (ROOT / path).read_text(encoding="utf-8")

    def changed(self, path, content):
        return check.inspect(ROOT, {path: content})

    def test_current_active_pair_set_passes(self):
        self.assertEqual(check.inspect(ROOT), [])

    def test_missing_section_fails(self):
        path = "README.zh-CN.md"
        text = self.read(path)
        self.assertIn("<!-- docs-section: status -->", text)
        changed = text.replace("<!-- docs-section: status -->", "", 1)
        self.assertTrue(any("semantic sections drift" in e
                            for e in self.changed(path, changed)))

    def test_wrong_locale_is_detected(self):
        path = "docs/zh-CN/qualification/provider.md"
        changed = self.read(path).replace(
            "<!-- docs-lang: zh-CN -->", "<!-- docs-lang: en -->", 1
        )
        self.assertTrue(any("locale mismatch" in e for e in self.changed(path, changed)))

    def test_broken_counterpart_and_local_target_fail(self):
        path = "README.zh-CN.md"
        text = self.read(path)
        changed = text.replace("[English](README.md)",
                               "[English](this-file-is-missing.md)", 1)
        errors = self.changed(path, changed)
        self.assertTrue(any("reciprocal language link" in e for e in errors))
        self.assertTrue(any("broken local link" in e for e in errors))

    def test_repository_path_escape_is_rejected(self):
        path = "docs/README.md"
        errors = self.changed(path, self.read(path) +
                              "\n[unsafe](../../../../etc/passwd)\n")
        self.assertTrue(any("escapes repository" in e for e in errors))

    def test_duplicate_docs_id_and_unpaired_chinese_rejected(self):
        path = "docs/i18n/manifest.json"
        data = json.loads(self.read(path))
        data["pairs"].append(dict(data["pairs"][0]))
        errors = self.changed(path, json.dumps(data))
        self.assertTrue(any("duplicate docs-id" in e for e in errors))
        data = json.loads(self.read(path))
        data["pairs"] = [p for p in data["pairs"] if p["id"] != "support"]
        errors = self.changed(path, json.dumps(data))
        self.assertTrue(any("unpaired current Chinese" in e for e in errors))

    def test_active_english_and_chinese_must_change_together(self):
        manifest = json.loads(self.read("docs/i18n/manifest.json"))
        self.assertEqual(check.changed_pair_issues(manifest, set()), [])
        self.assertEqual(
            check.changed_pair_issues(manifest, {"README.md", "README.zh-CN.md"}), []
        )
        for only in ({"README.md"}, {"README.zh-CN.md"}):
            errors = check.changed_pair_issues(manifest, only)
            self.assertTrue(any("only one language" in item for item in errors))

    def test_changed_pair_guard_covers_each_manifest_identity(self):
        manifest = json.loads(self.read("docs/i18n/manifest.json"))
        for pair in manifest["pairs"]:
            with self.subTest(id=pair["id"]):
                self.assertTrue(check.changed_pair_issues(manifest, {pair["en"]}))
                self.assertTrue(check.changed_pair_issues(manifest, {pair["zh-CN"]}))
                self.assertEqual(
                    check.changed_pair_issues(
                        manifest, {pair["en"], pair["zh-CN"]}
                    ),
                    [],
                )

    def test_github_event_exact_sha_pinned_git_comparison(self):
        workflow = self.read(".github/workflows/ci.yml")
        self.assertIn("github.event.pull_request.base.sha", workflow)
        self.assertIn("github.event.before", workflow)
        self.assertIn("git -c protocol.version=2 fetch --no-tags --depth=1", workflow)
        self.assertIn('git config --global --add safe.directory "$GITHUB_WORKSPACE"', workflow)
        self.assertNotIn("safe.directory=*", workflow)
        self.assertIn('python scripts/docs/check_docs.py --changed-base "$DOCS_DIFF_BASE"', workflow)
        self.assertNotIn("--changed-base HEAD^", workflow)

    def test_fenced_code_is_not_navigation(self):
        raw = "~~~bash\n[bad](../../missing)\n~~~\n[good](README.md)\n"
        self.assertEqual(list(check.local_links(raw)), ["README.md"])


if __name__ == "__main__":
    unittest.main()
