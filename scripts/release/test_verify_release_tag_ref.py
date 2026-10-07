import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "verify_release_tag_ref",
    Path(__file__).resolve().parent / "verify_release_tag_ref.py",
)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

SHA = "0123456789abcdef0123456789abcdef01234567"


class ReleaseTagRefReceipt(unittest.TestCase):
    def ref(self, **changes):
        value = {
            "ref": "refs/tags/v1.4.0",
            "object": {
                "type": "commit",
                "sha": SHA,
            },
        }
        value.update(changes)
        return value

    def test_exact_lightweight_tag_is_accepted(self):
        receipt = m.validate_tag_ref(self.ref(), "v1.4.0", SHA)
        self.assertEqual(receipt["ref"], "refs/tags/v1.4.0")
        self.assertEqual(receipt["objectType"], "commit")
        self.assertEqual(receipt["objectSha"], SHA)

    def test_wrong_ref_annotated_tag_and_wrong_sha_fail_closed(self):
        with self.assertRaisesRegex(SystemExit, "ref name mismatch"):
            m.validate_tag_ref(
                self.ref(ref="refs/tags/v9.9.9"),
                "v1.4.0",
                SHA,
            )
        with self.assertRaisesRegex(SystemExit, "lightweight commit ref"):
            m.validate_tag_ref(
                self.ref(object={"type": "tag", "sha": SHA}),
                "v1.4.0",
                SHA,
            )
        with self.assertRaisesRegex(SystemExit, "does not match"):
            m.validate_tag_ref(
                self.ref(object={"type": "commit", "sha": "1" * 40}),
                "v1.4.0",
                SHA,
            )

    def test_invalid_object_sha_is_rejected(self):
        with self.assertRaisesRegex(SystemExit, "exactly 40"):
            m.validate_tag_ref(
                self.ref(object={"type": "commit", "sha": "main"}),
                "v1.4.0",
                SHA,
            )

    def test_snapshot_digest_is_exact_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "tag.json"
            path.write_text(json.dumps(self.ref()) + "\n", encoding="utf-8")
            self.assertEqual(
                m.sha256(path),
                hashlib.sha256(path.read_bytes()).hexdigest(),
            )


if __name__ == "__main__":
    unittest.main()
