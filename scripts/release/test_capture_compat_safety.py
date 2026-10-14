"""Compatibility capture orchestration; fixtures are not live qualification."""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import capture_compat as capture

SHA = "0123456789abcdef0123456789abcdef01234567"
SECRET = "Bearer fixture-only-secret https://user:password@internal.invalid"


def report():
    return {"schema": "codex-tui/compat/v2", "sourceSha": SHA,
            "readiness": "ready", "os": "linux", "arch": "x86_64",
            "productVersion": "1.4.0", "generatedAtUnixMs": 1790000000000,
            "components": [], "extension": "preserved"}


class CompatibilityCaptureSafety(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.output = self.root / "evidence" / "compat.json"
        self.refusals = 0
        self.stdout = io.StringIO()
        self.stderr = io.StringIO()

    def process(self, value=None, code=0):
        payload = report() if value is None else value
        return subprocess.CompletedProcess([], code, json.dumps(payload), SECRET)

    def invoke(self, outcome, source=SHA):
        self.stdout.seek(0)
        self.stdout.truncate()
        self.stderr.seek(0)
        self.stderr.truncate()
        argv = ["capture_compat.py", "--binary", "fixture-binary",
                "--output", str(self.output), "--expected-source-sha", source]
        with mock.patch.object(sys, "argv", argv), \
             mock.patch.object(capture.subprocess, "run", side_effect=outcome) as child, \
             contextlib.redirect_stdout(self.stdout), \
             contextlib.redirect_stderr(self.stderr):
            self.child = child
            return capture.main()

    def reject(self, outcome, pattern):
        self.refusals += 1
        self.output = self.root / ("refusal-" + str(self.refusals)) / "compat.json"
        with self.assertRaisesRegex(SystemExit, pattern) as caught:
            self.invoke(outcome)
        visible = str(caught.exception) + self.stdout.getvalue() + self.stderr.getvalue()
        self.assertNotIn(SECRET, visible)
        self.assertFalse(self.output.exists())
        self.assertFalse(self.output.parent.exists())

    def test_invalid_expected_source_refuses_before_child_or_output(self):
        with self.assertRaisesRegex(SystemExit, "40 hexadecimal"):
            self.invoke(lambda *a, **kw: self.fail("child must not start"), "unknown")
        self.child.assert_not_called()
        self.assertFalse(self.output.parent.exists())

    def test_timeout_has_explicit_budget_and_redacts_captured_output(self):
        self.reject(subprocess.TimeoutExpired([SECRET], 45, output=SECRET, stderr=SECRET),
                    "timed out after 45s")
        self.assertEqual(self.child.call_args.kwargs["timeout"], 45)
        self.assertEqual(self.child.call_args.kwargs["encoding"], "utf-8")

    def test_nonzero_exit_does_not_echo_child_streams(self):
        self.reject(lambda *a, **kw: self.process(SECRET, 3), "exit code 3")
        self.assertEqual(self.stderr.getvalue(), "")

    def test_launch_and_decode_errors_are_redacted(self):
        for error in (OSError(SECRET), UnicodeDecodeError("utf-8", b"\xff", 0, 1, SECRET)):
            with self.subTest(error=type(error).__name__):
                self.reject(error, "could not run or decode")

    def test_invalid_json_and_nonobject_reports_do_not_create_evidence(self):
        for raw in (SECRET, "[]", "null", "true", "17", json.dumps(SECRET)):
            with self.subTest(raw=raw):
                process = subprocess.CompletedProcess([], 0, raw, SECRET)
                self.reject(lambda *a, **kw: process, "valid JSON|JSON object")

    def test_schema_readiness_and_source_failures_do_not_echo_values(self):
        for field in ("schema", "readiness", "sourceSha"):
            with self.subTest(field=field):
                value = report()
                value[field] = SECRET
                self.reject(lambda *a, **kw: self.process(value),
                            "schema|readiness=ready|source SHA mismatch")

    def test_incomplete_build_metadata_and_invalid_time_are_rejected(self):
        cases = [(field, value) for field in ("os", "arch", "productVersion")
                 for value in (None, True, 42, {}, " ")]
        cases += [("generatedAtUnixMs", value)
                  for value in (None, True, 0, -1, "1790000000000", 1.5)]
        for field, value in cases:
            with self.subTest(field=field, value=value):
                payload = report()
                payload[field] = value
                self.reject(lambda *a, **kw: self.process(payload),
                            "nonempty string|positive integer")

    def test_ready_report_has_exact_retained_digest_and_keeps_extensions(self):
        value = report()
        self.assertEqual(self.invoke(lambda *a, **kw: self.process(value)), 0)
        raw = self.output.read_bytes()
        self.assertEqual(raw, (json.dumps(value, sort_keys=True, indent=2) + "\n").encode())
        summary = json.loads(self.stdout.getvalue())
        self.assertEqual(summary["reportSha256"], hashlib.sha256(raw).hexdigest())
        self.assertEqual(summary["reportPath"], str(self.output))
        self.assertEqual(summary["sourceSha"], SHA)
        self.assertEqual(summary["observedAt"], "unix-ms:1790000000000")
        self.assertEqual(self.stderr.getvalue(), "")
        self.child.assert_called_once()
        self.assertEqual(self.child.call_args.args[0],
                         ["fixture-binary", "doctor", "compat", "--json"])

    def test_source_case_and_surrounding_whitespace_remain_compatible(self):
        value = report()
        value["sourceSha"] = " " + SHA.upper() + " "
        self.assertEqual(self.invoke(lambda *a, **kw: self.process(value), SHA.upper()), 0)
        self.assertEqual(json.loads(self.stdout.getvalue())["sourceSha"], SHA)

    def test_existing_report_refuses_before_any_child_execution(self):
        self.output.parent.mkdir()
        self.output.write_bytes(b"retained original")
        with self.assertRaisesRegex(SystemExit, "refusing to overwrite"):
            self.invoke(lambda *a, **kw: self.process())
        self.child.assert_not_called()
        self.assertEqual(self.output.read_bytes(), b"retained original")
        self.assertEqual(self.stdout.getvalue(), "")

    def test_competing_writer_after_preflight_cannot_be_overwritten(self):
        def competing_writer(*args, **kwargs):
            self.output.parent.mkdir()
            self.output.write_bytes(b"another collector")
            return self.process()
        with self.assertRaisesRegex(SystemExit, "refusing to overwrite"):
            self.invoke(competing_writer)
        self.assertEqual(self.output.read_bytes(), b"another collector")
        self.assertEqual(self.stdout.getvalue(), "")

    @unittest.skipUnless(os.name == "posix", "real symlink fixture requires POSIX")
    def test_existing_and_dangling_symlinks_are_not_followed(self):
        self.output.parent.mkdir()
        for exists in (False, True):
            with self.subTest(target_exists=exists):
                target = self.root / ("target-" + str(exists))
                if exists:
                    target.write_bytes(b"never replace this")
                self.output.symlink_to(target)
                with self.assertRaisesRegex(SystemExit, "refusing to overwrite"):
                    self.invoke(lambda *a, **kw: self.process())
                self.child.assert_not_called()
                self.assertTrue(self.output.is_symlink())
                self.assertEqual(target.read_bytes() if exists else target.exists(),
                                 b"never replace this" if exists else False)
                self.output.unlink()

    def test_write_failure_emits_no_success_summary(self):
        original = Path.open
        def cannot_write(path, *args, **kwargs):
            if path == self.output:
                raise OSError(SECRET)
            return original(path, *args, **kwargs)
        with mock.patch.object(Path, "open", cannot_write):
            with self.assertRaisesRegex(SystemExit, "cannot retain") as caught:
                self.invoke(lambda *a, **kw: self.process())
        self.assertNotIn(SECRET, str(caught.exception))
        self.assertEqual(self.stdout.getvalue(), "")
        self.assertFalse(self.output.exists())

    def real_child(self, script):
        run = subprocess.run
        def launch(command, **kwargs):
            self.assertEqual(command, ["fixture-binary", "doctor", "compat", "--json"])
            return run([sys.executable, "-c", script], **kwargs)
        return launch

    def test_real_child_success_retains_observed_report(self):
        script = "print(" + repr(json.dumps(report())) + ")"
        self.assertEqual(self.invoke(self.real_child(script)), 0)
        self.assertEqual(json.loads(self.output.read_bytes()), report())

    def test_real_child_failure_is_redacted_and_does_not_write_report(self):
        script = "import sys; print(" + repr(SECRET) + ", file=sys.stderr); sys.exit(9)"
        self.reject(self.real_child(script), "exit code 9")

    def test_real_sleeping_child_times_out_instead_of_minting_late_evidence(self):
        # The separate mock test pins the production budget to 45 seconds.
        # Shorten only this fixture so native CI need not sleep for 45 seconds.
        script = "import time; time.sleep(1); print(" + repr(json.dumps(report())) + ")"
        with mock.patch.object(capture, "DOCTOR_TIMEOUT_SECONDS", 0.15, create=True):
            self.reject(self.real_child(script), "timed out")


if __name__ == "__main__":
    unittest.main()
