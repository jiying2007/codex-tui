import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "validate_stable_dry_run",
    Path(__file__).resolve().parent / "validate_stable_dry_run.py",
)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class StableDryRunBinding(unittest.TestCase):
    def test_v6_and_real_bundle_are_mandatory_binding_authority(self):
        self.assertEqual(m.EVIDENCE_SCHEMA, "codex-tui/release-evidence/v6")
        self.assertIn("realEvidenceBundle", m.STABLE_BINDING_KEYS)
        self.assertIn("performance", m.STABLE_BINDING_KEYS)
        self.assertIn("terminalRestoration", m.STABLE_BINDING_KEYS)


if __name__ == "__main__":
    unittest.main()
