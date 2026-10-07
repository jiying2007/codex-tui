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
    def evidence(self):
        return {
            "schema": "codex-tui/release-evidence/v6",
            "version": "1.4.0",
            "commitSha": "0" * 40,
            "automatedQualification": {
                "schema": "codex-tui/automated-qualification/v3",
                "sourceSha": "0" * 40,
            },
            "realEvidenceBundle": {
                "schema": "codex-tui/stable-real-evidence-bundle/v1",
                "sourceSha": "0" * 40,
                "payloadSha256": "f" * 64,
                "payloadChars": 1024,
                "files": {
                    "linuxCompat": {"sha256": "a" * 64},
                    "linuxTerminal": {"sha256": "b" * 64},
                    "performance": {"sha256": "c" * 64},
                },
            },
        }

    def test_real_bundle_shape_is_independently_validated(self):
        value = self.evidence()
        m.validate_evidence_shape(value, "0" * 40, "1.4.0", "fixture")

        value = self.evidence()
        value["realEvidenceBundle"]["payloadSha256"] = "bad"
        with self.assertRaisesRegex(SystemExit, "payload SHA-256"):
            m.validate_evidence_shape(value, "0" * 40, "1.4.0", "fixture")

        value = self.evidence()
        value["realEvidenceBundle"]["files"].pop("linuxTerminal")
        with self.assertRaisesRegex(SystemExit, "file missing"):
            m.validate_evidence_shape(value, "0" * 40, "1.4.0", "fixture")

    def test_v6_and_real_bundle_are_mandatory_binding_authority(self):
        self.assertEqual(m.EVIDENCE_SCHEMA, "codex-tui/release-evidence/v6")
        self.assertIn("realEvidenceBundle", m.STABLE_BINDING_KEYS)
        self.assertIn("performance", m.STABLE_BINDING_KEYS)
        self.assertIn("terminalRestoration", m.STABLE_BINDING_KEYS)


if __name__ == "__main__":
    unittest.main()
