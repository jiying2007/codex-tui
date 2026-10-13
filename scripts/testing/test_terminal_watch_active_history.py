"""Active-history fixture changes are isolated and retain strict read counts."""
import unittest
from watch_active_history_regression import CASES, fixture
from watch_cache_regression import validate_counts


class ActiveHistoryObserver(unittest.TestCase):
    def test_fixture_combinations_use_history_not_registry_activity(self):
        for active in (False, True):
            text = fixture(active)
            compile(text, 'active history fixture', 'exec')
            self.assertNotIn("'active' if name", text)
            self.assertEqual("'inProgress'" in text, active)
            self.assertIn("home/'fail-first'", text)
        self.assertEqual(len(set(CASES)),3)

    def test_every_evicted_owner_must_continue_receiving_requests(self):
        rows = [{'seq':1,'method':'thread/read','params':{'threadId':'latest'}}]
        with self.assertRaises(RuntimeError):
            validate_counts(rows,1,['oldest','latest'],1)

    def test_completed_or_failed_owner_must_not_receive_a_late_request(self):
        rows = [{'seq':2,'method':'thread/read','params':{'threadId':'oldest'}}]
        with self.assertRaises(RuntimeError):
            validate_counts(rows,2,['oldest'],0)


if __name__=='__main__': unittest.main()
