"""Bilingual release artifacts preserve current guides and historical v1.0 audits."""
import fnmatch
import json
import re
from pathlib import Path
import tempfile
import unittest

import package_release as package
from archive_doc_links import rewrite_packaged_links, verify_packaged_links
import verify_archive as verify

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = json.loads((ROOT / "docs/i18n/manifest.json").read_text(encoding="utf-8"))


class BilingualReleaseDocumentation(unittest.TestCase):
    def stage(self):
        temp = tempfile.TemporaryDirectory(prefix="bilingual-release-")
        self.addCleanup(temp.cleanup)
        stage = Path(temp.name)
        package.stage_active_bilingual_docs(ROOT, stage)
        return stage

    def test_staged_guides_are_complete_and_reproducibly_scoped(self):
        stage = self.stage()
        expected = {"docs/i18n/manifest.json",
                    "INSTALL-UPGRADE.zh-CN.md", "TEAM-QUICKSTART.zh-CN.md"}
        for pair in MANIFEST["pairs"]:
            expected.add(pair["en"])
            expected.add(pair["zh-CN"])
        actual = {x.relative_to(stage).as_posix() for x in stage.rglob("*") if x.is_file()}
        self.assertEqual(actual, expected)
        verify.verify_active_bilingual_docs(stage, "1.4.0")

    def test_all_current_bundled_pages_have_live_navigation(self):
        stage = self.stage()
        rewrite_packaged_links(stage, ROOT, "a" * 40)
        verify_packaged_links(stage)
        index = (stage / "docs/README.md").read_text(encoding="utf-8")
        self.assertIn(
            "https://github.com/jiying2007/codex-tui/blob/" +
            "a" * 40 + "/docs/design/final-plan.md", index
        )
        self.assertIn("[Team quickstart](team-quickstart.md)", index)

    def test_a_missing_translation_fails_archive_verification(self):
        stage = self.stage()
        (stage / "docs/zh-CN/release/install-upgrade.md").unlink()
        with self.assertRaisesRegex(SystemExit, "missing bilingual document"):
            verify.verify_active_bilingual_docs(stage, "1.4.0")

    def test_a_malformed_translation_fails_archive_verification(self):
        stage = self.stage()
        path = stage / "docs/zh-CN/team-quickstart.md"
        source = path.read_text(encoding="utf-8")
        self.assertIn("<!-- docs-section: daily -->", source)
        path.write_text(source.replace("<!-- docs-section: daily -->", "", 1),
                        encoding="utf-8")
        with self.assertRaisesRegex(SystemExit, "sections mismatch"):
            verify.verify_active_bilingual_docs(stage, "1.4.0")

    def test_a_missing_manifest_identity_is_rejected(self):
        stage = self.stage()
        path = stage / "docs/i18n/manifest.json"
        data = json.loads(path.read_text(encoding="utf-8"))
        data["pairs"] = [p for p in data["pairs"] if p["id"] != "security"]
        path.write_text(json.dumps(data), encoding="utf-8")
        with self.assertRaisesRegex(SystemExit, "required bilingual guide"):
            verify.verify_active_bilingual_docs(stage, "1.4.0")

    def test_legacy_v1_0_archive_policy_is_not_retroactively_rewritten(self):
        with tempfile.TemporaryDirectory() as directory:
            verify.verify_active_bilingual_docs(Path(directory), "1.0.0")

    def test_release_push_trigger_covers_every_current_packaged_pair(self):
        release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertIn("  push:\n", release)
        prefix = release.split("  workflow_dispatch:\n", 1)[0]
        patterns = re.findall(r'(?m)^      - "([^"]+)"$', prefix)
        self.assertIn("scripts/release/**", patterns)
        self.assertIn("scripts/docs/**", patterns)
        self.assertIn("docs/i18n/**", patterns)
        for pair in MANIFEST["pairs"]:
            for locale in ("en", "zh-CN"):
                file = pair[locale]
                with self.subTest(pair=pair["id"], locale=locale):
                    self.assertTrue(
                        any(fnmatch.fnmatchcase(file, pattern) for pattern in patterns),
                        "release push cannot see a packaged documentation change: " + file,
                    )



if __name__ == "__main__":
    unittest.main()
