import hashlib
import json
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

import configure_main_protection as cfg
import main_protection as mp
import stable_publish as sp


ROOT = pathlib.Path(__file__).resolve().parents[2]
SOURCE_SHA = "a" * 40
ACTIONS_APP_ID = 15368


def completed(returncode=0, stdout="", stderr=""):
    return subprocess.CompletedProcess(
        args=["gh"],
        returncode=returncode,
        stdout=stdout,
        stderr=stderr,
    )


def valid_check_runs(app_id=ACTIONS_APP_ID):
    return {
        "check_runs": [
            {
                "name": name,
                "head_sha": SOURCE_SHA,
                "status": "completed",
                "conclusion": "success",
                "app": {
                    "id": app_id,
                    "slug": "github-actions",
                },
            }
            for name in mp.REQUIRED_CHECKS
        ]
    }


def valid_protection(app_id=ACTIONS_APP_ID):
    return {
        "required_status_checks": {
            "strict": True,
            "contexts": list(mp.REQUIRED_CHECKS),
            "checks": [
                {"context": name, "app_id": app_id}
                for name in mp.REQUIRED_CHECKS
            ],
        },
        "enforce_admins": {"enabled": True},
        "allow_force_pushes": {"enabled": False},
        "allow_deletions": {"enabled": False},
    }


def write_check_runs(root: pathlib.Path, payload=None):
    path = root / "check-runs.json"
    raw = json.dumps(
        payload if payload is not None else valid_check_runs(),
        separators=(",", ":"),
    ).encode("utf-8")
    path.write_bytes(raw)
    return path, raw


class MainProtectionConfiguration(unittest.TestCase):
    def test_payload_pins_every_required_check_to_observed_actions_app(self):
        payload = cfg.protection_payload(ACTIONS_APP_ID)
        status = payload["required_status_checks"]
        self.assertEqual(status["contexts"], list(cfg.REQUIRED_CHECKS))
        self.assertEqual(
            status["checks"],
            [
                {"context": name, "app_id": ACTIONS_APP_ID}
                for name in cfg.REQUIRED_CHECKS
            ],
        )
        cfg.validate_applied_protection(
            {
                "required_status_checks": status,
                "enforce_admins": {"enabled": True},
                "allow_force_pushes": {"enabled": False},
                "allow_deletions": {"enabled": False},
            },
            ACTIONS_APP_ID,
        )

    def test_payload_rejects_invalid_app_identity(self):
        for app_id in (0, -1, True, "15368"):
            with self.subTest(app_id=app_id):
                with self.assertRaises(ValueError):
                    cfg.protection_payload(app_id)


class MainProtectionReceipt(unittest.TestCase):
    def test_valid_policy_retains_exact_snapshots_and_digests(self):
        raw = json.dumps(valid_protection(), separators=(",", ":")) + "\n"
        response = completed(stdout=raw)
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            check_runs, check_runs_raw = write_check_runs(root)
            snapshot = root / "main-protection.json"
            runner = mock.Mock(return_value=response)
            receipt = mp.require_main_protection(
                pathlib.Path("/repo"),
                "owner/repo",
                source_sha=SOURCE_SHA,
                check_runs_path=check_runs,
                runner=runner,
                snapshot_output=snapshot,
            )

            self.assertEqual(snapshot.read_bytes(), raw.encode("utf-8"))
            self.assertEqual(
                receipt["settingsSnapshotSha256"],
                hashlib.sha256(raw.encode("utf-8")).hexdigest(),
            )
            self.assertEqual(
                receipt["checkRunsSnapshotSha256"],
                hashlib.sha256(check_runs_raw).hexdigest(),
            )
            self.assertEqual(receipt["schema"], "codex-tui/main-protection-state/v2")
            self.assertEqual(receipt["sourceSha"], SOURCE_SHA)
            self.assertEqual(receipt["githubActionsAppId"], ACTIONS_APP_ID)
            self.assertEqual(receipt["requiredChecks"], list(mp.REQUIRED_CHECKS))
            self.assertEqual(receipt["requiredChecksAppBound"], True)
            self.assertEqual(receipt["enforceAdmins"], True)
            self.assertEqual(receipt["allowForcePushes"], False)
            self.assertEqual(receipt["allowDeletions"], False)
            self.assertEqual(
                receipt["authority"],
                "github-rest-main-protection-and-exact-main-check-runs-readback",
            )

            command = runner.call_args.args[0]
            self.assertEqual(command[:4], ["gh", "api", "--method", "GET"])
            self.assertIn("X-GitHub-Api-Version: 2026-03-10", command)
            self.assertEqual(command[-1], "repos/owner/repo/branches/main/protection")
            self.assertFalse(runner.call_args.kwargs["check"])

    def test_unreadable_or_weakened_policy_fails_closed(self):
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
        value["required_status_checks"]["checks"] = []
        weakened.append(value)

        value = valid_protection()
        value["required_status_checks"]["checks"][0]["app_id"] = -1
        weakened.append(value)

        value = valid_protection()
        value["required_status_checks"]["checks"][0]["app_id"] = ACTIONS_APP_ID + 1
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

        with tempfile.TemporaryDirectory() as directory:
            check_runs, _ = write_check_runs(pathlib.Path(directory))
            for response in cases:
                with self.subTest(response=response):
                    with self.assertRaises(SystemExit):
                        mp.require_main_protection(
                            pathlib.Path("/repo"),
                            "owner/repo",
                            source_sha=SOURCE_SHA,
                            check_runs_path=check_runs,
                            runner=mock.Mock(return_value=response),
                        )

    def test_non_actions_or_inconsistent_check_source_fails_closed(self):
        cases = []
        wrong_slug = valid_check_runs()
        wrong_slug["check_runs"][0]["app"]["slug"] = "foreign-app"
        cases.append(wrong_slug)

        mixed_app = valid_check_runs()
        mixed_app["check_runs"][0]["app"]["id"] = ACTIONS_APP_ID + 1
        cases.append(mixed_app)

        not_success = valid_check_runs()
        not_success["check_runs"][0]["conclusion"] = "failure"
        cases.append(not_success)

        wrong_source = valid_check_runs()
        wrong_source["check_runs"][0]["head_sha"] = "b" * 40
        cases.append(wrong_source)

        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            response = completed(stdout=json.dumps(valid_protection()))
            for payload in cases:
                with self.subTest(payload=payload):
                    check_runs, _ = write_check_runs(root, payload)
                    with self.assertRaises(SystemExit):
                        mp.require_main_protection(
                            pathlib.Path("/repo"),
                            "owner/repo",
                            source_sha=SOURCE_SHA,
                            check_runs_path=check_runs,
                            runner=mock.Mock(return_value=response),
                        )

    def test_stable_publish_uses_shared_source_bound_policy_check(self):
        response = completed(stdout=json.dumps(valid_protection()))
        with tempfile.TemporaryDirectory() as directory:
            check_runs, _ = write_check_runs(pathlib.Path(directory))
            with mock.patch.object(sp, "run", return_value=response) as runner:
                receipt = sp.require_main_protection(
                    pathlib.Path("/repo"),
                    "owner/repo",
                    source_sha=SOURCE_SHA,
                    check_runs_path=check_runs,
                )
        self.assertEqual(receipt["schema"], "codex-tui/main-protection-state/v2")
        self.assertEqual(receipt["githubActionsAppId"], ACTIONS_APP_ID)
        self.assertEqual(
            runner.call_args.args[0][-1],
            "repos/owner/repo/branches/main/protection",
        )


