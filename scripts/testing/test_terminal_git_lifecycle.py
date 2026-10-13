"""Behavioral assertions remain enabled under python -O and reject bad receipts."""
from pathlib import Path
import subprocess
import unittest
from git_lifecycle_regression import CASES, check_budget, check_editor, check_readers


class GitLifecycleAssertions(unittest.TestCase):
    def test_suite_has_unique_explicit_cases(self):
        self.assertEqual(len(CASES), 11)
        self.assertEqual(len(set(CASES)), 11)

    def test_wrong_editor_sibling_is_rejected(self):
        with self.assertRaises(RuntimeError):
            check_editor({'argv':['/repo/changed'], 'content':'CORRECT-FILE\n'}, Path('/repo  /changed'))

    def test_exact_path_with_wrong_content_is_rejected(self):
        with self.assertRaises(RuntimeError):
            check_editor({'argv':['/repo/changed'], 'content':'WRONG'}, Path('/repo/changed'))

    def test_exact_editor_target_is_accepted(self):
        check_editor({'argv':['/repo  /changed'], 'content':'CORRECT-FILE\n'}, Path('/repo  /changed'))

    def test_overflow_success_and_unrelated_failure_are_rejected(self):
        for code, error in [(0, b'budget'), (1, b'not found')]:
            with self.subTest(code=code), self.assertRaises(RuntimeError):
                check_budget(subprocess.CompletedProcess([], code, b'', error), False)

    def test_expected_budget_failure_and_healthy_control(self):
        check_budget(subprocess.CompletedProcess([], 1, b'', b'Git exceeds output budget'), False)
        check_budget(subprocess.CompletedProcess([], 0, b'git-repository: true', b''), True)

    def test_leak_gone_helper_and_missing_samples_are_rejected(self):
        good = {'auxiliaryProcessesGone':True, 'heldStderrPipes':0, 'helperConditionHeld':True}
        for changed in [{'heldStderrPipes':1}, {'helperConditionHeld':False}, {'auxiliaryProcessesGone':False}]:
            with self.subTest(changed=changed), self.assertRaises(RuntimeError):
                check_readers([dict(good, **changed)], 1)
        with self.assertRaises(RuntimeError):
            check_readers([], 5)
        check_readers([good], 1)


if __name__ == '__main__':
    unittest.main()
