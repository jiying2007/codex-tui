import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "validate_release_branch_state",
    Path(__file__).resolve().parent / "validate_release_branch_state.py",
)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

SHA = "0123456789abcdef0123456789abcdef01234567"


class ReleaseBranchStateReceipt(unittest.TestCase):
    def branch(self, **changes):
        value = {
            "name": "main",
            "commit": {"sha": SHA},
            "protected": True,
        }
        value.update(changes)
        return value

    def test_exact_protected_main_returns_machine_readable_receipt(self):
        receipt = m.validate_branch_state(self.branch(), SHA, True)
        self.assertEqual(receipt["schema"], "codex-tui/release-branch-state/v1")
        self.assertEqual(receipt["sourceSha"], SHA)
        self.assertEqual(receipt["mainSha"], SHA)
        self.assertTrue(receipt["protected"])
        self.assertTrue(receipt["requireProtected"])

    def test_wrong_branch_moved_main_and_missing_protection_fail_closed(self):
        with self.assertRaisesRegex(SystemExit, "must describe main"):
            m.validate_branch_state(self.branch(name="other"), SHA, True)
        with self.assertRaisesRegex(SystemExit, "main moved"):
            m.validate_branch_state(
                self.branch(commit={"sha": "1" * 40}),
                SHA,
                True,
            )
        with self.assertRaisesRegex(SystemExit, "main.protected=true"):
            m.validate_branch_state(self.branch(protected=False), SHA, True)

    def test_unprotected_main_is_allowed_only_when_not_required(self):
        receipt = m.validate_branch_state(self.branch(protected=False), SHA, False)
        self.assertFalse(receipt["protected"])
        self.assertFalse(receipt["requireProtected"])

    def test_snapshot_digest_is_exact_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "main.json"
            path.write_text(json.dumps(self.branch()) + "\n", encoding="utf-8")
            self.assertEqual(
                m.sha256(path),
                hashlib.sha256(path.read_bytes()).hexdigest(),
            )


if __name__ == "__main__":
    unittest.main()