class MainProtectionWorkflowGate(unittest.TestCase):
    def test_stable_publish_checks_app_bound_policy_at_gate_and_publish_point(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertEqual(text.count("python3 scripts/release/main_protection.py"), 2)
        self.assertGreaterEqual(text.count("checks: read"), 2)
        self.assertIn("qualification/main-protection-state.json", text)
        self.assertIn("qualification/main-protection-snapshot.json", text)
        self.assertIn("qualification/main-protection-check-runs.json", text)
        self.assertIn("gate/main-protection-state.json", text)
        self.assertIn("gate/main-protection-snapshot.json", text)
        self.assertIn("gate/main-protection-check-runs.json", text)
        self.assertIn("stable-prepublish-main-protection-state.json", text)
        self.assertIn("stable-prepublish-main-protection-snapshot.json", text)
        self.assertIn("stable-prepublish-main-protection-check-runs.json", text)
        self.assertEqual(text.count("--check-runs-json"), 2)
        self.assertIn('--source-sha "${{ github.sha }}"', text)
        self.assertIn('--source-sha "$GITHUB_SHA"', text)

        stable = text.rsplit("\n  publish:\n", 1)[1].split(
            "- name: Publish immutable stable GitHub Release", 1
        )[1].split("- uses: actions/upload-artifact@", 1)[0]
        check_runs = stable.index("stable-prepublish-main-protection-check-runs.json")
        protection = stable.index("scripts/release/main_protection.py")
        branch = stable.index("stable-prepublish-main-state.json")
        publish = stable.index('gh release edit "$TAG"')
        self.assertLess(check_runs, protection)
        self.assertLess(protection, branch)
        self.assertLess(branch, publish)

    def test_success_and_partial_evidence_retain_source_and_policy_snapshots(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        publish = text.rsplit("\n  publish:\n", 1)[1]
        required = publish.split(
            "- name: Require complete successful publication evidence", 1
        )[1].split("- uses: actions/upload-artifact@", 1)[0]
        for name in (
            "stable-prepublish-main-protection-state.json",
            "stable-prepublish-main-protection-snapshot.json",
            "stable-prepublish-main-protection-check-runs.json",
        ):
            self.assertIn(name, required)
            self.assertGreaterEqual(publish.count(name), 3)

    def test_local_preflight_fetches_exact_main_check_source(self):
        text = (ROOT / "scripts/release/stable_publish.py").read_text(encoding="utf-8")
        self.assertIn('check_runs_json = temp_dir / "main-check-runs.json"', text)
        self.assertIn("check-runs?filter=latest&per_page=100", text)
        self.assertIn("source_sha=commit_sha", text)
        self.assertIn("check_runs_path=check_runs_json", text)
        self.assertIn('"mainProtection": main_protection', text)


if __name__ == "__main__":
    unittest.main()
