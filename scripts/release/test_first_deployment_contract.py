"""Negative fixtures for the first-deployment compatibility boundary."""
from __future__ import annotations
import importlib.util
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
MODULE = ROOT / "scripts/architecture/check_first_deployment.py"
spec = importlib.util.spec_from_file_location("check_first_deployment", MODULE)
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)

class FirstDeploymentTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="first-deployment-")
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name)
        for name in guard.PATHS.values():
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes((ROOT / name).read_bytes())

    def change(self, name, old, new):
        path = self.root / name
        original = path.read_text(encoding="utf-8")
        self.assertIn(old, original)
        path.write_text(original.replace(old, new, 1), encoding="utf-8")

    def test_first_deployment_contract_passes(self):
        self.assertEqual(guard.inspect(self.root), [])

    def test_no_legacy_json_import_reentry(self):
        path = self.root / "src/sqlite_store.rs"
        path.write_text(path.read_text(encoding="utf-8") +
                        "\nfn migrate_legacy_state() {}\n", encoding="utf-8")
        self.assertTrue(any("JSON migration returned" in x for x in guard.inspect(self.root)))

    def test_no_obsolete_sqlite_schema_upgrade(self):
        path = self.root / "src/sqlite_schema.rs"
        path.write_text(path.read_text(encoding="utf-8") +
                        "\nif version == 2 {}\n", encoding="utf-8")
        self.assertTrue(any("SQLite upgrade path returned" in x for x in guard.inspect(self.root)))

    def test_no_obsolete_recovery_admission(self):
        self.change("src/sqlite_store/recovery.rs", "version == DB_SCHEMA_VERSION",
                    "(1..=DB_SCHEMA_VERSION).contains(&version)")
        self.assertTrue(any("old-schema recovery rejection" in x for x in guard.inspect(self.root)))

    def test_no_duplicate_historical_ratchet(self):
        path = self.root / ".github/workflows/ci.yml"
        path.write_text(path.read_text(encoding="utf-8") +
                        "\n# Enforce v1.3 stable-predecessor module ratchet\n", encoding="utf-8")
        self.assertTrue(any("duplicate undeployed numeric LOC" in x for x in guard.inspect(self.root)))

    def test_no_disabled_release_guard(self):
        self.change(".github/workflows/release.yml",
                    "scripts/architecture/check_first_deployment.py",
                    "scripts/architecture/disabled_first_deployment.py")
        self.assertTrue(any("first-deployment qualification guard" in x for x in guard.inspect(self.root)))

    def test_undeployed_qualification_fallback_is_rejected(self):
        path = self.root / "scripts/release/create_development_qualification.py"
        path.write_text(path.read_text(encoding="utf-8") +
                        '\nV13_PLAN_SCHEMA = "obsolete"\n', encoding="utf-8")
        self.assertTrue(any("qualification fallback returned" in x for x in guard.inspect(self.root)))

    def test_historical_executable_qualifier_is_rejected(self):
        path = self.root / "scripts/release/validate_v1_3_plan.py"
        path.write_text("# obsolete helper\n", encoding="utf-8")
        self.assertTrue(any("historical executable qualifier" in x for x in guard.inspect(self.root)))

    def test_no_fabricated_stable_status(self):
        self.change("release/v1.4-completion.json", '"publicationAllowed": false',
                    '"publicationAllowed": true')
        self.assertTrue(any("bypassed publication" in x for x in guard.inspect(self.root)))

if __name__ == "__main__":
    unittest.main()
