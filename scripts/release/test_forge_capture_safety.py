"""Provider capture failure must be bounded and must never expose raw output."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("capture_forge_capability.py")
spec = importlib.util.spec_from_file_location("capture_forge", SCRIPT)
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


class ForgeCaptureFailureTests(unittest.TestCase):
    def invoke(self, outcome):
        with tempfile.TemporaryDirectory() as temp:
            binary = Path(temp) / "fake-codex-tui"
            binary.write_text("fixture", encoding="utf-8")
            output = Path(temp) / "result.json"
            argv = ["capture_forge_capability", "--binary", str(binary),
                    "--output", str(output), "--expected-provider", "gitlab"]
            with patch.object(sys, "argv", argv):
                with patch.object(capture.subprocess, "run", side_effect=outcome) as run:
                    with self.assertRaises(SystemExit) as error:
                        capture.main()
                    self.assertEqual(run.call_args.kwargs["timeout"], 45)
            self.assertFalse(output.exists(), "failure cannot mint a qualified receipt")
            return str(error.exception)

    def test_stalled_doctor_bundle_cannot_hang_or_leak_secret(self):
        secret = "Bearer never-print-this"
        error = self.invoke(subprocess.TimeoutExpired(
            cmd=["codex-tui", "doctor", "bundle"], timeout=45,
            output=secret, stderr=secret
        ))
        self.assertIn("timed out after 45s", error)
        self.assertNotIn(secret, error)

    def test_nonzero_exit_discards_sensitive_stdout_and_stderr(self):
        secret = "glab auth https://alice:password@internal.example"
        process = subprocess.CompletedProcess(
            ["codex-tui", "doctor", "bundle"], 17,
            stdout=secret, stderr=secret
        )
        with patch.object(capture.subprocess, "run", return_value=process) as run:
            error = self.invoke_with_patch(run)
        self.assertIn("exit code 17", error)
        self.assertNotIn(secret, error)

    def invoke_with_patch(self, mock):
        with tempfile.TemporaryDirectory() as temp:
            binary = Path(temp) / "fake-codex-tui"
            binary.write_text("fixture", encoding="utf-8")
            output = Path(temp) / "capture.json"
            with patch.object(sys, "argv", [
                "capture_forge_capability", "--binary", str(binary),
                "--output", str(output)
            ]):
                with self.assertRaises(SystemExit) as error:
                    capture.main()
            self.assertEqual(mock.call_args.kwargs["timeout"], 45)
            self.assertFalse(output.exists())
            return str(error.exception)


if __name__ == "__main__":
    unittest.main()
