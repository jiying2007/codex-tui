"""Required security checks must report a result for every PR and fail closed."""
import itertools
import json
import os
from pathlib import Path
import subprocess
import textwrap
import unittest

import configure_main_protection as protection


ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / ".github/workflows/security.yml").read_text(encoding="utf-8")


class SecurityGate(unittest.TestCase):
    def test_canonical_criteria_names_every_enforced_check(self):
        criteria = json.loads((ROOT / "release/v1.4-criteria.json").read_text(encoding="utf-8"))
        evidence = next(gate["evidence"] for gate in criteria["requiredGates"]
                        if gate["id"] == "main-protection-integrity")
        for check in protection.REQUIRED_CHECKS:
            with self.subTest(check=check):
                self.assertIn(check, evidence)

    def test_every_pr_and_main_push_can_produce_the_required_result(self):
        protection.validate_security_contract(SOURCE)
        self.assertIn("security-gate", protection.REQUIRED_CHECKS)
        for changed in (
            SOURCE.replace("  pull_request:\n", "  pull_request:\n    paths: ['Cargo.lock']\n"),
            SOURCE.replace("    if: ${{ always() }}\n", ""),
            SOURCE.replace("needs: [cargo-deny, rustsec]", "needs: [rustsec]"),
        ):
            with self.subTest(source=changed):
                with self.assertRaises(SystemExit):
                    protection.validate_security_contract(changed)

    @unittest.skipUnless(os.name == "posix", "the security gate runs in Bash on Ubuntu")
    def test_gate_accepts_only_two_successful_dependencies(self):
        gate = SOURCE.split("\n  security-gate:\n", 1)[1]
        self.assertIn("CARGO_DENY_RESULT: ${{ needs.cargo-deny.result }}", gate)
        self.assertIn("RUSTSEC_RESULT: ${{ needs.rustsec.result }}", gate)
        script = textwrap.dedent(gate.split("        run: |\n", 1)[1])
        states = ("success", "failure", "cancelled", "skipped", "")
        for deny, audit in itertools.product(states, repeat=2):
            env = os.environ.copy()
            env.pop("BASH_ENV", None)
            env.pop("ENV", None)
            env.update(CARGO_DENY_RESULT=deny, RUSTSEC_RESULT=audit)
            result = subprocess.run(
                ["bash", "-e", "-o", "pipefail", "-c", script], env=env,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5,
            )
            with self.subTest(cargo_deny=deny, rustsec=audit):
                self.assertEqual(result.returncode == 0, deny == audit == "success")


if __name__ == "__main__":
    unittest.main()
