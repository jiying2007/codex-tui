"""Offline unit cases test the contract, never real GitLab connectivity."""
import importlib.util
from pathlib import Path
import unittest

FILE = Path(__file__).with_name("validate_internal_gitlab.py")
spec = importlib.util.spec_from_file_location("internal_gitlab", FILE)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
SHA = "a" * 40
NOW = 1_800_000_000_000


def sample():
    return {
        "schema": m.INPUT_SCHEMA,
        "qualified": True, "blockers": [],
        "observedAtUnixMs": NOW - 1000,
        "build": {"sourceSha": SHA, "os": "linux"},
        "forge": {"provider": "gitlab", "clientName": "glab",
                  "authenticated": True, "freshness": "fresh", "errorCode": None,
                  "capabilities": {name: "available" for name in m.CAPABILITIES}},
        "requirements": {"expectedProvider": "gitlab", "expectedSourceSha": SHA,
                         "authenticated": True,
                         "capabilities": {name: "available" for name in m.CAPABILITIES}},
        "privacy": list(m.PRIVACY),
    }


class InternalAdmissionTests(unittest.TestCase):
    def test_synthetic_valid_format(self):
        self.assertEqual(m.validate(sample(), SHA, NOW), [])

    def test_partial_or_unqualified_forge_fails_closed(self):
        for name in m.CAPABILITIES:
            example = sample()
            example["forge"]["capabilities"][name] = "unavailable"
            self.assertIn(name + "-not-qualified", m.validate(example, SHA, NOW))
        for field, value in (("authenticated", False), ("freshness", "stale"),
                             ("errorCode", "unavailable"), ("provider", "github")):
            example = sample()
            example["forge"][field] = value
            self.assertTrue(m.validate(example, SHA, NOW))

    def test_missing_capture_requirements_and_privacy(self):
        example = sample()
        example["requirements"]["capabilities"]["issues"] = "unknown"
        self.assertIn("issues-not-qualified", m.validate(example, SHA, NOW))
        example = sample()
        example["privacy"].remove("no-authentication-tokens")
        self.assertIn("privacy-contract-missing", m.validate(example, SHA, NOW))

    def test_wrong_sha_source_and_unqualified_capture(self):
        example = sample()
        example["build"]["sourceSha"] = "b" * 40
        self.assertIn("source-sha-mismatch", m.validate(example, SHA, NOW))
        example = sample()
        example["qualified"] = False
        self.assertIn("capture-not-qualified", m.validate(example, SHA, NOW))

    def test_expired_future_and_invalid_observation(self):
        for observed in (0, True, NOW - m.MAX_AGE_MS - 1, NOW + m.CLOCK_SKEW_MS + 1):
            example = sample()
            example["observedAtUnixMs"] = observed
            self.assertTrue(any(x.startswith("observation-") for x in
                                m.validate(example, SHA, NOW)))


if __name__ == "__main__":
    unittest.main()
