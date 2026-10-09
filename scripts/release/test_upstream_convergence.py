#!/usr/bin/env python3
"""Fail-closed contract tests for the upstream-convergence architecture guard."""
from __future__ import annotations

import copy
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
GUARD = ROOT / "scripts/architecture/check_upstream_convergence.py"
MANIFEST = json.loads(
    (ROOT / "release/v1.5-convergence.json").read_text(encoding="utf-8")
)


class UpstreamConvergenceGuardTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="convergence-guard-")
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name)
        self.manifest = copy.deepcopy(MANIFEST)
        modules = {
            name
            for policy in self.manifest["maintenanceOnlyCapabilities"].values()
            for name in policy["modules"]
        }
        for name in modules:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("// fixture\n", encoding="utf-8")
        # There is only one numeric ceiling authority, including in fixtures.
        self.ratchet = {
            "moduleRatchet": {name: 1 for name in sorted(modules)},
            "ratchetPolicy": {"completeSourceCoverage": True},
        }

    def check(self):
        manifest_path = self.root / "release/v1.5-convergence.json"
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        manifest_path.write_text(json.dumps(self.manifest), encoding="utf-8")
        ratchet_path = self.root / "release/v1.4-plan.json"
        ratchet_path.write_text(json.dumps(self.ratchet), encoding="utf-8")
        return subprocess.run(
            [sys.executable, str(GUARD), str(manifest_path.relative_to(self.root))],
            cwd=str(self.root),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            universal_newlines=True,
            check=False,
        )

    def test_exact_authority_and_covered_modules_pass(self):
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PASS upstream convergence guard", result.stdout)

    def test_untracked_duplicate_subsystem_file_fails(self):
        path = self.root / "src/app/queue_extra.rs"
        path.write_text("// bypass attempt\n", encoding="utf-8")
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("untracked", result.stderr)

    def test_untracked_managed_worktree_adapter_fails(self):
        path = self.root / "src/worktree_shadow.rs"
        path.write_text("// independent adapter must be reviewed\n", encoding="utf-8")
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("managed-worktree-operations", result.stderr)
        self.assertIn("untracked", result.stderr)

    def test_worktree_guard_prefix_cannot_be_narrowed(self):
        policy = self.manifest["maintenanceOnlyCapabilities"]["managed-worktree-operations"]
        policy["modulePrefixes"].remove("src/runtime_commands")
        policy["modules"].remove("src/runtime_commands.rs")
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("source prefixes drifted", result.stderr)

    def test_worktree_guard_policy_and_duplicate_freeze_cannot_be_weakened(self):
        self.manifest["maintenanceOnlyCapabilities"]["managed-worktree-operations"]["policy"] = "new-features-allowed"
        self.manifest["frozenDuplicateCapabilities"].remove("independent-managed-worktree-authority")
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("maintenance-only policy drifted", result.stderr)
        self.assertIn("frozen duplicate capability set drifted", result.stderr)

    def test_worktree_adapter_growth_fails_under_shared_ratchet(self):
        (self.root / "src/worktree_git.rs").write_text("// expanded\n" * 2, encoding="utf-8")
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("shared module ratchet", result.stderr)
        self.assertIn("no-growth ceiling", result.stderr)

    def test_dropping_prefix_and_owned_modules_does_not_escape_guard(self):
        policy = self.manifest["maintenanceOnlyCapabilities"]["thread-queue-ui"]
        policy["modulePrefixes"].remove("src/app/queue")
        policy["modules"].remove("src/app/queue_editor.rs")
        policy["modules"].remove("src/app/queue_confirmation.rs")
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("source prefixes drifted", result.stderr)

    def test_growth_fails_via_single_shared_ratchet(self):
        (self.root / "src/pty.rs").write_text("// row\n" * 2, encoding="utf-8")
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("shared module ratchet", result.stderr)
        self.assertIn("no-growth ceiling", result.stderr)

    def test_dropping_module_ratchet_coverage_fails(self):
        del self.ratchet["moduleRatchet"]["src/pty.rs"]
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("shared v1.4 module ratchet", result.stderr)

    def test_shared_ratchet_must_stay_complete(self):
        self.ratchet["ratchetPolicy"]["completeSourceCoverage"] = False
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("lost complete source coverage", result.stderr)

    def test_duplicate_module_names_fail(self):
        self.manifest["maintenanceOnlyCapabilities"]["embedded-terminal"][
            "modules"
        ].append("src/pty.rs")
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("distinct source paths", result.stderr)

    def test_legacy_numeric_limits_do_not_reenter_manifest(self):
        modules = self.manifest["maintenanceOnlyCapabilities"]["embedded-terminal"][
            "modules"
        ]
        self.manifest["maintenanceOnlyCapabilities"]["embedded-terminal"]["modules"] = {
            name: 602 for name in modules
        }
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("distinct source paths", result.stderr)

    def test_ratchet_source_drift_fails(self):
        self.manifest["moduleRatchetAuthority"] = "release/other-plan.json"
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("shared module ratchet authority drifted", result.stderr)

    def test_stealing_codex_authority_fails(self):
        self.manifest["authorities"]["codex"].remove("agent-orchestration")
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("upstream authorities drifted", result.stderr)

    def test_policy_weakened_fails(self):
        self.manifest["changePolicy"]["upstreamOverlap"] = "grow-local-layer"
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("changePolicy drifted", result.stderr)

    def test_untracked_manifest_category_fails(self):
        self.manifest["maintenanceOnlyCapabilities"]["agent-runtime"] = {
            "policy": "duplicate",
            "modulePrefixes": [],
            "modules": [],
        }
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("maintenance-only capability set drifted", result.stderr)

    def test_every_qualification_path_enforces_the_guard(self):
        for path in (
            ".github/workflows/ci.yml",
            ".github/workflows/development-qualification.yml",
            ".github/workflows/release.yml",
        ):
            with self.subTest(path=path):
                source = (ROOT / path).read_text(encoding="utf-8")
                self.assertIn(
                    "scripts/architecture/check_upstream_convergence.py", source
                )


if __name__ == "__main__":
    unittest.main()
