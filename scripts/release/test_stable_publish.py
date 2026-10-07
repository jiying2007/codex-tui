import pathlib
import subprocess
import unittest
from unittest import mock

import stable_publish as s


def completed(returncode=0, stdout="", stderr=""):
    return subprocess.CompletedProcess(
        args=["gh"],
        returncode=returncode,
        stdout=stdout,
        stderr=stderr,
    )


class StablePublicationInputs(unittest.TestCase):
    def evidence(self):
        return {
            "schema": "codex-tui/release-evidence/v6",
            "canonicalCiRun": 123,
            "realEvidenceBundle": {
                "schema": "codex-tui/stable-real-evidence-bundle/v1",
                "sourceSha": "0" * 40,
                "payloadSha256": "a" * 64,
            },
        }

    def test_publish_reuses_exact_raw_bundle_as_single_evidence_input(self):
        inputs = s.stable_publish_inputs(self.evidence(), 456, "payload")
        self.assertEqual(
            inputs,
            {
                "channel": "stable",
                "publish": True,
                "canonical_ci_run": "123",
                "stable_qualification_run": "456",
                "stable_real_evidence_bundle": "payload",
            },
        )
        self.assertLessEqual(len(inputs), 25)

    def test_dispatch_uses_json_stdin_and_summary_redacts_raw_bundle(self):
        source = (pathlib.Path(__file__).resolve().parent / "stable_publish.py").read_text(
            encoding="utf-8"
        )
        self.assertIn('"--json"', source)
        self.assertIn("stdin_text=json.dumps(workflow_inputs", source)
        self.assertNotIn('command.extend(["-f", f"{key}={value}"])', source)
        self.assertIn('"workflowInputSummary": {', source)
        self.assertIn('"redacted": True', source)
        self.assertNotIn('"workflowInputs": workflow_inputs', source)

    def test_old_schema_missing_bundle_or_empty_payload_fail_closed(self):
        old = self.evidence()
        old["schema"] = "codex-tui/release-evidence/v5"
        with self.assertRaisesRegex(SystemExit, "schema mismatch"):
            s.stable_publish_inputs(old, 456, "payload")

        missing = self.evidence()
        missing.pop("realEvidenceBundle")
        with self.assertRaisesRegex(SystemExit, "realEvidenceBundle"):
            s.stable_publish_inputs(missing, 456, "payload")

        with self.assertRaisesRegex(SystemExit, "must not be empty"):
            s.stable_publish_inputs(self.evidence(), 456, "")

class ImmutableReleasePreflight(unittest.TestCase):
    def test_enabled_repository_passes_and_retains_owner_enforcement(self):
        response = completed(
            stdout='{"enabled":true,"enforced_by_owner":true}'
        )
        with mock.patch.object(s, "run", return_value=response) as runner:
            receipt = s.require_immutable_releases(
                pathlib.Path("/repo"),
                "owner/repo",
            )
        self.assertEqual(receipt["enabled"], True)
        self.assertEqual(receipt["enforcedByOwner"], True)
        self.assertEqual(receipt["repository"], "owner/repo")
        self.assertEqual(receipt["apiVersion"], "2026-03-10")
        command = runner.call_args.args[0]
        self.assertEqual(command[:4], ["gh", "api", "--method", "GET"])
        self.assertIn("X-GitHub-Api-Version: 2026-03-10", command)
        self.assertEqual(command[-1], "repos/owner/repo/immutable-releases")
        self.assertFalse(runner.call_args.kwargs["check"])

    def test_disabled_or_unverifiable_repository_fails_closed(self):
        for response in [
            completed(returncode=1, stderr="HTTP 404"),
            completed(stdout='{"enabled":false,"enforced_by_owner":false}'),
            completed(stdout='{"enforced_by_owner":false}'),
        ]:
            with self.subTest(response=response):
                with mock.patch.object(s, "run", return_value=response):
                    with self.assertRaisesRegex(
                        SystemExit,
                        "immutable releases",
                    ):
                        s.require_immutable_releases(
                            pathlib.Path("/repo"),
                            "owner/repo",
                        )

    def test_malformed_status_fails_closed(self):
        with mock.patch.object(
            s,
            "run",
            return_value=completed(stdout="{"),
        ):
            with self.assertRaisesRegex(
                SystemExit,
                "decode GitHub immutable-releases",
            ):
                s.require_immutable_releases(
                    pathlib.Path("/repo"),
                    "owner/repo",
                )


if __name__ == "__main__":
    unittest.main()
