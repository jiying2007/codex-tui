import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "create_evidence",
    Path(__file__).resolve().parent / "create_evidence.py",
)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

SHA = "0123456789abcdef0123456789abcdef01234567"


def summary():
    return {
        "schema": "codex-tui/stable-real-evidence-summary/v1",
        "bundleSchema": "codex-tui/stable-real-evidence-bundle/v1",
        "sourceSha": SHA,
        "payloadSha256": "f" * 64,
        "payloadChars": 1024,
        "compatSchema": "codex-tui/compat/v2",
        "compatibility": {
            "linux": {
                "status": "ready",
                "sourceSha": SHA,
                "reportSha256": "a" * 64,
                "observedAt": "unix-ms:1",
            }
        },
        "terminalRestoration": {
            "linux": {
                "status": "pass",
                "sourceSha": SHA,
                "receiptSha256": "c" * 64,
                "observedAt": "2026-10-07T00:00:00Z",
            }
        },
        "performance": {
            "platform": "linux",
            "fixture": "resident-planning-10k",
            "sourceSha": SHA,
            "reportSha256": "d" * 64,
            "iterations": 200,
            "p95Ms": 1.0,
            "p99Ms": 2.0,
            "source": "linux:test",
            "observedAt": "unix-ms:1",
        },
        "files": {
            "linuxCompat": {"name": "compat-linux.json", "sha256": "a" * 64, "size": 10},
            "linuxTerminal": {"name": "terminal-linux.json", "sha256": "c" * 64, "size": 10},
            "performance": {"name": "performance-linux.json", "sha256": "d" * 64, "size": 10},
        },
    }


class ReleaseEvidenceV6(unittest.TestCase):
    def test_verified_summary_becomes_bundle_receipt(self):
        receipt = m.validate_real_summary(summary(), SHA)
        self.assertEqual(receipt["schema"], "codex-tui/stable-real-evidence-bundle/v1")
        self.assertEqual(receipt["sourceSha"], SHA)
        self.assertEqual(receipt["payloadSha256"], "f" * 64)
        self.assertEqual(receipt["files"]["linuxCompat"]["sha256"], "a" * 64)

    def test_cross_sha_missing_linux_and_bad_payload_fail_closed(self):
        value = summary()
        value["sourceSha"] = "1" * 40
        with self.assertRaisesRegex(SystemExit, "source SHA mismatch"):
            m.validate_real_summary(value, SHA)

        value = summary()
        value["terminalRestoration"].pop("linux")
        with self.assertRaisesRegex(SystemExit, "Linux terminal evidence is missing"):
            m.validate_real_summary(value, SHA)

        value = summary()
        value["payloadSha256"] = "not-a-digest"
        with self.assertRaisesRegex(SystemExit, "payload SHA-256"):
            m.validate_real_summary(value, SHA)

    def test_summary_hashes_must_match_raw_file_receipts(self):
        value = summary()
        value["compatibility"]["linux"]["reportSha256"] = "b" * 64
        with self.assertRaisesRegex(SystemExit, "compatibility hash"):
            m.validate_real_summary(value, SHA)

        value = summary()
        value["performance"]["p99Ms"] = float("inf")
        with self.assertRaisesRegex(SystemExit, "p99Ms"):
            m.validate_real_summary(value, SHA)

    def test_performance_and_file_receipts_are_strict(self):
        value = summary()
        value["performance"]["iterations"] = 199
        with self.assertRaisesRegex(SystemExit, "iterations"):
            m.validate_real_summary(value, SHA)

        value = summary()
        value["files"]["performance"]["sha256"] = "0" * 63
        with self.assertRaisesRegex(SystemExit, "SHA-256"):
            m.validate_real_summary(value, SHA)


if __name__ == "__main__":
    unittest.main()
