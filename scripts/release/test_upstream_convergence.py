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
        for policy in self.manifest["maintenanceOnlyCapabilities"].values():
            for name in policy["modules"]:
                path = self.root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("// fixture\n", encoding="utf-8")

    def check(self):
        path = self.root / "release/v1.5-convergence.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(self.manifest), encoding="utf-8")
        return subprocess.run(
            [sys.executable, str(GUARD), str(path.relative_to(self.root))],
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

    def test_dropping_prefix_and_owned_modules_does_not_escape_guard(self):
        policy = self.manifest["maintenanceOnlyCapabilities"]["thread-queue-ui"]
        policy["modulePrefixes"].remove("src/app/queue")
        del policy["modules"]["src/app/queue_editor.rs"]
        del policy["modules"]["src/app/queue_confirmation.rs"]
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("source prefixes drifted", result.stderr)

    def test_increased_local_module_size_fails(self):
        path = self.root / "src/pty.rs"
        ceiling = self.manifest["maintenanceOnlyCapabilities"]["embedded-terminal"][
            "modules"
        ]["src/pty.rs"]
        path.write_text("// row\n" * (ceiling + 1), encoding="utf-8")
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("maintenance-only ceiling", result.stderr)

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
            "modules": {},
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
