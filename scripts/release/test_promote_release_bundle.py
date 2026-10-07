import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "promote_release_bundle",
    Path(__file__).resolve().parent / "promote_release_bundle.py",
)
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)

VERSION = "1.4.0"
TAG = "v1.4.0"
SHA = "0123456789abcdef0123456789abcdef01234567"


def write_bundle(root: Path):
    payloads = {
        "codex-tui-1.4.0-x86_64-unknown-linux-gnu.tar.gz": b"linux",
        "codex-tui-1.4.0-aarch64-apple-darwin.tar.gz": b"macos",
        "codex-tui-1.4.0-x86_64-pc-windows-msvc.zip": b"windows",
        "RELEASE_NOTES.md": b"notes\n",
        "STABLE-CRITERIA.json": b"{}\n",
        "release-verification.json": json.dumps({
            "schema": "codex-tui/release-verification/v1",
            "channel": "stable",
            "tag": TAG,
            "version": VERSION,
            "commitSha": SHA,
            "publish": False,
            "valid": True,
            "blockers": [],
            "evidenceStatus": "verified",
        }).encode(),
        "release-evidence.json": json.dumps({
            "schema": "codex-tui/release-evidence/v5",
            "version": VERSION,
            "commitSha": SHA,
        }).encode(),
    }
    for name, content in payloads.items():
        (root / name).write_bytes(content)
    (root / "SHA256SUMS").write_text(
        "".join(
            f"{hashlib.sha256(content).hexdigest()}  {name}\n"
            for name, content in sorted(payloads.items())
        ),
        encoding="utf-8",
    )
    return payloads


class StableBundlePromotion(unittest.TestCase):
    def test_exact_qualified_bundle_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payloads = write_bundle(root)
            rows = p.validate_bundle(root, VERSION, TAG, SHA)
            self.assertEqual({row["name"] for row in rows}, set(payloads))

    def test_any_checksummed_byte_drift_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_bundle(root)
            (root / "RELEASE_NOTES.md").write_bytes(b"changed\n")
            with self.assertRaisesRegex(SystemExit, "SHA256SUMS mismatch"):
                p.validate_bundle(root, VERSION, TAG, SHA)

    def test_unchecksummed_extra_file_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_bundle(root)
            (root / "extra.txt").write_text("rogue", encoding="utf-8")
            with self.assertRaisesRegex(SystemExit, "does not exactly cover bundle files"):
                p.validate_bundle(root, VERSION, TAG, SHA)

    def test_wrong_source_or_non_dry_run_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_bundle(root)
            verification_path = root / "release-verification.json"
            verification = json.loads(verification_path.read_text(encoding="utf-8"))
            verification["publish"] = True
            verification_path.write_text(json.dumps(verification), encoding="utf-8")
            # Refresh the manifest so this reaches semantic validation rather than byte validation.
            files = [path for path in root.iterdir() if path.name != "SHA256SUMS"]
            (root / "SHA256SUMS").write_text(
                "".join(
                    f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
                    for path in sorted(files, key=lambda value: value.name)
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(SystemExit, "must originate from publish=false"):
                p.validate_bundle(root, VERSION, TAG, SHA)

    def test_missing_native_archive_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_bundle(root)
            missing = root / "codex-tui-1.4.0-x86_64-pc-windows-msvc.zip"
            missing.unlink()
            files = [path for path in root.iterdir() if path.name != "SHA256SUMS"]
            (root / "SHA256SUMS").write_text(
                "".join(
                    f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
                    for path in sorted(files, key=lambda value: value.name)
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(SystemExit, "exactly three native archives"):
                p.validate_bundle(root, VERSION, TAG, SHA)


if __name__ == "__main__":
    unittest.main()
