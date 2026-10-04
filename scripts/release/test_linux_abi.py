import unittest
from check_linux_abi import inspect_symbols

class LinuxAbiContract(unittest.TestCase):
    def test_weak_new_symbols_do_not_raise_the_required_floor(self):
        result = inspect_symbols("""
  1: 0000000000000000 0 FUNC GLOBAL DEFAULT UND memcpy@GLIBC_2.14 (3)
  2: 0000000000000000 0 FUNC GLOBAL DEFAULT UND pthread_create@GLIBC_2.31 (2)
  3: 0000000000000000 0 FUNC WEAK DEFAULT UND pidfd_spawn@GLIBC_2.39 (4)
""")
        self.assertEqual(result["requiredGlibc"], "2.31")
        self.assertEqual(result["weakVersions"], ["2.39"])
    def test_strong_new_symbol_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "exceeds"):
            inspect_symbols("1: 0000 0 FUNC GLOBAL DEFAULT UND openpty@GLIBC_2.34")
    def test_empty_or_private_import_fails_closed(self):
        for value in ("", "1: 0000 0 FUNC GLOBAL DEFAULT UND secret@GLIBC_PRIVATE"):
            with self.assertRaises(ValueError): inspect_symbols(value)

if __name__ == "__main__": unittest.main()
