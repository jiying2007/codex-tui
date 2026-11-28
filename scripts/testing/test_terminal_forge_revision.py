"""Fail-closed checks for the retired Forge routes, including Python -O."""
import copy
import unittest
from forge_revision_regression import CASES, FORGE, registry_visible, validate_read_only


class ForgeAssertions(unittest.TestCase):
    def test_isolated_shim_compiles(self):
        compile(FORGE, 'fixture-forge', 'exec')

    def test_exact_unique_case_coverage(self):
        self.assertEqual(set(CASES), {
            'github-approve-control', 'github-approve-missing',
            'gitlab-approve-control', 'gitlab-approve-missing',
        })
        self.assertEqual(len(CASES), 4)

    def test_exit_requires_the_observed_single_thread_registry(self):
        self.assertTrue(registry_visible('Mission Control\n0 need attention · 1 total · rows 1-1/1'))
        for screen in ('Workspace · Git + Forge\n1 total',
                       'Mission Control\n0 total', 'Mission Control\n11 total',
                       'Mission Control', 'Command Palette\n1 total'):
            with self.subTest(screen=screen):
                self.assertFalse(registry_visible(screen))

    def fixture(self, provider='github'):
        endpoint = ('/repos/fixture/isolated/pulls?per_page=100' if provider == 'github'
                    else '/projects/123/merge_requests?per_page=100')
        trace = [{'method':'GET', 'endpoint':endpoint, 'supported':True}]
        return [], trace, [['--version'], ['api', '--method', 'GET', endpoint]]

    def test_read_only_controls(self):
        for provider in ('github', 'gitlab'):
            rows, trace, calls = self.fixture(provider)
            self.assertEqual(validate_read_only(rows, trace, calls, provider),
                             {'writeCount':0, 'receiptCount':0, 'readCount':1})

    def test_missing_read_or_new_receipt_is_not_success(self):
        rows, trace, calls = self.fixture()
        for bad_rows, bad_trace, bad_calls in (
                ([{'state':'pending'}], trace, calls),
                (rows, [], calls), (rows, trace, [])):
            with self.subTest(rows=bad_rows, trace=bad_trace, calls=bad_calls):
                with self.assertRaises(RuntimeError):
                    validate_read_only(bad_rows, bad_trace, bad_calls, 'github')
        with self.assertRaises(RuntimeError):
            validate_read_only(rows, trace, calls, 'gitlab')

    def test_write_unsupported_request_and_unlogged_mutation_are_rejected(self):
        rows, trace, calls = self.fixture()
        for method, supported in (('POST', True), ('PUT', True), ('GET', False)):
            altered = copy.deepcopy(trace)
            altered[0].update(method=method, supported=supported)
            with self.assertRaises(RuntimeError):
                validate_read_only(rows, altered, calls, 'github')
        for command in (['pr', 'merge', '7'], ['api', '-XPOST', '/write'],
                        ['api', '--method=PUT', '/write'], ['api', '--method', 'DELETE', '/write'],
                        ['api', '--method']):
            with self.subTest(command=command), self.assertRaises(RuntimeError):
                validate_read_only(rows, trace, calls+[command], 'github')


if __name__ == '__main__': unittest.main()
