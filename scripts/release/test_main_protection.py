import copy
import hashlib
import io
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
            "checks": [
                {"context": name, "app_id": app_id}
                for name in mp.REQUIRED_CHECKS
            ],
        },
        "enforce_admins": {"enabled": True},
        "allow_force_pushes": {"enabled": False},
        "allow_deletions": {"enabled": False},
    }


def protection_snapshot(strong=False):
    value = valid_protection()
    value["required_status_checks"]["contexts"] = list(cfg.REQUIRED_CHECKS)
    value["required_pull_request_reviews"] = None
    value["restrictions"] = None
    value["required_signatures"] = {"enabled": strong}
    for name in cfg.PRESERVED_FLAGS:
        value[name] = {"enabled": strong}
    if strong:
        actors = {
            "users": [{"login": "release-owner", "id": 1}],
            "teams": [{"slug": "maintainers", "id": 2}],
            "apps": [{"slug": "policy-bot", "id": 3}],
        }
        value["required_pull_request_reviews"] = {
            "dismiss_stale_reviews": True,
            "require_code_owner_reviews": True,
            "require_last_push_approval": True,
            "required_approving_review_count": 2,
            "dismissal_restrictions": copy.deepcopy(actors),
            "bypass_pull_request_allowances": copy.deepcopy(actors),
        }
        value["restrictions"] = actors
        value["required_status_checks"]["contexts"].append("independent-attestation")
        value["required_status_checks"]["checks"].append(
            {"context": "independent-attestation", "app_id": 45678}
        )
    return value


def http_response(payload, status=200):
    return completed(
        returncode=0 if status == 200 else 1,
        stdout=f"HTTP/2.0 {status} response\r\nContent-Type: application/json\r\n\r\n" + json.dumps(payload),
    )


