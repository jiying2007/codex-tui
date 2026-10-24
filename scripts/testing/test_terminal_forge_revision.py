"""Fail-closed contract tests for the actual-program Forge observer, including -O."""
import copy
import json
import unittest
from forge_revision_regression import CASES, SHA_A, SHA_B, registry_visible, validate_outcome


class ForgeAssertions(unittest.TestCase):
    def test_exact_unique_case_coverage(self):
        self.assertEqual(len(CASES), 22)
        self.assertEqual(len(set(CASES)), 22)

    def test_exit_requires_the_observed_single_thread_registry(self):
        self.assertTrue(registry_visible('Mission Control\n0 need attention · 1 total · rows 1-1/1'))
        for screen in ('Workspace · Git + Forge\n1 total',
                       'Mission Control\n0 total', 'Mission Control\n11 total',
                       'Mission Control', 'Command Palette\n1 total'):
            with self.subTest(screen=screen):
                self.assertFalse(registry_visible(screen))

    def fixture(self, state='succeeded'):
        rows = [{'state':state, 'plan_json':json.dumps({'preconditions':[
            {'key':'confirmed-head-sha', 'expected':SHA_A}]})}]
        trace = [{'method':'POST','fields':{'commit_id':SHA_A},'supported':True}]
        return rows, trace

    def test_healthy_and_unknown_controls(self):
        rows, trace = self.fixture()
        self.assertEqual(validate_outcome(rows,trace,'control')['writeCount'],1)
        rows[0]['state']='outcome-unknown'
        validate_outcome(rows,trace,'after')
        rows[0]['state']='failed'
        validate_outcome(rows,[],'before')

    def test_unknown_outcome_cannot_be_reported_as_success(self):
        rows, trace = self.fixture()
        for mode in ('after','uncertain','wrong-review'):
            with self.subTest(mode=mode), self.assertRaises(RuntimeError):
                validate_outcome(rows,trace,mode)

    def test_retarget_missing_plan_replay_and_unsupported_shims_are_rejected(self):
        rows, trace = self.fixture()
        variants = [(rows,[]), (rows,trace*2), ([],trace), (rows*2,trace)]
        moved = copy.deepcopy(trace); moved[0]['fields']['commit_id']=SHA_B
        unknown = copy.deepcopy(trace); unknown[0]['supported']=False
        missing = copy.deepcopy(rows); missing[0]['plan_json']='{"preconditions":[]}'
        variants.extend([(rows,moved),(rows,unknown),(missing,trace)])
        for bad_rows, bad_trace in variants:
            with self.subTest(rows=bad_rows,trace=bad_trace), self.assertRaises(RuntimeError):
                validate_outcome(bad_rows,bad_trace,'control')


if __name__ == '__main__': unittest.main()
