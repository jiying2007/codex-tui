"""Offline Forge evidence I/O controls; no live service or release authority."""
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

import capture_forge_capability as capture
import validate_internal_gitlab as admission

SHA = "0123456789abcdef0123456789abcdef01234567"
SECRET = "Bearer fixture-secret https://user:password@internal.invalid"
NOW = 1_800_000_000_000


def snapshot():
    return {
        "schema": "codex-tui/support-bundle/v1",
        "generatedAtUnixMs": NOW,
        "build": {"productVersion": "1.4.0", "sourceSha": SHA,
                  "os": "linux", "arch": "x86_64"},
        "forge": {
            "provider": "git-lab", "authenticated": True, "clientName": "glab",
            "clientVersion": "1.0", "serverVersion": None, "serverEdition": None,
            "serverTier": None, "freshness": "fresh", "errorCode": None,
            "capabilities": {"issues": "available", "merge-requests": "available",
                             "pipelines": "available", "future-capability": "unknown"},
            "recentIssues": 1, "openChangeRequests": 2,
            "recentPipelines": 3, "issueBoards": 0,
        },
        "ignoredExtension": {"private": SECRET},
    }


class FixtureIO(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.binary = self.root / "fixture-program"
        self.binary.write_text("not a real Codex binary", encoding="utf-8")
        self.output = self.root / "evidence" / "report.json"
        self.stdout = io.StringIO()
        self.stderr = io.StringIO()

    def reset_streams(self):
        for stream in (self.stdout, self.stderr):
            stream.seek(0)
            stream.truncate()

    def assert_no_evidence(self):
        self.assertFalse(self.output.exists())
        self.assertFalse(self.output.parent.exists())
        self.assertEqual(self.stdout.getvalue(), "")

    def assert_redacted(self, error):
        self.assertNotIn(SECRET, str(error) + self.stdout.getvalue() + self.stderr.getvalue())


class ForgeCaptureIO(FixtureIO):
    def doctor(self, value=None, raw=None, hook=None):
        value = snapshot() if value is None else value

        def run(command, **kwargs):
            self.assertEqual(command[:3], [str(self.binary), "doctor", "bundle"])
            self.assertEqual(kwargs["timeout"], 45)
            bundle = Path(command[command.index("--output") + 1])
            bundle.mkdir()
            data = json.dumps(value).encode("utf-8") if raw is None else raw
            (bundle / "snapshot.json").write_bytes(data)
            if hook is not None:
                hook()
            return subprocess.CompletedProcess(command, 0, SECRET, SECRET)
        return run

    def invoke(self, run, args=None):
        self.reset_streams()
        requirements = [
            "--expected-source-sha", SHA, "--expected-provider", "gitlab",
            "--require-authenticated", "--required-capability", "issues=available",
            "--required-capability", "merge-requests=available",
            "--required-capability", "pipelines=available",
        ] if args is None else args
        with mock.patch.object(sys, "argv", [
            "capture_forge_capability.py", "--binary", str(self.binary),
            "--output", str(self.output), *requirements
        ]), mock.patch.object(capture.subprocess, "run", side_effect=run) as child, \
                contextlib.redirect_stdout(self.stdout), contextlib.redirect_stderr(self.stderr):
            self.child = child
            return capture.main()

    def test_healthy_capture_preserves_projection_and_passes_downstream_gate(self):
        self.assertEqual(self.invoke(self.doctor()), 0)
        data = json.loads(self.output.read_bytes())
        self.assertEqual(data, json.loads(self.stdout.getvalue()))
        self.assertEqual(data["forge"]["capabilities"]["future-capability"], "unknown")
        self.assertEqual(data["forge"]["serverVersion"], None)
        self.assertEqual(data["build"]["sourceSha"], SHA)
        self.assertEqual(admission.validate(data, SHA, NOW), [])
        self.assertNotIn(SECRET, self.output.read_text())
        self.child.assert_called_once()

    def test_unmet_requirements_remain_retained_unqualified_diagnostics(self):
        value = snapshot()
        value["forge"]["authenticated"] = False
        value["forge"]["capabilities"]["issues"] = "unavailable"
        self.assertEqual(self.invoke(self.doctor(value)), 3)
        result = json.loads(self.output.read_bytes())
        self.assertFalse(result["qualified"])
        self.assertEqual(len(result["blockers"]), 2)
        self.assertIn("capture-not-qualified", admission.validate(result, SHA, NOW))

    def test_provider_aliases_source_case_and_unknown_build_inspection_remain_supported(self):
        for provider in ("gitlab", "GIT-LAB", "github", "Git-Hub", None):
            with self.subTest(provider=provider):
                value = snapshot()
                value["forge"]["provider"] = provider
                value["build"]["sourceSha"] = " " + SHA.upper() + " "
                self.assertEqual(self.invoke(self.doctor(value), []), 0)
                self.output.unlink()
        value["build"]["sourceSha"] = "unknown"
        self.assertEqual(self.invoke(self.doctor(value), []), 0)
        self.assertEqual(json.loads(self.output.read_bytes())["build"]["sourceSha"], "unknown")

    def test_invalid_metadata_never_produces_qualified_or_partial_output(self):
        cases = [
            (("generatedAtUnixMs",), v) for v in (None, True, 0, -1, "123", 1.5)
        ] + [
            (("build", field), v) for field in ("sourceSha", "os", "arch", "productVersion")
            for v in (None, 7, False, {}, " ")
        ] + [
            (("forge", "provider"), SECRET),
            (("forge", "authenticated"), "true"),
            (("forge", "freshness"), SECRET),
            (("forge", "capabilities"), {"issues": SECRET}),
            (("forge", "capabilities"), {SECRET: "available"}),
            (("forge", "capabilities"), None),
            (("forge", "recentIssues"), True),
            (("forge", "recentPipelines"), -1),
            (("forge", "serverVersion"), {"private": SECRET}),
            (("forge", "errorCode"), SECRET),
        ]
        for i, (path, item) in enumerate(cases):
            with self.subTest(path=path, case=i):
                self.output = self.root / ("invalid-" + str(i)) / "report.json"
                value = snapshot()
                target = value
                for name in path[:-1]:
                    target = target[name]
                target[path[-1]] = item
                with self.assertRaises(SystemExit) as error:
                    self.invoke(self.doctor(value))
                self.assert_redacted(error.exception)
                self.assert_no_evidence()

    def test_invalid_snapshot_bytes_and_shapes_are_controlled_failures(self):
        for raw in (SECRET.encode(), b"\xff", b"[]", b"null", b"true", b"{}", b'{"build":[]}'):
            with self.subTest(raw=raw):
                with self.assertRaises(SystemExit) as error:
                    self.invoke(self.doctor(raw=raw))
                self.assert_redacted(error.exception)
                self.assert_no_evidence()

    def test_missing_snapshot_and_launch_decode_failures_are_redacted(self):
        outcomes = [
            OSError(SECRET), UnicodeDecodeError("utf-8", b"\xff", 0, 1, SECRET),
            subprocess.TimeoutExpired(SECRET, 45, output=SECRET, stderr=SECRET),
            lambda *a, **kw: subprocess.CompletedProcess([], 0, SECRET, SECRET),
            lambda *a, **kw: subprocess.CompletedProcess([], 9, SECRET, SECRET),
        ]
        for outcome in outcomes:
            with self.subTest(outcome=type(outcome).__name__):
                with self.assertRaises(SystemExit) as error:
                    self.invoke(outcome)
                self.assert_redacted(error.exception)
                self.assert_no_evidence()

    def test_existing_output_refuses_before_doctor(self):
        self.output.parent.mkdir()
        self.output.write_bytes(b"original evidence")
        with self.assertRaisesRegex(SystemExit, "refusing to overwrite"):
            self.invoke(self.doctor())
        self.child.assert_not_called()
        self.assertEqual(self.output.read_bytes(), b"original evidence")

    def test_competing_writer_is_preserved_after_doctor(self):
        def compete():
            self.output.parent.mkdir()
            self.output.write_bytes(b"competing evidence")
        with self.assertRaisesRegex(SystemExit, "refusing to overwrite"):
            self.invoke(self.doctor(hook=compete))
        self.assertEqual(self.output.read_bytes(), b"competing evidence")
        self.assertEqual(self.stdout.getvalue(), "")

    @unittest.skipUnless(os.name == "posix", "actual symlink behavior requires POSIX")
    def test_existing_and_dangling_output_symlinks_are_never_followed(self):
        self.output.parent.mkdir()
        for existing in (False, True):
            with self.subTest(existing=existing):
                self.output = self.root / "evidence" / ("link-" + str(existing))
                target = self.root / str(existing)
                if existing:
                    target.write_bytes(b"target evidence")
                self.output.symlink_to(target)
                with self.assertRaisesRegex(SystemExit, "refusing to overwrite"):
                    self.invoke(self.doctor())
                self.child.assert_not_called()
                self.assertTrue(self.output.is_symlink())
                self.assertEqual(target.read_bytes() if existing else target.exists(),
                                 b"target evidence" if existing else False)
                self.output.unlink()

    def test_output_parent_and_write_failures_do_not_echo_paths_or_success(self):
        for operation in ("mkdir", "open"):
            original = getattr(Path, operation)

            def fail(path, *args, **kwargs):
                if path == (self.output.parent if operation == "mkdir" else self.output):
                    raise OSError(SECRET)
                return original(path, *args, **kwargs)

            with self.subTest(operation=operation), mock.patch.object(Path, operation, fail):
                with self.assertRaisesRegex(SystemExit, "cannot retain") as error:
                    self.invoke(self.doctor())
            self.assert_redacted(error.exception)
            self.assertFalse(self.output.exists())
            self.assertEqual(self.stdout.getvalue(), "")

    def test_conflicting_or_malformed_requirements_refuse_before_doctor(self):
        for i, args in enumerate((
            ["--required-capability", "issues=available", "--required-capability", "issues=unknown"],
            ["--required-capability", "issues=" + SECRET],
            ["--required-capability", SECRET + "=available"],
            ["--expected-source-sha", SECRET],
        )):
            with self.subTest(args=args):
                self.output = self.root / ("requirement-" + str(i)) / "report.json"
                with self.assertRaises(SystemExit) as error:
                    self.invoke(self.doctor(), args)
                self.child.assert_not_called()
                self.assert_redacted(error.exception)
                self.assert_no_evidence()

    def test_matching_duplicate_requirements_remain_accepted(self):
        args = ["--required-capability", "issues=available"] * 2
        self.assertEqual(self.invoke(self.doctor(), args), 0)

    def test_actual_child_writes_snapshot_and_streams_are_not_projected(self):
        original = subprocess.run
        value = json.dumps(snapshot())

        def child(command, **kwargs):
            script = (
                "import pathlib,sys; p=pathlib.Path(sys.argv[1]); p.mkdir(); "
                "(p/'snapshot.json').write_text(sys.argv[2],encoding='utf-8'); "
                "print(sys.argv[3]); print(sys.argv[3],file=sys.stderr)"
            )
            return original([sys.executable, "-c", script, command[-1], value, SECRET], **kwargs)
        self.assertEqual(self.invoke(child), 0)
        self.assertEqual(admission.validate(json.loads(self.output.read_bytes()), SHA, NOW), [])
        self.assertNotIn(SECRET, self.stdout.getvalue() + self.stderr.getvalue())


class GitLabAdmissionIO(FixtureIO):
    def setUp(self):
        super().setUp()
        self.input = self.root / "fixture.json"
        self.fixture = {
            "schema": admission.INPUT_SCHEMA, "qualified": True, "blockers": [],
            "observedAtUnixMs": NOW, "build": {"sourceSha": SHA, "os": "linux"},
            "forge": dict(snapshot()["forge"], provider="gitlab"),
            "requirements": {"expectedProvider": "gitlab", "expectedSourceSha": SHA,
                             "authenticated": True, "capabilities": {
                                 name: "available" for name in admission.CAPABILITIES}},
            "privacy": list(admission.PRIVACY),
        }
        self.input.write_bytes((json.dumps(self.fixture, indent=2) + "\r\n").encode("utf-8"))

    def invoke(self):
        self.reset_streams()
        with mock.patch.object(sys, "argv", [
            "validate_internal_gitlab.py", "--fixture", str(self.input),
            "--source-sha", SHA, "--output", str(self.output)
        ]), mock.patch.object(admission.time, "time", return_value=NOW / 1000), \
                contextlib.redirect_stdout(self.stdout), contextlib.redirect_stderr(self.stderr):
            return admission.main()

    def test_success_binds_exact_input_bytes_and_preserves_false_publication_authority(self):
        self.assertEqual(self.invoke(), 0)
        report = json.loads(self.output.read_bytes())
        self.assertEqual(report, json.loads(self.stdout.getvalue()))
        self.assertEqual(report["fixtureSha256"], hashlib.sha256(self.input.read_bytes()).hexdigest())
        self.assertFalse(report["stablePublicationAuthority"])

    def test_unqualified_semantics_still_write_failure_receipt_and_exit_three(self):
        self.fixture["forge"]["authenticated"] = False
        self.input.write_text(json.dumps(self.fixture))
        self.assertEqual(self.invoke(), 3)
        report = json.loads(self.output.read_bytes())
        self.assertFalse(report["qualified"])
        self.assertIn("auth-unconfirmed", report["blockers"])
        self.assertFalse(report["stablePublicationAuthority"])

    def test_existing_receipt_refuses_before_reading_fixture(self):
        self.output.parent.mkdir()
        self.output.write_bytes(b"prior")
        with mock.patch.object(Path, "read_bytes", side_effect=AssertionError("must not read")):
            with self.assertRaisesRegex(SystemExit, "refusing to overwrite"):
                self.invoke()
        self.assertEqual(self.output.read_bytes(), b"prior")

    def test_competing_writer_during_validation_is_preserved(self):
        validate = admission.validate

        def compete(*args):
            self.output.parent.mkdir()
            self.output.write_bytes(b"other admission")
            return validate(*args)
        with mock.patch.object(admission, "validate", side_effect=compete):
            with self.assertRaisesRegex(SystemExit, "refusing to overwrite"):
                self.invoke()
        self.assertEqual(self.output.read_bytes(), b"other admission")
        self.assertEqual(self.stdout.getvalue(), "")

    @unittest.skipUnless(os.name == "posix", "actual symlink behavior requires POSIX")
    def test_dangling_output_symlink_cannot_create_a_target(self):
        self.output.parent.mkdir()
        target = self.root / "missing-target"
        self.output.symlink_to(target)
        with self.assertRaisesRegex(SystemExit, "refusing to overwrite"):
            self.invoke()
        self.assertTrue(self.output.is_symlink())
        self.assertFalse(target.exists())
        self.assertEqual(self.stdout.getvalue(), "")

    def test_malformed_missing_or_non_utf8_input_is_redacted_without_output(self):
        for raw in (SECRET.encode(), b"\xff", None):
            with self.subTest(raw=raw):
                if raw is None:
                    self.input.unlink()
                else:
                    self.input.write_bytes(raw)
                with self.assertRaisesRegex(SystemExit, "cannot read valid") as error:
                    self.invoke()
                self.assert_redacted(error.exception)
                self.assert_no_evidence()

    def test_invalid_observation_values_are_not_echoed_into_failure_receipts(self):
        for i, value in enumerate((SECRET, {"private": SECRET}, True, None, 0, -1)):
            with self.subTest(case=i):
                self.output = self.root / ("invalid-time-" + str(i)) / "report.json"
                self.fixture["observedAtUnixMs"] = value
                self.input.write_text(json.dumps(self.fixture))
                self.assertEqual(self.invoke(), 3)
                report = json.loads(self.output.read_bytes())
                self.assertIsNone(report["observedAtUnixMs"])
                self.assertIn("observation-time-invalid", report["blockers"])
                self.assertFalse(report["stablePublicationAuthority"])
                self.assertNotIn(SECRET, self.stdout.getvalue() + self.output.read_text())

    def test_write_failure_is_redacted_and_has_no_success_summary(self):
        original = Path.open

        def fail(path, *args, **kwargs):
            if path == self.output:
                raise OSError(SECRET)
            return original(path, *args, **kwargs)
        with mock.patch.object(Path, "open", fail):
            with self.assertRaisesRegex(SystemExit, "cannot retain") as error:
                self.invoke()
        self.assert_redacted(error.exception)
        self.assertEqual(self.stdout.getvalue(), "")
        self.assertFalse(self.output.exists())

    def test_fixture_replacement_does_not_change_validated_digest(self):
        raw = self.input.read_bytes()
        original = Path.read_bytes
        reads = []

        def replace_after_read(path):
            data = original(path)
            if path == self.input:
                reads.append(path)
                path.write_text("{}")
            return data
        with mock.patch.object(Path, "read_bytes", replace_after_read):
            self.assertEqual(self.invoke(), 0)
        self.assertEqual(reads, [self.input])
        result = json.loads(self.output.read_bytes())
        self.assertEqual(result["fixtureSha256"], hashlib.sha256(raw).hexdigest())
        self.assertTrue(result["qualified"])


if __name__ == "__main__":
    unittest.main()
