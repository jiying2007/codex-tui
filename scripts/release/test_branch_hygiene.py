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

    def test_squash_requires_exact_head_and_complete_equivalence_proof(self):
        branch = {"name": "fix/squashed", "commit": {"sha": "a" * 40}}
        pr = {"number": 2, "state": "closed", "merged_at": "date", "merge_commit_sha": "c" * 40,
              "head": {"ref": branch["name"], "sha": "a" * 40, "repo": {"full_name": "owner/repo"}}, "base": {"ref": "main"}}
        proof = {"method": "identical-raw-tree-delta-v1", "deltaSha256": "d" * 64}
        entry = h.build_plan("owner/repo", "b" * 40, [branch], [pr], lambda *_: False, lambda *_: proof)["entries"][0]
        self.assertEqual(entry["decision"], "delete")
        self.assertEqual(entry["equivalenceProof"], proof)
        pr["head"]["sha"] = "e" * 40
        self.assertEqual(h.build_plan("owner/repo", "b" * 40, [branch], [pr], lambda *_: False, lambda *_: proof)["entries"][0]["decision"], "keep")
    def test_deleted_pull_request_repository_is_not_a_cleanup_authority(self):
        branch = {"name": "fix/old", "commit": {"sha": "a" * 40}}
        pr = {"number": 2, "state": "closed", "merged_at": "date", "head": {"ref": "fix/old", "sha": "a" * 40, "repo": None}}
        self.assertEqual(h.build_plan("owner/repo", "b" * 40, [branch], [pr], lambda *_: True)["entries"][0]["decision"], "keep")

class RealSquashProof(unittest.TestCase):
    def test_blob_mode_and_binary_delta_match_is_required(self):
        import os, subprocess, tempfile
        with tempfile.TemporaryDirectory() as directory:
            def git(*args):
                return subprocess.check_output(["git", "-C", directory, *args], stderr=subprocess.DEVNULL).decode().strip()
            git("init")
            git("config", "user.email", "test@example.invalid")
            git("config", "user.name", "Test")
            path = Path(directory, "binary.dat")
            path.write_bytes(b"before\x00")
            git("add", "."); git("commit", "-m", "base")
            base = git("rev-parse", "HEAD")
            git("checkout", "-b", "topic")
            path.write_bytes(b"after\x00")
            git("commit", "-am", "topic")
            head = git("rev-parse", "HEAD")
            git("checkout", "-b", "main-copy", base)
            git("merge", "--squash", "topic"); git("commit", "-m", "squash")
            merge = git("rev-parse", "HEAD")
            old = os.getcwd()
            try:
                os.chdir(directory)
                proof = h.squash_equivalence(head, merge, merge)
                self.assertEqual(proof["mergeCommit"], merge)
                path.write_bytes(b"different\x00")
                git("commit", "-am", "different")
                wrong = git("rev-parse", "HEAD")
                self.assertIsNone(h.squash_equivalence(head, wrong, wrong))
                self.assertIsNone(h.squash_equivalence(head, merge, base))
                self.assertIsNone(h.squash_equivalence(base, base, base))
            finally:
                os.chdir(old)

class WorkflowBacklogCleanupContract(unittest.TestCase):
    def test_policy_change_push_rechecks_retained_backlog_fail_closed(self):
        text = (
            Path(__file__).resolve().parents[2]
            / ".github/workflows/repository-hygiene.yml"
        ).read_text(encoding="utf-8")
        self.assertIn("group: repository-hygiene", text)
        self.assertIn(
            "if: github.event_name == 'pull_request' && github.event.pull_request.merged == true",
            text,
        )
        self.assertIn("cancel-in-progress: false", text)
        self.assertIn('if: github.event_name == \'push\'', text)
        self.assertIn('"scripts/repository/branch_hygiene.py"', text)
        self.assertIn('"scripts/release/test_branch_hygiene.py"', text)
        self.assertIn('ARGS=(--repo "$GITHUB_REPOSITORY" --output "$PLAN")', text)
        self.assertIn("--plan-sha256", text)
        self.assertIn("--confirm-repository", text)
        self.assertIn("retained-branch-hygiene-", text)
        self.assertIn("if-no-files-found: error", text)

    def test_release_and_historical_prefixes_remain_retained(self):
        for name in (
            "release/v1.1-parked",
            "archive/v1.0",
            "checkpoint/evidence",
        ):
            branch = {"name": name, "commit": {"sha": "a" * 40}, "protected": False}
            pr = {
                "number": 1,
                "state": "closed",
                "merged_at": "date",
                "head": {
                    "ref": name,
                    "sha": "a" * 40,
                    "repo": {"full_name": "owner/repo"},
                },
            }
            entry = h.build_plan(
                "owner/repo",
                "b" * 40,
                [branch],
                [pr],
                lambda *_: True,
            )["entries"][0]
            self.assertEqual(entry["decision"], "keep")
            self.assertEqual(entry["reason"], "protected-or-retained-reference")


class ScopedCleanupContract(unittest.TestCase):
    def test_scope_excludes_other_branches_and_accepts_already_absent(self):
        branches = [{"name": "fix/merged", "commit": {"sha": "a" * 40}},
                    {"name": "fix/other", "commit": {"sha": "b" * 40}}]
        self.assertEqual(h.scoped_branches(branches, "fix/merged", "a" * 40), branches[:1])
        self.assertEqual(h.scoped_branches(branches, "absent", "a" * 40), [])
        self.assertEqual(h.scoped_branches(branches, None, None), branches)
    def test_scope_rejects_missing_identity_and_advanced_head(self):
        branches = [{"name": "fix/merged", "commit": {"sha": "b" * 40}}]
        for name, sha in (("fix/merged", "a" * 40), ("fix/merged", None), (None, "b" * 40)):
            with self.assertRaises(ValueError):
                h.scoped_branches(branches, name, sha)
