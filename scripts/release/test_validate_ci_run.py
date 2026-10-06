import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("validate_ci_run.py")
SHA = "0123456789abcdef0123456789abcdef01234567"


def run_validator(payload):
    with tempfile.TemporaryDirectory() as temporary:
        path = Path(temporary) / "run.json"
        path.write_text(json.dumps(payload), encoding="utf-8")
        return subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--run-json",
                str(path),
                "--commit",
                SHA,
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )


def canonical_payload():
    return {
        "id": 77,
        "name": "ci",
        "path": ".github/workflows/ci.yml",
        "event": "push",
        "status": "completed",
        "conclusion": "success",
        "head_branch": "main",
        "head_sha": SHA,
    }


class CanonicalCiRunValidation(unittest.TestCase):
    def test_exact_main_push_run_passes(self):
        result = run_validator(canonical_payload())
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_non_push_run_cannot_impersonate_canonical_main_ci(self):
        payload = canonical_payload()
        payload["event"] = "pull_request"
        result = run_validator(payload)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("workflow event", result.stderr)

    def test_incomplete_run_cannot_impersonate_canonical_main_ci(self):
        payload = canonical_payload()
        payload["status"] = "in_progress"
        result = run_validator(payload)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("status", result.stderr)


if __name__ == "__main__":
    unittest.main()
