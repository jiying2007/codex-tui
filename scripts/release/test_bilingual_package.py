"""Bilingual release artifacts preserve current guides and historical v1.0 audits."""
import json
from pathlib import Path
import tempfile
import unittest

import package_release as package
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

    def test_release_push_trigger_covers_localized_archive_sources(self):
        release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        prefix = release.split("  workflow_dispatch:\n", 1)[0]
        for needed in ('      - "README.zh-CN.md"',
                       '      - "docs/zh-CN/**"',
                       '      - "docs/i18n/**"',
                       '      - "scripts/docs/**"'):
            self.assertIn(needed, prefix)


if __name__ == "__main__":
    unittest.main()
