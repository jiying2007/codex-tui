"""Offline metadata must never masquerade as a successful external probe."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from diagnostic_fixtures import COMMIT, write_fixtures
from validate_diagnostics import validate_offline_support

ROOT = Path(__file__).resolve().parents[2]
STORE_COUNTS = dict.fromkeys(("operatorPins", "operatorAliases", "operatorDrafts", "markedUnread",
                             "workCards", "scratch", "savedViews", "notes", "bookmarks", "hotSlots",
                             "managedWorktrees", "recentOperationReceipts", "recentForgeReceipts"), 0)


def offline_snapshot():
    components = []
    for ident in ("codex-app-server", "git", "terminal-pty", "gh", "glab"):
        components.append({"id": ident, "requirement": "optional" if ident in ("gh", "glab") else "required",
                           "state": "unavailable", "version": None,
                           "facts": {"probeStatus": "not-probed-offline"}})
    components.append({"id": "sqlite", "requirement": "required", "state": "available",
                       "version": "schema-4", "facts": {"probeStatus": "local-metadata-only"}})
    return {"collectionMode": "offline", "realCodexQualification": False,
            "generatedAtUnixMs": 0,
            "store": {**STORE_COUNTS, "available": True, "schemaVersion": 4, "integrity": "ok"},
            "degradedReasons": ["external-probes-not-run-offline"],
            "errors": {"codes": ["external-probes-not-run-offline"], "window": "bundle-run", "maxEntries": 16},
            "compat": {"readiness": "blocked", "components": components, "requiredDegraded": [],
                       "requiredFailures": ["codex-app-server", "git", "terminal-pty"],
                       "optionalUnavailable": ["gh", "glab"]},
            "git": {"errorCode": "not-probed-offline", "repository": False, "worktree": False,
                    "dirty": False, "changedFiles": 0, "ahead": 0, "behind": 0},
            "forge": {"errorCode": "not-probed-offline", "freshness": "not-probed",
                      "authenticated": None, "capabilities": {}, "recentIssues": 0,
                      "openChangeRequests": 0, "recentPipelines": 0, "issueBoards": 0}}


class OfflineSupport(unittest.TestCase):
    def test_explicit_unavailable_evidence_passes_without_claiming_readiness(self):
        validate_offline_support(offline_snapshot())

    def test_unavailable_local_store_has_no_health_claim(self):
        report = offline_snapshot()
        report["store"] = {**STORE_COUNTS, "available": False, "schemaVersion": None, "integrity": None}
        report["compat"]["requiredFailures"].append("sqlite")
        report["compat"]["components"][-1].update(state="unavailable", version=None)
        validate_offline_support(report)

    def test_forged_mode_readiness_authority_and_missing_components_are_rejected(self):
        mutations = [
            lambda r: r.pop("collectionMode"),
            lambda r: r.update(generatedAtUnixMs=True),
            lambda r: r.update(generatedAtUnixMs=-1),
            lambda r: r["errors"].update(maxEntries=True),
            lambda r: r["errors"].update(maxEntries=-1),
            lambda r: r["store"].update(notes=True),
            lambda r: r["store"].update(notes=-1),
            lambda r: r["store"].pop("notes"),
            lambda r: r.update(collectionMode="live-probes"),
            lambda r: r.update(realCodexQualification=True),
            lambda r: r.update(realCodexQualification=0),
            lambda r: r["compat"].update(readiness="ready"),
            lambda r: r["compat"].update(requiredFailures=[]),
            lambda r: r["compat"].update(optionalUnavailable=[]),
            lambda r: r["compat"]["components"].pop(),
            lambda r: r["compat"]["components"].append(r["compat"]["components"][0]),
            lambda r: r["compat"]["components"][0].update(state="available"),
            lambda r: r["compat"]["components"][0].update(version="real"),
            lambda r: r["compat"]["components"][0].update(facts={"connected": "true"}),
            lambda r: r["compat"]["components"][0].update(requirement="optional"),
            lambda r: r["git"].update(errorCode=None),
            lambda r: r["forge"].update(authenticated=True),
            lambda r: r["forge"].update(capabilities={"write": "available"}),
            lambda r: r["forge"].update(freshness="fresh"),
            lambda r: r["forge"].update(provider="github"),
            lambda r: r["forge"].update(recentIssues=1),
            lambda r: r["git"].update(repository=True),
            lambda r: r["compat"]["components"].append({"id": "extra"}),
            lambda r: r["compat"]["components"][-1].update(state="unavailable"),
            lambda r: r["compat"]["components"][-1].update(version="schema-999"),
            lambda r: r["compat"]["components"][-1].update(requirement="optional"),
            lambda r: r["compat"]["components"][-1].update(facts={}),
            lambda r: r["compat"].update(requiredFailures="codex-app-server git terminal-pty"),
            lambda r: r["compat"].update(optionalUnavailable="gh glab"),
            lambda r: r["compat"].update(requiredDegraded=["codex-app-server"]),
            lambda r: r.update(degradedReasons=[]),
            lambda r: r["errors"].update(codes=[]),
            lambda r: r["store"].update(available=False),
        ]
        for mutate in mutations:
            report = offline_snapshot()
            mutate(report)
            with self.subTest(report=report), self.assertRaises(ValueError):
                validate_offline_support(report)

    def test_automatic_bundle_commands_cannot_silently_return_to_live_probes(self):
        for name in ("development-qualification", "release"):
            workflow = (ROOT / ".github/workflows" / (name + ".yml")).read_text()
            commands = workflow.replace("\\\n", " ").splitlines()
            bundles = [line for line in commands if "./target/release/codex-tui doctor bundle" in line]
            validators = [line for line in commands if "python" in line and "create_automated_qualification.py" in line]
            self.assertEqual(len(bundles), 1, name)
            self.assertEqual(len(validators), 1, name)
            self.assertIn("--offline", bundles[0])
            self.assertIn("--require-offline-support", validators[0])

    def test_offline_gate_refuses_existing_non_offline_support_fixture(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            write_fixtures(output)
            result = subprocess.run([
                sys.executable, str(ROOT / "scripts/release/create_automated_qualification.py"),
                "--output", str(output / "receipt.json"), "--commit", COMMIT,
                "--failure-matrix", str(output / "failure-matrix.json"),
                "--scale", str(output / "scale.json"),
                "--soak", str(output / "soak.json"),
                "--support-manifest", str(output / "support-manifest.json"),
                "--support-snapshot", str(output / "support-snapshot.json"),
                "--require-offline-support",
            ], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("support collection must be explicitly offline", result.stderr)
            self.assertFalse((output / "receipt.json").exists())


if __name__ == "__main__":
    unittest.main()
