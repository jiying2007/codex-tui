import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "compare_release_archives",
    Path(__file__).resolve().parent / "compare_release_archives.py",
)
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)


VERSION = "1.4.0"


def write_bundle(root: Path, payloads=None, manifest_changes=None):
    payloads = payloads or {
        "codex-tui-1.4.0-x86_64-unknown-linux-gnu.tar.gz": b"linux",
        "codex-tui-1.4.0-aarch64-apple-darwin.tar.gz": b"macos",
        "codex-tui-1.4.0-x86_64-pc-windows-msvc.zip": b"windows",
    }
    for name, content in payloads.items():
        (root / name).write_bytes(content)
    entries = {
        name: hashlib.sha256(content).hexdigest()
        for name, content in payloads.items()
    }
    if manifest_changes:
        entries.update(manifest_changes)
    (root / "SHA256SUMS").write_text(
        "".join(f"{digest}  {name}\n" for name, digest in sorted(entries.items())),
        encoding="utf-8",
    )


class StablePackageEquivalence(unittest.TestCase):
    def test_matching_native_archives_pass(self):
        with tempfile.TemporaryDirectory() as prior, tempfile.TemporaryDirectory() as current:
            prior_path, current_path = Path(prior), Path(current)
            write_bundle(prior_path)
            write_bundle(current_path)
            rows = c.compare_release_archives(prior_path, current_path, VERSION)
            self.assertEqual(len(rows), 3)
            self.assertTrue(all(len(row["sha256"]) == 64 for row in rows))

    def test_archive_byte_drift_fails_closed(self):
        with tempfile.TemporaryDirectory() as prior, tempfile.TemporaryDirectory() as current:
            prior_path, current_path = Path(prior), Path(current)
            write_bundle(prior_path)
            write_bundle(
                current_path,
                {
                    "codex-tui-1.4.0-x86_64-unknown-linux-gnu.tar.gz": b"linux-drift",
                    "codex-tui-1.4.0-aarch64-apple-darwin.tar.gz": b"macos",
                    "codex-tui-1.4.0-x86_64-pc-windows-msvc.zip": b"windows",
                },
            )
            with self.assertRaisesRegex(SystemExit, "archive bytes drifted"):
                c.compare_release_archives(prior_path, current_path, VERSION)

    def test_corrupt_checksum_manifest_fails_even_when_archive_bytes_match(self):
        with tempfile.TemporaryDirectory() as prior, tempfile.TemporaryDirectory() as current:
            prior_path, current_path = Path(prior), Path(current)
            write_bundle(prior_path)
            write_bundle(current_path, manifest_changes={
                "codex-tui-1.4.0-x86_64-unknown-linux-gnu.tar.gz": "0" * 64,
            })
            with self.assertRaisesRegex(SystemExit, "does not match archive bytes"):
                c.compare_release_archives(prior_path, current_path, VERSION)

    def test_missing_or_extra_native_archive_is_rejected(self):
        with tempfile.TemporaryDirectory() as prior, tempfile.TemporaryDirectory() as current:
            prior_path, current_path = Path(prior), Path(current)
            write_bundle(prior_path)
            write_bundle(current_path)
            (current_path / "codex-tui-1.4.0-x86_64-pc-windows-msvc.zip").unlink()
            with self.assertRaisesRegex(SystemExit, "exactly three native archives"):
                c.compare_release_archives(prior_path, current_path, VERSION)


if __name__ == "__main__":
    unittest.main()
