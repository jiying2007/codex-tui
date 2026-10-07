import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "verify_release_assets",
    Path(__file__).resolve().parent / "verify_release_assets.py",
)
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)


def release_for(payloads, changes=None):
    assets = []
    for index, (name, content) in enumerate(sorted(payloads.items()), start=1):
        assets.append(
            {
                "id": index,
                "name": name,
                "state": "uploaded",
                "size": len(content),
                "digest": "sha256:" + hashlib.sha256(content).hexdigest(),
            }
        )
    release = {
        "id": 10,
        "tag_name": "v1.4.0",
        "target_commitish": "0123456789abcdef0123456789abcdef01234567",
        "published_at": None,
        "draft": True,
        "prerelease": False,
        "immutable": False,
        "assets": assets,
    }
    release.update(changes or {})
    return release


class ReleaseAssetVerification(unittest.TestCase):
    def bundle(self, root):
        payloads = {
            "codex-tui-linux.tar.gz": b"linux",
            "codex-tui-windows.zip": b"windows",
            "SHA256SUMS": b"manifest",
        }
        for name, content in payloads.items():
            (root / name).write_bytes(content)
        return payloads

    def test_matching_remote_assets_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payloads = self.bundle(root)
            rows = v.verify(release_for(payloads), root, "v1.4.0")
            self.assertEqual([row["name"] for row in rows], sorted(payloads))

    def test_missing_or_extra_remote_asset_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payloads = self.bundle(root)
            release = release_for(payloads)
            release["assets"].pop()
            with self.assertRaisesRegex(SystemExit, "asset set does not exactly match"):
                v.verify(release, root, "v1.4.0")

    def test_remote_digest_or_size_drift_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payloads = self.bundle(root)
            for field, value, message in (
                ("digest", "sha256:" + "0" * 64, "SHA-256 mismatch"),
                ("size", 999, "size mismatch"),
            ):
                release = release_for(payloads)
                release["assets"][0][field] = value
                with self.subTest(field=field):
                    with self.assertRaisesRegex(SystemExit, message):
                        v.verify(release, root, "v1.4.0")

    def test_invalid_state_missing_digest_and_duplicate_name_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payloads = self.bundle(root)
            release = release_for(payloads)
            release["assets"][0]["state"] = "new"
            with self.assertRaisesRegex(SystemExit, "not uploaded"):
                v.verify(release, root, "v1.4.0")

            release = release_for(payloads)
            release["assets"][0]["digest"] = None
            with self.assertRaisesRegex(SystemExit, "digest is missing or invalid"):
                v.verify(release, root, "v1.4.0")

            release = release_for(payloads)
            release["assets"].append(dict(release["assets"][0]))
            with self.assertRaisesRegex(SystemExit, "duplicate asset name"):
                v.verify(release, root, "v1.4.0")

            release = release_for(payloads)
            release["assets"][0]["id"] = True
            with self.assertRaisesRegex(SystemExit, "asset id is missing or invalid"):
                v.verify(release, root, "v1.4.0")

    def test_phase_and_channel_state_are_fail_closed(self):
        payloads = {
            "asset": b"bytes",
        }

        draft = release_for(payloads)
        v.validate_release_state(draft, "draft", "preview")
        v.validate_release_state(draft, "draft", "stable")
        v.validate_release_state(draft, "prepublish", "stable")

        with self.assertRaisesRegex(SystemExit, "stable-only"):
            v.validate_release_state(draft, "prepublish", "preview")

        published_preview = release_for(
            payloads,
            {"draft": False, "prerelease": True, "immutable": False},
        )
        v.validate_release_state(published_preview, "published", "preview")
        with self.assertRaisesRegex(SystemExit, "prerelease=false"):
            v.validate_release_state(published_preview, "published", "stable")

        published_stable = release_for(
            payloads,
            {"draft": False, "prerelease": False, "immutable": True},
        )
        v.validate_release_state(published_stable, "published", "stable")

        for changes, message in (
            ({"draft": True, "prerelease": False, "immutable": True}, "draft=false"),
            ({"draft": False, "prerelease": False, "immutable": False}, "immutable=true"),
        ):
            release = release_for(payloads, changes)
            with self.subTest(changes=changes):
                with self.assertRaisesRegex(SystemExit, message):
                    v.validate_release_state(release, "published", "stable")

    def test_release_id_and_boolean_states_are_required(self):
        payloads = {"asset": b"bytes"}
        for changes, message in (
            ({"id": None}, "release id"),
            ({"id": True}, "release id"),
            ({"draft": "false"}, "draft state"),
            ({"prerelease": None}, "prerelease state"),
        ):
            release = release_for(payloads, changes)
            with self.subTest(changes=changes):
                with self.assertRaisesRegex(SystemExit, message):
                    v.validate_release_state(release, "draft", "preview")

    def test_receipt_schema_v2_and_release_target_identity(self):
        self.assertEqual(v.SCHEMA, "codex-tui/release-asset-verification/v2")
        sha = "0123456789abcdef0123456789abcdef01234567"

        draft = release_for({"asset": b"bytes"})
        v.validate_release_identity(draft, sha, "draft")

        published = release_for(
            {"asset": b"bytes"},
            {
                "draft": False,
                "prerelease": False,
                "immutable": True,
                "published_at": "2026-10-07T00:00:00Z",
            },
        )
        v.validate_release_identity(published, sha, "published")

        for changes, message in (
            ({"target_commitish": "main"}, "exact 40-character source SHA"),
            (
                {"target_commitish": "1123456789abcdef0123456789abcdef01234567"},
                "does not match",
            ),
            ({"published_at": None, "draft": False}, "must have published_at"),
        ):
            release = release_for({"asset": b"bytes"}, changes)
            with self.subTest(changes=changes):
                with self.assertRaisesRegex(SystemExit, message):
                    v.validate_release_identity(release, sha, "published")

    def test_tag_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payloads = self.bundle(root)
            with self.assertRaisesRegex(SystemExit, "tag mismatch"):
                v.verify(release_for(payloads), root, "v9.9.9")


if __name__ == "__main__":
    unittest.main()
