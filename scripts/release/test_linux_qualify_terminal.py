"""Exercise real qualification orchestration with offline subprocess doubles."""
import contextlib
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import linux_qualify as q

SHA = "0123456789abcdef0123456789abcdef01234567"


def valid_terminal():
    # Test data only, never a real-environment attestation.
    return {"schema": q.TERMINAL_SCHEMA, "platform": "linux", "status": "pass",
            "sourceSha": SHA, "terminal": "offline-fixture",
            "observedAt": "2026-01-01T00:00:00Z"}


class LinuxTerminalPreflight(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.receipt = self.root / "real-terminal.json"
        self.output = self.root / "out"
        (self.root / "LICENSE").write_text("fixture", encoding="utf-8")
        self.commands = []
        self.forbid_work = False
        self.drift = False

    def run_command(self, command, **kwargs):
        self.commands.append(command)
        result = ""
        if command[:2] == ["git", "status"]:
            result = ""
        elif command[:3] == ["git", "rev-parse", "HEAD"]:
            result = SHA
        elif command[:3] == ["git", "branch", "--show-current"]:
            result = "main"
        else:
            if self.forbid_work:
                raise AssertionError("remote/build work ran before terminal rejection")
            if command[:2] == ["gh", "api"]:
                result = json.dumps({"id": 7})
            elif command[:2] == ["cargo", "build"]:
                binary = self.root / "target/release/codex-tui"
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b"offline binary placeholder; never executed")
            elif "scripts/release/capture_compat.py" in command:
                result = json.dumps({"schema": q.COMPAT_SCHEMA, "readiness": "ready",
                                     "sourceSha": SHA, "reportSha256": "a" * 64,
                                     "observedAt": "fixture"})
            elif command[1:3] == ["doctor", "bundle"]:
                directory = Path(command[command.index("--output") + 1])
                directory.mkdir(parents=True)
                for name in ("manifest.json", "snapshot.json"):
                    (directory / name).write_text("{}", encoding="utf-8")
            elif command[1:3] == ["release", "benchmark"]:
                result = json.dumps({"schema": q.PERFORMANCE_SCHEMA,
                                     "fixture": "resident-planning-10k", "sourceSha": SHA,
                                     "iterations": 200, "sampleQualified": True,
                                     "p95Ms": 1, "p99Ms": 2, "source": "fixture",
                                     "observedAt": "fixture"})
            elif command[1:3] == ["release", "verify"]:
                result = '{"valid": true}'
            elif command[:2] == ["git", "ls-remote"]:
                result = ("f" * 40 if self.drift else SHA) + "\trefs/heads/main"
            else:
                result = "{}"
        return subprocess.CompletedProcess(command, 0, result, "")

    def invoke(self, dispatch=False):
        argv = ["linux_qualify.py", "--canonical-ci-run", "7",
                "--github-repo", "owner/repo", "--terminal-receipt", str(self.receipt),
                "--output-dir", str(self.output)]
        if dispatch:
            argv.append("--dispatch")
        with mock.patch.object(sys, "argv", argv), \
             mock.patch.object(sys, "platform", "linux"), \
             mock.patch.object(Path, "cwd", return_value=self.root), \
             mock.patch.object(q, "cargo_package", return_value={"version": "1.4.0", "license": "Apache-2.0"}), \
             mock.patch.object(q, "run", side_effect=self.run_command), \
             contextlib.redirect_stdout(io.StringIO()):
            return q.main()

    def test_bad_receipts_stop_before_network_build_or_output_creation(self):
        self.forbid_work = True
        cases = [None, b"not json", b"\xff", b"[]", b"null"]
        for field, values in (("schema", ["other"]), ("platform", ["windows"]),
                              ("status", ["fail"]), ("sourceSha", ["f" * 40]),
                              ("terminal", [None, 42, {}, "  "]),
                              ("observedAt", [None, True, [], ""])):
            for value in values:
                payload = valid_terminal()
                payload[field] = value
                cases.append(json.dumps(payload).encode())
        for raw in cases:
            with self.subTest(raw=raw):
                if self.receipt.exists():
                    self.receipt.unlink()
                if raw is not None:
                    self.receipt.write_bytes(raw)
                self.commands.clear()
                with self.assertRaisesRegex(SystemExit, "terminal receipt"):
                    self.invoke()
                self.assertTrue(all(command[0] == "git" for command in self.commands))
                self.assertFalse(self.output.exists())

    def test_valid_snapshot_is_byte_exact_and_default_does_not_dispatch(self):
        raw = (json.dumps(valid_terminal(), indent=3) + "\r\n").encode()
        self.receipt.write_bytes(raw)
        self.assertEqual(self.invoke(), 0)
        summary = json.loads((self.output / "qualification-summary.json").read_text())
        retained = Path(summary["terminalReceipt"])
        self.assertNotEqual(retained, self.receipt)
        self.assertEqual(retained.read_bytes(), raw)
        self.assertEqual(summary["terminalReceiptSha256"], hashlib.sha256(raw).hexdigest())
        self.assertEqual(summary["workflowInputs"]["publish"], "false")
        self.assertFalse(any(c[:2] == ["gh", "workflow"] for c in self.commands))

    def test_input_replacement_cannot_change_validated_digest_or_retained_bytes(self):
        good = json.dumps(valid_terminal()).encode()
        bad = b'{"status":"fail"}'
        self.receipt.write_bytes(good)
        reads = []
        read_bytes, read_text = Path.read_bytes, Path.read_text

        def bytes_then_replace(path, *args, **kwargs):
            value = read_bytes(path, *args, **kwargs)
            if path == self.receipt:
                reads.append("bytes")
                path.write_bytes(bad)
            return value

        def text_then_replace(path, *args, **kwargs):
            value = read_text(path, *args, **kwargs)
            if path == self.receipt:
                reads.append("text")
                path.write_bytes(bad)
            return value

        with mock.patch.object(Path, "read_bytes", bytes_then_replace), \
             mock.patch.object(Path, "read_text", text_then_replace):
            self.assertEqual(self.invoke(), 0)
        summary = json.loads((self.output / "qualification-summary.json").read_text())
        self.assertEqual(summary["terminalReceiptSha256"], hashlib.sha256(good).hexdigest())
        self.assertEqual(Path(summary["terminalReceipt"]).read_bytes(), good)
        self.assertEqual(reads, ["bytes"])
        self.assertEqual(self.receipt.read_bytes(), bad)
        evidence_command = next(c for c in self.commands if "scripts/release/create_evidence.py" in c)
        index = evidence_command.index("--linux-terminal-sha256") + 1
        self.assertEqual(evidence_command[index], hashlib.sha256(good).hexdigest())

    def test_retained_snapshot_is_never_overwritten(self):
        self.receipt.write_text(json.dumps(valid_terminal()))
        self.output.mkdir()
        retained = self.output / "terminal-restoration.json"
        retained.write_bytes(b"existing evidence")
        self.forbid_work = True
        with self.assertRaisesRegex(SystemExit, "fresh --output-dir"):
            self.invoke()
        self.assertEqual(retained.read_bytes(), b"existing evidence")

    def test_explicit_dispatch_remains_nonpublishing(self):
        self.receipt.write_text(json.dumps(valid_terminal()))
        self.assertEqual(self.invoke(dispatch=True), 0)
        commands = [c for c in self.commands if c[:2] == ["gh", "workflow"]]
        self.assertEqual(len(commands), 1)
        self.assertIn("publish=false", commands[0])
        self.assertNotIn("publish=true", commands[0])

    def test_main_drift_still_prevents_dispatch(self):
        self.receipt.write_text(json.dumps(valid_terminal()))
        self.drift = True
        with self.assertRaisesRegex(SystemExit, "origin/main drifted"):
            self.invoke(dispatch=True)
        self.assertFalse(any(c[:2] == ["gh", "workflow"] for c in self.commands))

    def test_receipt_source_matching_stays_case_insensitive(self):
        payload = valid_terminal()
        payload["sourceSha"] = SHA.upper()
        self.receipt.write_text(json.dumps(payload))
        self.assertEqual(self.invoke(), 0)


if __name__ == "__main__":
    unittest.main()
