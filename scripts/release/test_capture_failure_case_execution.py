import unittest
from unittest.mock import patch
from types import SimpleNamespace

import capture_failure_case_execution as execution

COMMIT = "a" * 40


class FailureCaseExecutionTests(unittest.TestCase):
    def matrix(self):
        return {
            "schema": "codex-tui/failure-matrix/v2",
            "cases": [
                {
                    "id": "first",
                    "expected": "degraded",
                    "maxRecoveryMs": 2000,
                    "writesAllowed": False,
                    "evidence": ["module::test_one", "module::test_two"],
                },
                {
                    "id": "second",
                    "expected": "blocked",
                    "maxRecoveryMs": 1000,
                    "writesAllowed": False,
                    "evidence": ["module::test_two"],
                },
            ],
        }

    def test_execution_is_deduplicated_and_never_claims_recovery_slo(self):
        runs = []

        def fake(identifier):
            runs.append(identifier)
            return {
                "identifier": identifier,
                "status": "pass",
                "matchedTests": 1,
                "processWallMs": 1234,
                "outputSha256": "f" * 64,
            }

        output = execution.build_receipt(self.matrix(), COMMIT, run=fake)
        self.assertEqual(runs, ["module::test_one", "module::test_two"])
        self.assertEqual(output["caseCount"], 2)
        self.assertEqual(output["uniqueTestCount"], 2)
        self.assertEqual(output["status"], "tests-executed")
        self.assertFalse(output["cases"][0]["recoveryLatencyMeasured"])
        self.assertEqual(output["cases"][0]["declaredMaxRecoveryMs"], 2000)

    def test_fail_if_identifier_is_absent_or_execution_is_skipped(self):
        with self.assertRaisesRegex(ValueError, "not proven"):
            execution.build_receipt(
                self.matrix(), COMMIT,
                run=lambda identifier: {"status": "pass", "matchedTests": 0},
            )
        broken = self.matrix()
        broken["cases"][0]["evidence"] = ["../../bad"]
        with self.assertRaisesRegex(ValueError, "identifier"):
            execution.build_receipt(broken, COMMIT, run=lambda _: {})

    def test_rust_test_output_must_contain_an_actual_passing_test(self):
        good = SimpleNamespace(
            returncode=0,
            stdout="test result: ok. 1 passed; 0 failed;\n"
                   "test result: ok. 0 passed; 0 failed;\n",
        )
        with patch.object(execution.subprocess, "run", return_value=good):
            receipt = execution.run_one("app_server::tests::fixture")
        self.assertEqual(receipt["matchedTests"], 1)

        empty = SimpleNamespace(
            returncode=0, stdout="test result: ok. 0 passed; 0 failed;",
        )
        with patch.object(execution.subprocess, "run", return_value=empty):
            with self.assertRaisesRegex(ValueError, "did not execute"):
                execution.run_one("app_server::tests::missing")


if __name__ == "__main__":
    unittest.main()
