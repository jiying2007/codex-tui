"""Pressure evidence must distinguish absent owners from healthy live owners."""
import unittest
from watch_pressure_regression import CASES, PRESSURE_MOCK, check_counts


class WatchPressureAssertions(unittest.TestCase):
    def test_scenarios_and_fixture_contract(self):
        self.assertEqual(len(CASES), 6)
        self.assertEqual(len(set(CASES)), 6)
        compile(PRESSURE_MOCK, 'isolated pressure fixture', 'exec')
        self.assertIn("(home/'waiting').touch()", PRESSURE_MOCK)

    def test_only_the_exact_target_and_phase_count(self):
        rows = [{'seq':1, 'method':'thread/read', 'params':{'threadId':'watch-a'}},
                {'seq':2, 'method':'thread/read', 'params':{'threadId':'other'}}]
        check_counts(rows, 1, (1, 0))
        check_counts(rows, 2, (0, 0))

    def test_leaked_reads_and_lost_live_updates_fail(self):
        row = {'seq':2, 'method':'thread/read', 'params':{'threadId':'watch-a'}}
        for rows, expected in [([row], (0, 0)), ([], (1, 0)), ([row, row], (1, 0)), ([], (0, 1))]:
            with self.subTest(expected=expected), self.assertRaises(RuntimeError):
                check_counts(rows, 2, expected)


if __name__ == '__main__': unittest.main()
