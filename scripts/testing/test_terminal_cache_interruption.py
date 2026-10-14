"""Keep interruption probes tied to actual admission and real fixture exit."""
import unittest
from watch_cache_interruption_regression import CASES, held_mock
from watch_cache_regression import MOCK, validate_counts


class CacheInterruptionObserver(unittest.TestCase):
    def test_complete_unique_control_matrix(self):
        self.assertEqual(len(CASES), 6)
        self.assertEqual(len(set(CASES)), 6)
        self.assertEqual(sum(name.endswith('-cancel') for name in CASES), 2)
        self.assertEqual(sum(name.endswith('-disconnect') for name in CASES), 2)
        self.assertEqual(sum(name.endswith('-refused') for name in CASES), 2)

    def test_held_fixture_is_local_and_target_specific(self):
        for target in ('anchor', 'ghost'):
            text = held_mock(target)
            compile(text, 'isolated interruption fixture', 'exec')
            self.assertIn("ident=="+repr(target), text)
            self.assertIn("(home/'held').touch()", text)
            self.assertIn('until=time.monotonic()+4', text)
            self.assertIn("(home/'disconnect').exists():sys.exit(0)", text)
            self.assertIn("(home/'shutdown').exists():os._exit(0)", text)
        self.assertNotIn("home/'hold'", MOCK)

    def test_late_response_does_not_authorize_a_new_history_or_queue_read(self):
        for method in ('thread/read', 'thread/queue/list'):
            row = {'seq':3, 'method':method, 'params':{'threadId':'ghost'}}
            validate_counts([row], 4, ['ghost'], 0, method)
            with self.assertRaises(RuntimeError):
                validate_counts([row], 3, ['ghost'], 0, method)
            with self.assertRaises(RuntimeError):
                validate_counts([], 3, ['ghost'], 1, method)


if __name__ == '__main__': unittest.main()
