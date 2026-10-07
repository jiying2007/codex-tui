import hashlib
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

import main_protection as mp
import stable_publish as sp


ROOT = pathlib.Path(__file__).resolve().parents[2]


def completed(returncode=0, stdout="", stderr=""):
    return subprocess.CompletedProcess(
        args=["gh"],
        returncode=returncode,
        stdout=stdout,
        stderr=stderr,
    )


def valid_protection():
    return {
        "required_status_checks": {
            "strict": True,
            "contexts": list(mp.REQUIRED_CHECKS),
        },
        "enforce_admins": {"enabled": True},
        "allow_force_pushes": {"enabled": False},
        "allow_deletions": {"enabled": False},
    }


class MainProtectionReceipt(unittest.TestCase):
    def test_valid_policy_retains_exact_snapshot_and_digest(self):
        import json

        raw = json.dumps(valid_protection(), separators=(",", ":")) + "\n"
        response = completed(stdout=raw)
        with tempfile.TemporaryDirectory() as directory:
            snapshot = pathlib.Path(directory) / "main-protection.json"
            runner = mock.Mock(return_value=response)
            receipt = mp.require_main_protection(
                pathlib.Path("/repo"),
                "owner/repo",
                runner=runner,
                snapshot_output=snapshot,
            )

            self.assertEqual(snapshot.read_bytes(), raw.encode("utf-8"))
            self.assertEqual(
                receipt["settingsSnapshotSha256"],
                hashlib.sha256(raw.encode("utf-8")).hexdigest(),
            )
            self.assertEqual(receipt["schema"], "codex-tui/main-protection-state/v1")
            self.assertEqual(receipt["requiredChecks"], list(mp.REQUIRED_CHECKS))
            self.assertEqual(receipt["enforceAdmins"], True)
            self.assertEqual(receipt["allowForcePushes"], False)
            self.assertEqual(receipt["allowDeletions"], False)
            self.assertEqual(
                receipt["authority"],
                "github-rest-main-branch-protection-readback",
            )

            command = runner.call_args.args[0]
            self.assertEqual(command[:4], ["gh", "api", "--method", "GET"])
            self.assertIn("X-GitHub-Api-Version: 2026-03-10", command)
            self.assertEqual(command[-1], "repos/owner/repo/branches/main/protection")
            self.assertFalse(runner.call_args.kwargs["check"])

    def test_unreadable_or_weakened_policy_fails_closed(self):
        import json

        cases = [
            completed(returncode=1, stderr="HTTP 403"),
            completed(stdout="{"),
        ]
        weakened = []

        value = valid_protection()
        value["required_status_checks"]["strict"] = False
        weakened.append(value)

        value = valid_protection()
        value["required_status_checks"]["contexts"] = list(mp.REQUIRED_CHECKS[:-1])
        weakened.append(value)

        value = valid_protection()
        value["enforce_admins"]["enabled"] = False
        weakened.append(value)

        value = valid_protection()
        value["allow_force_pushes"]["enabled"] = True
        weakened.append(value)

        value = valid_protection()
        value["allow_deletions"]["enabled"] = True
        weakened.append(value)

        cases.extend(
            completed(stdout=json.dumps(value))
            for value in weakened
        )

        for response in cases:
            with self.subTest(response=response):
                with self.assertRaises(SystemExit):
                    mp.require_main_protection(
                        pathlib.Path("/repo"),
                        "owner/repo",
                        runner=mock.Mock(return_value=response),
                    )

    def test_stable_publish_uses_shared_policy_check(self):
        response = completed(
            stdout='{"required_status_checks":{"strict":true,"contexts":'
            '["rust-1.88-msrv","ubuntu-24.04","macos-latest","windows-latest"]},'
            '"enforce_admins":{"enabled":true},'
            '"allow_force_pushes":{"enabled":false},'
            '"allow_deletions":{"enabled":false}}'
        )
        with mock.patch.object(sp, "run", return_value=response) as runner:
            receipt = sp.require_main_protection(pathlib.Path("/repo"), "owner/repo")
        self.assertEqual(receipt["schema"], "codex-tui/main-protection-state/v1")
        self.assertEqual(
            runner.call_args.args[0][-1],
            "repos/owner/repo/branches/main/protection",
        )


class MainProtectionWorkflowGate(unittest.TestCase):
    def test_stable_publish_checks_full_policy_at_gate_and_publish_point(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertEqual(text.count("python3 scripts/release/main_protection.py"), 2)
        self.assertIn("qualification/main-protection-state.json", text)
        self.assertIn("qualification/main-protection-snapshot.json", text)
        self.assertIn("gate/main-protection-state.json", text)
        self.assertIn("gate/main-protection-snapshot.json", text)
        self.assertIn("stable-prepublish-main-protection-state.json", text)
        self.assertIn("stable-prepublish-main-protection-snapshot.json", text)

        stable = text.rsplit("\n  publish:\n", 1)[1].split(
            "- name: Publish immutable stable GitHub Release", 1
        )[1].split("- uses: actions/upload-artifact@", 1)[0]
        protection = stable.index("scripts/release/main_protection.py")
        branch = stable.index("stable-prepublish-main-state.json")
        publish = stable.index('gh release edit "$TAG"')
        self.assertLess(protection, branch)
        self.assertLess(branch, publish)

    def test_success_and_partial_evidence_retain_protection_snapshot(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        publish = text.rsplit("\n  publish:\n", 1)[1]
        required = publish.split(
            "- name: Require complete successful publication evidence", 1
        )[1].split("- uses: actions/upload-artifact@", 1)[0]
        for name in (
            "stable-prepublish-main-protection-state.json",
            "stable-prepublish-main-protection-snapshot.json",
        ):
            self.assertIn(name, required)
            self.assertGreaterEqual(publish.count(name), 3)

    def test_local_preflight_reports_canonical_protection_receipt(self):
        text = (ROOT / "scripts/release/stable_publish.py").read_text(encoding="utf-8")
        self.assertIn("main_protection = require_main_protection(root, github_repo)", text)
        self.assertIn('"mainProtection": main_protection', text)


if __name__ == "__main__":
    unittest.main()
