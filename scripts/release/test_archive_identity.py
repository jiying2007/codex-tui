"""Negative tests run through the distributable verifier, not only its helpers."""
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

import verify_archive as v

SHA = "a" * 40
PLATFORM = {"linux": "linux", "darwin": "macos", "win32": "windows"}[sys.platform]
TRIPLES = {"linux": "x86_64-unknown-linux-gnu", "macos": "aarch64-apple-darwin", "windows": "x86_64-pc-windows-msvc"}

class ArchiveIdentity(unittest.TestCase):
    def verify(self, changes=None, compiled_sha=SHA):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = "codex-tui.exe" if os.name == "nt" else "codex-tui"
            content = b"isolated synthetic binary bytes"
            (root / binary).write_bytes(content)
            for name in ("README.md", "CHANGELOG.md", "INSTALL-UPGRADE.md", "TEAM-QUICKSTART.md", "THIRD_PARTY_NOTICES.txt"):
                (root / name).write_text("fixture", encoding="utf-8")
            (root / "LICENSE").write_text("Apache License Version 2.0", encoding="utf-8")
            (root / "STABLE-CRITERIA.json").write_text(json.dumps({"schema": "codex-tui/stable-criteria/v2", "stableVersion": "1.4.0"}), encoding="utf-8")
            metadata = {"schema": "codex-tui/release-artifact/v1", "version": "1.4.0", "tag": "audit", "commitSha": SHA,
                        "license": "Apache-2.0", "platform": PLATFORM, "hostTriple": TRIPLES[PLATFORM], "binary": binary,
                        "binarySha256": hashlib.sha256(content).hexdigest(), "linuxRuntime": {"minimumGlibc": "2.31"}}
            metadata.update(changes or {})
            (root / "RELEASE-METADATA.json").write_text(json.dumps(metadata), encoding="utf-8")
            (root / "LINUX-ABI.json").write_text(json.dumps({"schema": "codex-tui/linux-abi/v1", "passed": True, "sourceSha": SHA,
                "binarySha256": hashlib.sha256(content).hexdigest(), "maximumGlibc": "2.31", "requiredGlibc": "2.29"}), encoding="utf-8")
            def command(args):
                if "--help" in args: return "Usage: codex-tui"
                if "--version" in args: return "codex-tui 1.4.0"
                if "benchmark" in args: return json.dumps({"schema": "codex-tui/performance/v2", "sourceSha": compiled_sha})
                return json.dumps({"schema": "codex-tui/headless-threads/v1", "degraded": False, "threads": [{}] * 10_000})
            with mock.patch.object(sys, "argv", ["verify_archive.py", "--archive", "fixture.zip", "--version", "1.4.0", "--tag", "audit", "--commit", SHA]), \
                 mock.patch.object(v, "extract", return_value=root), mock.patch.object(v, "run_checked", side_effect=command), \
                 mock.patch.object(v.subprocess, "run", return_value=mock.Mock(returncode=2)):
                return v.main()
    def test_matching_identity_passes(self):
        self.assertEqual(self.verify(), 0)
    def test_unknown_metadata_schema_is_rejected(self):
        with self.assertRaises(SystemExit): self.verify({"schema": "unrecognized"})
    def test_foreign_platform_cannot_skip_native_checks(self):
        with self.assertRaises(SystemExit): self.verify({"platform": "unknown"})
    def test_wrong_triple_is_rejected(self):
        wrong = "windows" if PLATFORM != "windows" else "linux"
        with self.assertRaises(SystemExit): self.verify({"hostTriple": TRIPLES[wrong]})
    def test_metadata_cannot_name_another_binary(self):
        with self.assertRaises(SystemExit): self.verify({"binary": "other-program"})
    def test_binary_digest_is_required_on_every_platform(self):
        with self.assertRaises(SystemExit): self.verify({"binarySha256": "b" * 64})
    def test_metadata_cannot_relabel_an_old_compiled_source(self):
        with self.assertRaises(SystemExit): self.verify(compiled_sha="b" * 40)

if __name__ == "__main__": unittest.main()
