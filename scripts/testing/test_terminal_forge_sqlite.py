"""Real-SQLite read contention and failure cleanup; valid on Python 3.8 and -O."""
from contextlib import closing, contextmanager
import hashlib
from pathlib import Path
import queue
import sqlite3
import tempfile
import threading
import time
import unittest
from unittest import mock

import forge_revision_regression as observer


class ReceiptReadContention(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='ctui-forge-reader-')
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.path = self.home/'state-v2.sqlite3'
        with closing(sqlite3.connect(self.path)) as db:
            db.execute('CREATE TABLE forge_mutation_receipts ('
                       'operation_id TEXT, plan_json TEXT, state TEXT, '
                       'verification TEXT, failure TEXT, updated_at_unix_ms INTEGER)')
            db.execute('INSERT INTO forge_mutation_receipts VALUES (?, ?, ?, ?, ?, ?)',
                       ('op-test', '{}', 'outcome-unknown', None, 'fixture', 17))
            db.commit()
        self.digest = hashlib.sha256(self.path.read_bytes()).hexdigest()

    @contextmanager
    def exclusive_writer(self):
        ready = queue.Queue()
        release = threading.Event()
        def writer():
            try:
                with closing(sqlite3.connect(self.path)) as db:
                    db.execute('BEGIN EXCLUSIVE')
                    ready.put(None)
                    release.wait(3)
                    db.rollback()
            except Exception as error:
                ready.put(error)
        thread = threading.Thread(target=writer, daemon=True)
        thread.start()
        try:
            error = ready.get(timeout=2)
            if error is not None:
                raise error
            yield release
        finally:
            release.set()
            thread.join(timeout=2)
            self.assertFalse(thread.is_alive(), 'test writer must be reclaimed')

    def test_old_reader_reproduces_locked_error_after_100ms(self):
        # Exact old-query behavior, not a fabricated exception or compilation failure.
        with self.exclusive_writer():
            with self.assertRaises(sqlite3.OperationalError) as caught:
                with closing(sqlite3.connect(self.path.as_uri()+'?mode=ro',
                                             uri=True, timeout=.1)) as db:
                    db.execute('SELECT * FROM forge_mutation_receipts').fetchall()
            self.assertTrue(observer.sqlite_busy(caught.exception))

    def test_released_lock_returns_actual_rows_without_writing(self):
        with self.exclusive_writer() as release:
            timer = threading.Timer(.3, release.set)
            timer.start()
            try:
                started = time.monotonic()
                rows = observer.receipt_rows(self.home)
                elapsed = time.monotonic()-started
            finally:
                timer.cancel()
                timer.join()
        self.assertGreaterEqual(elapsed, .2, 'must encounter the real lock')
        self.assertLess(elapsed, 1.5)
        self.assertEqual(rows[0]['operation_id'], 'op-test')
        self.assertEqual(rows[0]['state'], 'outcome-unknown')
        self.assertEqual(rows[0]['updated_at_unix_ms'], 17)
        self.assertEqual(hashlib.sha256(self.path.read_bytes()).hexdigest(), self.digest)

    def test_persistent_lock_is_a_bounded_failure_not_empty_success(self):
        with self.exclusive_writer():
            started = time.monotonic()
            with self.assertRaises(TimeoutError) as caught:
                observer.receipt_rows(self.home, timeout_seconds=.08)
            elapsed = time.monotonic()-started
        self.assertGreaterEqual(elapsed, .06)
        self.assertLess(elapsed, .6)
        self.assertIsInstance(caught.exception.__cause__, sqlite3.OperationalError)
        self.assertEqual(hashlib.sha256(self.path.read_bytes()).hexdigest(), self.digest)

    def test_missing_schema_is_not_retried_or_treated_as_no_receipt(self):
        with closing(sqlite3.connect(self.path)) as db:
            db.execute('DROP TABLE forge_mutation_receipts')
            db.commit()
        with mock.patch.object(observer.time, 'sleep') as sleeper:
            with self.assertRaisesRegex(sqlite3.OperationalError, 'no such table'):
                observer.receipt_rows(self.home)
            sleeper.assert_not_called()

    def test_corrupt_database_is_not_retried(self):
        self.path.write_bytes(b'not a sqlite database\n'*64)
        with mock.patch.object(observer.time, 'sleep') as sleeper:
            with self.assertRaises(sqlite3.DatabaseError):
                observer.receipt_rows(self.home)
            sleeper.assert_not_called()

    def test_absent_database_is_not_created(self):
        self.path.unlink()
        self.assertEqual(observer.receipt_rows(self.home), [])
        self.assertFalse(self.path.exists())

    def test_ambiguous_database_is_rejected(self):
        other = self.home/'another'
        other.mkdir()
        (other/self.path.name).write_bytes(self.path.read_bytes())
        with self.assertRaisesRegex(RuntimeError, 'ambiguous'):
            observer.receipt_rows(self.home)

    def test_every_reader_connection_is_closed_even_on_busy_retry(self):
        real_connect = sqlite3.connect
        closed = []
        opened = []
        class TrackedConnection(sqlite3.Connection):
            def close(self):
                closed.append(self)
                return super().close()
        def connect(*args, **kwargs):
            self.assertEqual(kwargs['timeout'], 0.0)
            self.assertIn('?mode=ro', args[0])
            db = real_connect(*args, factory=TrackedConnection, **kwargs)
            opened.append(db)
            return db
        with self.exclusive_writer():
            with mock.patch.object(observer.sqlite3, 'connect', side_effect=connect):
                with self.assertRaises(TimeoutError):
                    observer.receipt_rows(self.home, timeout_seconds=.06)
        self.assertGreaterEqual(len(opened), 2)
        self.assertEqual(opened, closed)
        with mock.patch.object(observer.sqlite3, 'connect', side_effect=connect):
            observer.receipt_rows(self.home)
        self.assertEqual(opened, closed)

    def test_python38_message_fallback_is_strict(self):
        for message in ('database is locked', 'database table is locked',
                        'database table is locked: forge_mutation_receipts',
                        'database schema is locked: main'):
            self.assertTrue(observer.sqlite_busy(sqlite3.OperationalError(message)))
        for message in ('no such table: forge_mutation_receipts', 'disk I/O error',
                        'attempt to write a readonly database', 'not a database'):
            self.assertFalse(observer.sqlite_busy(sqlite3.OperationalError(message)))

    def test_primary_and_extended_error_codes_take_precedence(self):
        for code, expected in ((5, True), (6, True), (5 | (2 << 8), True),
                               (6 | (1 << 8), True), (1, False), (11, False)):
            error = sqlite3.OperationalError('database is locked')
            error.sqlite_errorcode = code
            self.assertEqual(observer.sqlite_busy(error), expected)

    def test_invalid_deadlines_are_rejected(self):
        for budget in (0, -1, float('inf'), float('nan'), 1.01):
            with self.subTest(budget=budget), self.assertRaises(RuntimeError):
                observer.receipt_rows(self.home, timeout_seconds=budget)

    def test_failed_final_evidence_collection_still_closes_session(self):
        binary = self.home/'unused-binary'
        bin_dir = self.home/'bin'
        bin_dir.mkdir()
        (bin_dir/'codex').write_text("'cwd':os.environ['HOME']")
        output = self.home/'evidence'
        output.mkdir()
        session = mock.Mock()
        session.wait.side_effect = RuntimeError('fixture phase failed')
        with mock.patch.object(observer, 'environment', return_value={}), \
             mock.patch.object(observer.subprocess, 'run'), \
             mock.patch.object(observer, 'Session', return_value=session), \
             mock.patch.object(observer, 'receipt_rows', side_effect=TimeoutError('busy')):
            with self.assertRaises(TimeoutError):
                observer.case(binary, output, self.home, 'github-approve-control')
        session.close.assert_called_once_with()


if __name__ == '__main__':
    unittest.main()
