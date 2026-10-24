"""Synthetic terminal receipts must never replace retained observations."""
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import create_terminal_receipt as receipt

if os.name == "posix":
    import terminal_smoke as smoke
else:
    smoke = None

SHA = "a" * 40
LABEL = "UNIT-TEST-ONLY-NOT-QUALIFICATION"


class TerminalEvidenceFixture(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.output = self.root / "receipt.json"

    def symlink(self, link, target):
        try:
            link.symlink_to(target)
        except (OSError, NotImplementedError):
            self.skipTest("symbolic link creation is unavailable")


class TerminalReceiptEvidence(TerminalEvidenceFixture):
    def invoke(self, output=None, notes="first observation"):
        argv = ["create_terminal_receipt.py", "--platform", "linux",
                "--terminal", LABEL, "--source-sha", SHA,
                "--output", str(output or self.output), "--notes", notes, "--pass"]
        with mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
            return receipt.main()

    def test_existing_observation_is_preserved_byte_for_byte(self):
        old = b"previous failed observation\r\n"
        self.output.write_bytes(old)
        with self.assertRaisesRegex(SystemExit, "fresh output path"):
            self.invoke()
        self.assertEqual(self.output.read_bytes(), old)

    def test_second_attestation_requires_a_new_path(self):
        self.assertEqual(self.invoke(), 0)
        old = self.output.read_bytes()
        self.assertEqual(json.loads(old)["notes"], "first observation")
        self.assertNotIn(b"\r\n", old)
        with self.assertRaisesRegex(SystemExit, "fresh output path"):
            self.invoke(notes="different observation")
        self.assertEqual(self.output.read_bytes(), old)
        self.assertEqual(self.invoke(self.root / "new" / "receipt.json"), 0)

    def test_existing_symlink_target_is_not_modified(self):
        target = self.root / "prior.json"
        target.write_bytes(b"prior evidence")
        self.symlink(self.output, target)
        with self.assertRaisesRegex(SystemExit, "fresh output path"):
            self.invoke()
        self.assertTrue(self.output.is_symlink())
        self.assertEqual(target.read_bytes(), b"prior evidence")

    def test_dangling_symlink_is_not_followed(self):
        target = self.root / "missing.json"
        self.symlink(self.output, target)
        with self.assertRaisesRegex(SystemExit, "fresh output path"):
            self.invoke()
        self.assertTrue(self.output.is_symlink())
        self.assertFalse(target.exists())

    def test_symlink_is_refused_even_when_exclusive_open_follows_it(self):
        target = self.root / "windows-missing.json"
        self.symlink(self.output, target)
        original_open = Path.open

        def follow_link_on_open(path, *args, **kwargs):
            # Model the Windows runtime observed in CI: "x" follows a dangling
            # symlink and creates its absent target instead of refusing the link.
            return original_open(target if path == self.output else path, *args, **kwargs)

        with mock.patch.object(Path, "open", follow_link_on_open):
            with self.assertRaisesRegex(SystemExit, "fresh output path"):
                self.invoke()
        self.assertTrue(self.output.is_symlink())
        self.assertFalse(target.exists())

    def test_competing_regular_file_is_still_protected_by_exclusive_creation(self):
        original_mkdir = Path.mkdir
        old = b"another collector's retained observation"

        def concurrent_collector(path, *args, **kwargs):
            result = original_mkdir(path, *args, **kwargs)
            if path == self.output.parent:
                # Another collector creates a regular file just before open.
                self.output.write_bytes(old)
            return result

        with mock.patch.object(Path, "mkdir", concurrent_collector):
            with self.assertRaisesRegex(SystemExit, "fresh output path"):
                self.invoke()
        self.assertEqual(self.output.read_bytes(), old)

    def test_rejected_attestation_creates_no_output(self):
        with mock.patch.object(sys, "argv", ["create_terminal_receipt.py",
                "--platform", "linux", "--terminal", LABEL,
                "--source-sha", SHA, "--output", str(self.output)]):
            with self.assertRaisesRegex(SystemExit, "explicit --pass"):
                receipt.main()
        self.assertFalse(self.output.exists())


@unittest.skipUnless(smoke is not None, "terminal_smoke uses Linux termios")
class TerminalSmokeEvidence(TerminalEvidenceFixture):
    def invoke_smoke(self, command, output=None, pending=None):
        pending = pending or self.root / "pending.json"
        argv = ["terminal_smoke.py", command, "--pending", str(pending)]
        if command == "record-pass":
            argv += ["--output", str(output or self.output), "--pass"]
        else:
            argv += ["--terminal-label", LABEL]

        def create(command, **kwargs):
            # Exercise the actual receipt writer while keeping this fixture offline.
            self.assertEqual(command[1], "scripts/release/create_terminal_receipt.py")
            stream = io.StringIO()
            with mock.patch.object(sys, "argv", command[1:]), \
                    contextlib.redirect_stdout(stream):
                status = receipt.main()
            return subprocess.CompletedProcess(command, status, stream.getvalue(), "")

        flags = smoke.termios.ICANON | smoke.termios.ECHO | smoke.termios.ISIG
        with mock.patch.object(sys, "argv", argv), \
                mock.patch.object(sys, "stdin", mock.Mock()), \
                mock.patch.object(Path, "cwd", return_value=self.root), \
                mock.patch.object(smoke, "require_real_tty", return_value="/dev/fixture-tty"), \
                mock.patch.object(smoke, "repo_identity", return_value=SHA), \
                mock.patch.object(smoke.termios, "tcgetattr", return_value=[0, 0, 0, flags]), \
                mock.patch.object(smoke, "run", side_effect=create), \
                contextlib.redirect_stdout(io.StringIO()), \
                contextlib.redirect_stderr(io.StringIO()):
            return smoke.main()

    def test_prepare_does_not_replace_an_existing_pending_manifest(self):
        self.assertEqual(self.invoke_smoke("prepare"), 0)
        pending = self.root / "pending.json"
        original = pending.read_bytes()
        with self.assertRaisesRegex(SystemExit, "fresh output path"):
            self.invoke_smoke("prepare")
        self.assertEqual(pending.read_bytes(), original)
        self.assertTrue(json.loads(original)["humanObservationRequired"])

    def test_prepare_does_not_follow_existing_or_dangling_links(self):
        for exists in (True, False):
            with self.subTest(target_exists=exists):
                target = self.root / f"target-{exists}.json"
                if exists:
                    target.write_bytes(b"existing pending evidence")
                pending = self.root / f"pending-{exists}.json"
                self.symlink(pending, target)
                with self.assertRaisesRegex(SystemExit, "fresh output path"):
                    self.invoke_smoke("prepare", pending=pending)
                self.assertTrue(pending.is_symlink())
                if exists:
                    self.assertEqual(target.read_bytes(), b"existing pending evidence")
                else:
                    self.assertFalse(target.exists())

    def test_record_pass_preserves_an_existing_receipt(self):
        self.invoke_smoke("prepare")
        self.assertEqual(self.invoke_smoke("record-pass"), 0)
        original = self.output.read_bytes()
        with self.assertRaisesRegex(SystemExit, "fresh output path"):
            self.invoke_smoke("record-pass")
        self.assertEqual(self.output.read_bytes(), original)

    def test_record_pass_keeps_the_output_leaf_unresolved(self):
        self.invoke_smoke("prepare")
        for exists in (True, False):
            with self.subTest(target_exists=exists):
                target = self.root / f"target-{exists}.json"
                if exists:
                    target.write_bytes(b"retained terminal evidence")
                output = self.root / f"receipt-{exists}.json"
                self.symlink(output, target)
                with self.assertRaisesRegex(SystemExit, "fresh output path"):
                    self.invoke_smoke("record-pass", output=output)
                self.assertTrue(output.is_symlink())
                if exists:
                    self.assertEqual(target.read_bytes(), b"retained terminal evidence")
                else:
                    self.assertFalse(target.exists())


if __name__ == "__main__":
    unittest.main()