class ConfigurationAPI:
    """Only the subprocess boundary is mocked; run the real CLI orchestration."""

    def __init__(self, snapshots, subresources=None, protected=True, branch_shas=None):
        self.snapshots = iter(snapshots)
        self.subresources = subresources or {}
        self.protected = protected
        self.branch_shas = iter(branch_shas) if branch_shas is not None else None
        self.commands = []
        self.puts = []

    def run(self, command, **kwargs):
        self.commands.append(command)
        if command[:2] == ["git", "status"]:
            return completed(stdout="")
        if command[:2] == ["git", "branch"]:
            return completed(stdout="main\n")
        if command[:2] == ["git", "rev-parse"]:
            return completed(stdout=SOURCE_SHA + "\n")
        if command[:2] != ["gh", "api"]:
            raise AssertionError(f"unexpected subprocess: {command}")
        if "PUT" in command:
            self.puts.append(json.loads(kwargs["input_text"]))
            # A successful minimum-only PUT response must not replace a fresh GET.
            return completed(stdout=json.dumps(valid_protection()))
        endpoint = command[-1]
        if endpoint.endswith("/branches/main"):
            sha = next(self.branch_shas) if self.branch_shas is not None else SOURCE_SHA
            return completed(stdout=json.dumps({"protected": self.protected, "commit": {"sha": sha}}))
        if "/check-runs?" in endpoint:
            return completed(stdout=json.dumps(valid_check_runs()))
        if endpoint.endswith("/protection"):
            value = next(self.snapshots)
            return value if isinstance(value, subprocess.CompletedProcess) else http_response(copy.deepcopy(value))
        if "/protection/" in endpoint:
            name = endpoint.rsplit("/", 1)[1]
            return self.subresources.get(name, http_response({"message": "Not Found"}, 404))
        raise AssertionError(f"unexpected API request: {command}")

    def invoke(self, apply=True, confirmation="owner/repo"):
        argv = ["configure_main_protection.py", "--github-repo", "owner/repo"]
        if apply:
            argv.extend(["--apply", "--confirm-repository", confirmation])
        self.output = io.StringIO()
        with mock.patch.object(cfg, "run", side_effect=self.run), mock.patch("sys.argv", argv), mock.patch("sys.stdout", self.output):
            if cfg.main() != 0:
                raise AssertionError("configuration did not return success")
        return json.loads(self.output.getvalue())


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
        payload = cfg.protection_payload(ACTIONS_APP_ID, protection_snapshot())
        status = payload["required_status_checks"]
        self.assertNotIn("contexts", status)
        self.assertEqual(
            status["checks"],
            [
                {"context": name, "app_id": ACTIONS_APP_ID}
                for name in sorted(cfg.REQUIRED_CHECKS)
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
                    cfg.protection_payload(app_id, protection_snapshot())

    def test_security_gate_cannot_be_missing_from_checks_or_protection(self):
        checks = valid_check_runs()
        checks["check_runs"] = [
            check for check in checks["check_runs"] if check["name"] != "security-gate"
        ]
        with self.assertRaisesRegex(SystemExit, "security-gate"):
            cfg.validate_check_runs(checks, SOURCE_SHA)
        policy = valid_protection()
        policy["required_status_checks"]["checks"] = [
            check for check in policy["required_status_checks"]["checks"]
            if check["context"] != "security-gate"
        ]
        with self.assertRaisesRegex(SystemExit, "security-gate"):
            cfg.validate_applied_protection(policy, ACTIONS_APP_ID)


class MainProtectionPreservation(unittest.TestCase):
    def test_apply_adds_gate_without_losing_existing_governance(self):
        before = protection_snapshot(strong=True)
        before["required_status_checks"]["contexts"].remove("security-gate")
        before["required_status_checks"]["checks"] = [
            item for item in before["required_status_checks"]["checks"] if item["context"] != "security-gate"
        ]
        original = copy.deepcopy(before)
        api = ConfigurationAPI([before, before, protection_snapshot(strong=True)])
        plan = api.invoke()
        self.assertEqual(len(api.puts), 1)
        payload = api.puts[0]
        checks = {item["context"]: item["app_id"] for item in payload["required_status_checks"]["checks"]}
        self.assertEqual(checks.pop("independent-attestation"), 45678)
        self.assertEqual(checks, dict.fromkeys(cfg.REQUIRED_CHECKS, ACTIONS_APP_ID))
        reviews = payload["required_pull_request_reviews"]
        self.assertEqual(reviews["required_approving_review_count"], 2)
        for name in ("dismiss_stale_reviews", "require_code_owner_reviews", "require_last_push_approval"):
            self.assertIs(reviews[name], True)
        expected_actors = {"users": ["release-owner"], "teams": ["maintainers"], "apps": ["policy-bot"]}
        self.assertEqual(payload["restrictions"], expected_actors)
        self.assertEqual(reviews["dismissal_restrictions"], expected_actors)
        self.assertEqual(reviews["bypass_pull_request_allowances"], expected_actors)
        for name in cfg.PRESERVED_FLAGS:
            self.assertIs(payload[name], True)
        self.assertNotIn("required_signatures", payload)
        self.assertTrue(plan["existingProtectionPreserved"])
        self.assertTrue(plan["signatureProtectionObserved"])
        self.assertEqual(before, original)
        full_reads = [cmd for cmd in api.commands if cmd[-1].endswith("/protection")]
        self.assertEqual(len(full_reads), 3)

    def test_unreadable_or_unsafe_full_snapshot_never_writes(self):
        missing_flag = protection_snapshot(strong=True)
        del missing_flag["lock_branch"]
        empty_reviews = protection_snapshot(strong=True)
        empty_reviews["required_pull_request_reviews"] = {}
        unknown_control = protection_snapshot(strong=True)
        unknown_control["required_deployments"] = {"enabled": True}
        missing_actor = protection_snapshot(strong=True)
        del missing_actor["restrictions"]["apps"][0]["slug"]
        cases = [
            http_response({"message": "Forbidden"}, 403),
            http_response({"message": "Not Found"}, 404),
            completed(stdout="HTTP/2.0 200 OK\n\n{"),
            missing_flag, empty_reviews, unknown_control, missing_actor,
        ]
        for response in cases:
            with self.subTest(response=response):
                api = ConfigurationAPI([response])
                with self.assertRaises(SystemExit):
                    api.invoke()
                self.assertEqual(api.puts, [])
        api = ConfigurationAPI([http_response({"message": "Not Found"}, 404)], protected=False)
        with self.assertRaises(SystemExit):
            api.invoke()
        self.assertEqual(api.puts, [])

    def test_optional_resource_absence_is_read_and_confirmed_each_time(self):
        before = protection_snapshot()
        for name in ("required_pull_request_reviews", "restrictions", "required_signatures"):
            del before[name]
        api = ConfigurationAPI([before, before, before])
        plan = api.invoke()
        self.assertTrue(plan["existingProtectionPreserved"])
        self.assertFalse(plan["signatureProtectionObserved"])
        self.assertIsNone(api.puts[0]["required_pull_request_reviews"])
        self.assertIsNone(api.puts[0]["restrictions"])
        for name in ("required_pull_request_reviews", "restrictions", "required_signatures"):
            self.assertEqual(sum(cmd[-1].endswith("/protection/" + name) for cmd in api.commands), 3)

    def test_unreadable_optional_resource_never_becomes_disabled(self):
        before = protection_snapshot()
        for status in (403, 429, 500):
            with self.subTest(status=status):
                api = ConfigurationAPI([before], {"required_pull_request_reviews": http_response({"message": "unreadable"}, status)})
                with self.assertRaises(SystemExit):
                    api.invoke()
                self.assertEqual(api.puts, [])

    def test_observed_concurrent_policy_change_refuses_put(self):
        before = protection_snapshot(strong=True)
        changed = copy.deepcopy(before)
        changed["required_pull_request_reviews"]["required_approving_review_count"] = 3
        api = ConfigurationAPI([before, changed])
        with self.assertRaisesRegex(SystemExit, "changed after planning"):
            api.invoke()
        self.assertEqual(api.puts, [])

    def test_observed_main_movement_refuses_put(self):
        before = protection_snapshot(strong=True)
        api = ConfigurationAPI([before, before], branch_shas=[SOURCE_SHA, "b" * 40])
        with self.assertRaisesRegex(SystemExit, "main moved after planning"):
            api.invoke()
        self.assertEqual(api.puts, [])

    def test_post_write_read_failure_does_not_claim_no_write_or_retry(self):
        before = protection_snapshot(strong=True)
        for response in (
            http_response({"message": "Forbidden"}, 403),
            http_response({"message": "server failure"}, 500),
            completed(stdout="HTTP/2.0 200 OK\n\n{"),
        ):
            with self.subTest(response=response):
                api = ConfigurationAPI([before, before, response])
                with self.assertRaises(SystemExit) as failure:
                    api.invoke()
                self.assertEqual(len(api.puts), 1)
                self.assertNotIn("no settings written", str(failure.exception))
                self.assertEqual(api.output.getvalue(), "")

    def test_independent_get_detects_lost_controls_despite_successful_put(self):
        before = protection_snapshot(strong=True)
        for field in ("required_signatures", "required_conversation_resolution"):
            after = copy.deepcopy(before)
            after[field]["enabled"] = False
            with self.subTest(field=field):
                api = ConfigurationAPI([before, before, after])
                with self.assertRaisesRegex(SystemExit, "independent protection readback"):
                    api.invoke()
                self.assertEqual(len(api.puts), 1)
        after = copy.deepcopy(before)
        after["required_status_checks"]["contexts"].remove("independent-attestation")
        after["required_status_checks"]["checks"] = [
            check for check in after["required_status_checks"]["checks"] if check["context"] != "independent-attestation"
        ]
        api = ConfigurationAPI([before, before, after])
        with self.assertRaisesRegex(SystemExit, "independent protection readback"):
            api.invoke()
        self.assertEqual(len(api.puts), 1)

    def test_extra_checks_keep_their_app_binding_and_unbound_semantics(self):
        before = protection_snapshot(strong=True)
        before["required_status_checks"]["checks"][-1]["app_id"] = None
        policy = cfg.protection_payload(ACTIONS_APP_ID, before)
        self.assertEqual(
            next(item for item in policy["required_status_checks"]["checks"] if item["context"] == "independent-attestation"),
            {"context": "independent-attestation", "app_id": -1},
        )
        before["required_status_checks"]["checks"][0]["app_id"] = 98765
        api = ConfigurationAPI([before])
        with self.assertRaisesRegex(SystemExit, "another app"):
            api.invoke()
        self.assertEqual(api.puts, [])

    def test_malformed_check_identity_never_writes(self):
        for mutation in ("missing-app", "contexts-mismatch", "duplicate-context", "boolean-app"):
            before = protection_snapshot(strong=True)
            status = before["required_status_checks"]
            if mutation == "missing-app":
                del status["checks"][0]["app_id"]
            elif mutation == "contexts-mismatch":
                status["contexts"].append("unobserved-check")
            elif mutation == "duplicate-context":
                status["checks"].append(copy.deepcopy(status["checks"][0]))
            else:
                status["checks"][0]["app_id"] = True
            api = ConfigurationAPI([before])
            with self.subTest(mutation=mutation), self.assertRaises(SystemExit):
                api.invoke()
            self.assertEqual(api.puts, [])

    def test_dry_run_and_wrong_confirmation_never_write(self):
        before = protection_snapshot(strong=True)
        api = ConfigurationAPI([before])
        plan = api.invoke(apply=False)
        self.assertFalse(plan["applyRequested"])
        self.assertEqual(plan["schema"], "codex-tui/main-protection-plan/v2")
        self.assertEqual(len(plan["observedPolicySha256"]), 64)
        self.assertEqual(api.puts, [])
        api = ConfigurationAPI([before])
        with self.assertRaisesRegex(SystemExit, "confirm-repository"):
            api.invoke(confirmation="another/repo")
        self.assertEqual(api.puts, [])


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

        boolean_app = valid_check_runs()
        boolean_app["check_runs"][0]["app"]["id"] = True
        cases.append(boolean_app)

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
