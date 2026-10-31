import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import quiet_visual_probe as q

class ObserverTests(unittest.TestCase):
    def test_sparse_screen_update_replaces_old_text(self):
        text=q.screen_text(b'OLD\x1b[1;1HNEW',10,2)
        self.assertTrue(text.startswith('NEW'));self.assertNotIn('OLD',text)
    def test_wide_characters_use_two_columns(self):
        self.assertEqual(q.screen_text('中A'.encode(),4,1),'中A ')
    def test_overwritten_wide_glyph_clears_continuation(self):
        self.assertEqual(q.screen_text('中X\x1b[1;1HA'.encode(),4,1),'A X ')
    def test_writing_at_wide_continuation_clears_head(self):
        self.assertEqual(q.screen_text('中X\x1b[1;2HA'.encode(),4,1),' AX ')
    def test_colored_text_and_modes_do_not_appear(self):
        self.assertEqual(q.screen_text(b'\x1b[?25l\x1b[31mOK\x1b[0m',2,1),'OK')
    def test_complete_clear_removes_old_frame(self):
        self.assertNotIn('OLD',q.screen_text(b'OLD\x1b[2J\x1b[1;1HN',6,2))
    def test_line_clear_removes_remainder(self):
        self.assertEqual(q.screen_text(b'ABCDE\x1b[1;3H\x1b[K',5,1),'AB   ')
    def test_partial_csi_is_not_visible(self):
        self.assertEqual(q.screen_text(b'OK\x1b[1;',2,1),'OK')
    def test_invalid_cursor_rejected(self):
        with self.assertRaises(ValueError):q.screen_text(b'\x1b[99;1H',10,10)
    def test_unsupported_control_rejected(self):
        with self.assertRaises(ValueError):q.screen_text(b'\x1b]0;title\x07',10,10)
    def test_unsupported_csi_rejected(self):
        with self.assertRaises(ValueError):q.screen_text(b'\x1b[P',10,10)
    def test_output_bound_enforced(self):
        with self.assertRaises(ValueError):q.screen_text(b'x'*(q.MAX_OUTPUT+1),10,10)

class FixtureTests(unittest.TestCase):
    def test_trace_trailing_partial_record_is_deferred(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp)/'trace';p.write_bytes(b'{"a":1}\n{"b":')
            self.assertEqual(q.trace_rows(p),[{'a':1}])
    def test_trace_complete_malformed_record_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp)/'trace';p.write_bytes(b'not-json\n')
            with self.assertRaises(ValueError):q.trace_rows(p)
    def test_trace_bound_enforced(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp)/'trace';p.write_bytes(b'x'*(q.MAX_TRACE+1))
            with self.assertRaises(ValueError):q.trace_rows(p)
    def test_only_explicit_environment_passed(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(os.environ,{'OPENAI_API_KEY':'do-not-inherit'}):
            home=Path(tmp);env=q.prepare(home,home/'trace','zh-CN','quiet')
            self.assertNotIn('OPENAI_API_KEY',env)
            text=(home/'config/codex-tui/config.toml').read_text()
            self.assertIn('mode="off"',text);self.assertIn(str(home/'bin/absolute-visual-fixture'),text)
            self.assertEqual(env['PATH'],str(home/'bin'))
    def test_mock_source_is_valid_python(self):
        compile(q.MOCK,'mock-fixture','exec')
    def test_write_rpc_is_rejected(self):
        with self.assertRaises(ValueError):q.validate_requests([{'kind':'request','method':'turn/start'}])
    def test_wrong_thread_is_rejected(self):
        with self.assertRaises(ValueError):q.validate_requests([{'kind':'request','method':'thread/read','threadId':'alpha'}])
    def test_resume_without_exclude_turns_is_rejected(self):
        with self.assertRaises(ValueError):q.validate_requests([{'kind':'request','method':'thread/resume','threadId':'beta','excludeTurns':False}])
    def test_query_and_subscription_allowlist(self):
        items=[{'kind':'request','method':'thread/resume','threadId':'beta','excludeTurns':True},
               {'kind':'request','method':'thread/items/list','threadId':'beta'}]
        self.assertEqual(q.validate_requests(items),items)

class IdentityTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name);self.binary=self.root/'binary';self.binary.write_bytes(b'fixture-only')
        self.meta=self.root/'meta.json';self.data={'platform':'linux','commitSha':'a'*40,
             'binarySha256':hashlib.sha256(self.binary.read_bytes()).hexdigest()}
        self.write()
    def write(self):self.meta.write_text(json.dumps(self.data))
    def test_matching_digest_and_source(self):
        self.assertEqual(q.verify_identity(self.binary,self.meta),('a'*40,self.data['binarySha256']))
    def test_digest_mismatch(self):
        self.binary.write_bytes(b'changed')
        with self.assertRaises(ValueError):q.verify_identity(self.binary,self.meta)
    def test_invalid_source_identity(self):
        self.data['commitSha']='short';self.write()
        with self.assertRaises(ValueError):q.verify_identity(self.binary,self.meta)
    def test_wrong_platform(self):
        self.data['platform']='windows';self.write()
        with self.assertRaises(ValueError):q.verify_identity(self.binary,self.meta)
    def test_preexisting_output_is_never_overwritten(self):
        out=self.root/'out';out.mkdir();(out/'keep').write_text('sentinel')
        with self.assertRaises(FileExistsError):
            q.run(self.binary,self.meta,out,'en',60,24,'normal')
        self.assertEqual((out/'keep').read_text(),'sentinel')

if __name__=='__main__':unittest.main()
