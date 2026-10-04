import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("branch_hygiene", Path(__file__).resolve().parents[1] / "repository/branch_hygiene.py")
h = importlib.util.module_from_spec(spec)
spec.loader.exec_module(h)

class BranchHygieneContract(unittest.TestCase):
    def plan(self, name="fix/old", sha="a" * 40, protected=False, state="closed", reachable=True):
        pr = {"number": 1, "state": state, "merged_at": "date", "head": {"ref": name, "sha": "a" * 40, "repo": {"full_name": "owner/repo"}}, "base": {"ref": "main"}}
        branch = {"name": name, "commit": {"sha": sha}, "protected": protected}
        return h.build_plan("owner/repo", "b" * 40, [branch], [pr], lambda *_: reachable)["entries"][0]
    def test_exact_merged_reachable_head_is_only_candidate(self):
        self.assertEqual(self.plan()["decision"], "delete")
    def test_advanced_open_unreachable_or_protected_work_is_retained(self):
        for kwargs in ({"sha": "c" * 40}, {"state": "open"}, {"reachable": False}, {"protected": True}):
            self.assertEqual(self.plan(**kwargs)["decision"], "keep")
    def test_release_checkpoints_are_never_candidates(self):
        for name in ("main", "release/v1.1-parked", "archive/old", "checkpoint/evidence"):
            self.assertEqual(self.plan(name=name)["decision"], "keep")
