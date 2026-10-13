"""Return validity must not be confused with retained cached text."""
import unittest
from watch_return_regression import CASES, validate_return

class ReturnObserver(unittest.TestCase):
    def test_exact_cases(self):
        self.assertEqual(len(CASES), 4)
        self.assertEqual(len(set(CASES)), 4)

    def test_live_and_retired_controls(self):
        validate_return(True, True, 1)
        validate_return(False, False, 0)

    def test_wrong_surface_or_watch_count_fails(self):
        for returned, keep, reads in [(False,True,0),(True,False,1),
                                      (True,True,0),(False,False,1)]:
            with self.subTest(returned=returned,keep=keep,reads=reads):
                with self.assertRaises(RuntimeError):
                    validate_return(returned,keep,reads)

if __name__ == '__main__': unittest.main()
