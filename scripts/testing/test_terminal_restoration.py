"""Negative fixtures verify the probe, not additional product defects."""
import unittest
from terminal_restoration import CursorReplies, ProbeFailure, validate_terminal

ENTER = b"\x1b[?1049h\x1b[?2004h\x1b[?25l"
LEAVE = b"\x1b[?2004l\x1b[?1049l\x1b[?25h"
ERROR = b"Error: final operator-state save failed: unsupported schema 999\r\n"


class TerminalEvidenceContract(unittest.TestCase):
    def check(self, data=ENTER + LEAVE, before=None, after=None, raw=True, code=0, failed=False):
        return validate_terminal(data, before or [1, 2, [3]], after or [1, 2, [3]], raw, code, failed)

    def test_complete_normal_exit(self):
        self.assertEqual(len(self.check()), 6)

    def test_complete_failed_save(self):
        self.assertTrue(self.check(ENTER + LEAVE + ERROR, code=1, failed=True)["saveErrorAfterRestoration"])

    def test_each_missing_mode_transition_fails(self):
        for token in (b"\x1b[?1049h", b"\x1b[?1049l", b"\x1b[?2004h", b"\x1b[?2004l", b"\x1b[?25l", b"\x1b[?25h"):
            with self.subTest(token=token), self.assertRaises(ProbeFailure):
                self.check((ENTER + LEAVE).replace(token, b""))

    def test_cleanup_before_entry_is_not_restoration(self):
        with self.assertRaises(ProbeFailure):
            self.check(LEAVE + ENTER)

    def test_later_reentry_cannot_reuse_earlier_cleanup(self):
        with self.assertRaises(ProbeFailure):
            self.check(ENTER + LEAVE + b"\x1b[?1049h")

    def test_error_before_cleanup_is_not_post_restore_failure(self):
        with self.assertRaises(ProbeFailure):
            self.check(ENTER + ERROR + LEAVE, code=1, failed=True)

    def test_unrelated_error_cannot_count_as_injected_failure(self):
        with self.assertRaises(ProbeFailure):
            self.check(ENTER + LEAVE + b"final network error", code=1, failed=True)

    def test_wrong_exit_or_attributes_or_no_raw_mode_fail(self):
        for kwargs in ({"code": 1}, {"after": [9]}, {"raw": False}):
            with self.subTest(kwargs=kwargs), self.assertRaises(ProbeFailure):
                self.check(**kwargs)

    def test_empty_output_cannot_pass(self):
        with self.assertRaises(ProbeFailure):
            self.check(b"")

    def test_cursor_queries_at_every_chunk_boundary(self):
        query, reply = b"\x1b[6n", b"\x1b[1;1R"
        for split in range(len(query) + 1):
            responder = CursorReplies()
            self.assertEqual(responder.feed(query[:split]) + responder.feed(query[split:]), reply)
            self.assertEqual(responder.feed(b"more"), b"")
        self.assertEqual(CursorReplies().feed(query * 2), reply * 2)


if __name__ == "__main__":
    unittest.main()
