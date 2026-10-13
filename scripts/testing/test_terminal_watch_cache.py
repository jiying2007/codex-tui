"""Reject missing eviction prerequisites, lost returns and leaked RPCs."""
import unittest
from watch_cache_regression import CASES, MOCK, WORKERS, validate_counts, validate_return


class CacheObserver(unittest.TestCase):
    def test_fixture_and_unique_matrix(self):
        compile(MOCK, 'isolated cache protocol', 'exec')
        self.assertEqual(len(WORKERS), 32)
        self.assertEqual(len(CASES), 9)
        self.assertEqual(len(set(CASES)), 9)
        self.assertIn('LATEST_IS_NOT_SEARCH_HIT', MOCK)

    def test_actual_completions_and_exact_phase_are_required(self):
        row = {'seq':1,'method':'thread/items/list','params':{'threadId':'worker'}}
        validate_counts([row], 1, ['worker'], 1, 'thread/items/list')
        validate_counts([row], 2, ['worker'], 0, 'thread/items/list')
        for rows in ([], [row, row]):
            with self.assertRaises(RuntimeError):
                validate_counts(rows, 1, ['worker'], 1, 'thread/items/list')
        with self.assertRaises(RuntimeError):
            validate_counts([row], 1, ['worker', 'missing'], 1, 'thread/items/list')

    def test_cache_hits_retirement_errors_and_misses_are_distinct(self):
        for kind, body, reads in [('resident',True,0), ('evicted',True,1),
                                  ('retired',False,0), ('read-error',False,1)]:
            validate_return(kind,body,reads)
            with self.assertRaises(RuntimeError):
                validate_return(kind,not body,reads)
            with self.assertRaises(RuntimeError):
                validate_return(kind,body,reads+1)


if __name__ == '__main__': unittest.main()
