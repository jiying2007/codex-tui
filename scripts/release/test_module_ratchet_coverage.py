import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "check_module_ratchet",
    Path(__file__).resolve().parents[1] / "architecture" / "check_module_ratchet.py",
)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class ModuleRatchetCoverage(unittest.TestCase):
    def write_plan(self, root, ratchet, complete=True):
        plan = {
            "moduleRatchet": ratchet,
            "ratchetPolicy": {"completeSourceCoverage": complete},
        }
        (root / "plan.json").write_text(
            json.dumps(plan),
            encoding="utf-8",
        )

    def test_complete_policy_rejects_untracked_rust_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / "src" / "tracked.rs").write_text("fn tracked() {}\n", encoding="utf-8")
            (root / "src" / "rogue.rs").write_text("fn rogue() {}\n", encoding="utf-8")
            self.write_plan(root, {"src/tracked.rs": 1})
            _, failures, complete = m.inspect_plan(Path("plan.json"), root)
            self.assertTrue(complete)
            self.assertIn(
                "src/rogue.rs: Rust source module is not governed by moduleRatchet",
                failures,
            )

    def test_complete_policy_passes_when_every_source_is_bounded(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / "src" / "a.rs").write_text("a\n", encoding="utf-8")
            (root / "src" / "b.rs").write_text("b\n", encoding="utf-8")
            self.write_plan(root, {"src/a.rs": 1, "src/b.rs": 1})
            rows, failures, complete = m.inspect_plan(Path("plan.json"), root)
            self.assertTrue(complete)
            self.assertEqual(failures, [])
            self.assertEqual(len(rows), 2)

    def test_historical_plan_can_retain_partial_ratchet(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / "src" / "tracked.rs").write_text("x\n", encoding="utf-8")
            (root / "src" / "newer.rs").write_text("y\n", encoding="utf-8")
            self.write_plan(root, {"src/tracked.rs": 1}, complete=False)
            _, failures, complete = m.inspect_plan(Path("plan.json"), root)
            self.assertFalse(complete)
            self.assertEqual(failures, [])


if __name__ == "__main__":
    unittest.main()
