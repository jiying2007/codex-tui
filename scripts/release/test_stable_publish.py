import hashlib
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


class ImmutableReleasePreflight(unittest.TestCase):
    def test_enabled_repository_passes_and_retains_owner_enforcement(self):
        raw = '{"enabled":true,"enforced_by_owner":true}'
        response = completed(stdout=raw)
        with mock.patch.object(s, "run", return_value=response) as runner:
            receipt = s.require_immutable_releases(
                pathlib.Path("/repo"),
                "owner/repo",
            )
        self.assertEqual(receipt["enabled"], True)
        self.assertEqual(receipt["enforcedByOwner"], True)
        self.assertEqual(receipt["repository"], "owner/repo")
        self.assertEqual(receipt["apiVersion"], "2026-03-10")
        self.assertEqual(receipt["schema"], "codex-tui/immutable-releases/v2")
        self.assertEqual(
            receipt["settingsSnapshotSha256"],
            hashlib.sha256(raw.encode("utf-8")).hexdigest(),
        )
        self.assertEqual(
            receipt["authority"],
            "github-rest-immutable-releases-readback",
        )
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
