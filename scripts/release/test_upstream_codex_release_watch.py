"""Offline negative controls for a non-authoritative Codex stable-release watch."""
import json
import unittest
from pathlib import Path
from upstream_codex_release_watch import observe, version


BASE = {"upstreamBaseline": {"repository": "openai/codex", "releaseTag": "rust-v0.162.1"}}


def release(tag, prerelease=False, draft=False):
    return {"tag_name": tag, "prerelease": prerelease, "draft": draft}


class StableWatchTests(unittest.TestCase):
    def test_only_stable_tags_are_candidates(self):
        self.assertEqual(version("rust-v0.162.1"), (0, 162, 1))
        self.assertIsNone(version("rust-v0.163.0-alpha.5"))
        report = observe([
            release("rust-v0.163.0-alpha.5"),
            release("rust-v0.164.0", prerelease=True),
            release("rust-v0.165.0", draft=True),
            release("rust-v0.162.1"),
        ], BASE)
        self.assertEqual(report["status"], "pinned-stable-current")
        self.assertFalse(report["l3Qualified"])
        self.assertFalse(report["stableQualified"])

    def test_new_stable_requires_review_not_qualification(self):
        report = observe([release("rust-v0.163.0"), release("rust-v0.162.1")], BASE)
        self.assertEqual(report["status"], "newer-stable-observed")
        self.assertEqual(report["latestObservedStable"], "rust-v0.163.0")
        self.assertEqual(report["authority"], "public-release-metadata-only")
        self.assertFalse(report["l3Qualified"])

    def test_missing_stable_does_not_claim_current(self):
        report = observe([release("rust-v0.163.0-alpha.5")], BASE)
        self.assertEqual(report["status"], "unverified-no-stable-in-page")
        self.assertIsNone(report["latestObservedStable"])
        self.assertFalse(report["stableQualified"])

    def test_bad_repository_and_bad_response_fail_closed(self):
        with self.assertRaises(ValueError):
            observe([], {"upstreamBaseline": {"repository": "other/repo", "releaseTag": "rust-v0.162.1"}})
        with self.assertRaises(ValueError):
            observe({}, BASE)

    def test_pinned_fixture_metadata_is_present(self):
        path = Path(__file__).resolve().parents[2] / "tests/fixtures/protocol/manifest.json"
        baseline = json.loads(path.read_text(encoding="utf-8"))["upstreamBaseline"]
        self.assertEqual(baseline["repository"], "openai/codex")
        self.assertEqual(baseline["releaseTag"], "rust-v0.162.1")


if __name__ == "__main__":
    unittest.main()
