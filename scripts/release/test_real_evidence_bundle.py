import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest

spec = importlib.util.spec_from_file_location(
    "real_evidence_bundle",
    Path(__file__).resolve().parent / "real_evidence_bundle.py",
)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

SHA = "0123456789abcdef0123456789abcdef01234567"


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


class StableRealEvidenceBundle(unittest.TestCase):
    def fixtures(self, root):
        compat = root / "compat.json"
        terminal = root / "terminal.json"
        performance = root / "performance.json"
        write_json(
            compat,
            {
                "schema": "codex-tui/compat/v2",
                "generatedAtUnixMs": 1234,
                "sourceSha": SHA,
                "os": "linux",
                "readiness": "ready",
            },
        )
        write_json(
            terminal,
            {
                "schema": "codex-tui/terminal-restoration/v1",
                "platform": "linux",
                "status": "pass",
                "sourceSha": SHA,
                "terminal": "ssh:xterm-256color:/dev/pts/4",
                "observedAt": "2026-10-07T00:00:00Z",
            },
        )
        write_json(
            performance,
            {
                "schema": "codex-tui/performance/v2",
                "fixture": "resident-planning-10k",
                "sourceSha": SHA,
                "iterations": 200,
                "p95Ms": 10.0,
                "p99Ms": 12.0,
                "source": "linux:test",
                "observedAt": "unix-ms:1234",
                "sampleQualified": True,
            },
        )
        return compat, terminal, performance

    def create_args(self, root):
        compat, terminal, performance = self.fixtures(root)
        return SimpleNamespace(
            commit=SHA,
            linux_compat=str(compat),
            linux_terminal=str(terminal),
            performance=str(performance),
            macos_compat="",
            macos_terminal="",
            windows_compat="",
            windows_terminal="",
        )

    def test_round_trip_recomputes_raw_file_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = self.create_args(root)
            payload, created = m.create_payload(args)
            output = root / "decoded"
            verified = m.decode_payload(payload, SHA, output)
            self.assertLessEqual(len(payload), m.MAX_PAYLOAD_CHARS)
            self.assertEqual(created["payloadSha256"], verified["payloadSha256"])
            self.assertEqual(
                verified["files"]["linuxCompat"]["sha256"],
                hashlib.sha256(Path(args.linux_compat).read_bytes()).hexdigest(),
            )
            self.assertEqual(
                verified["files"]["linuxTerminal"]["sha256"],
                hashlib.sha256(Path(args.linux_terminal).read_bytes()).hexdigest(),
            )
            self.assertEqual(
                verified["files"]["performance"]["sha256"],
                hashlib.sha256(Path(args.performance).read_bytes()).hexdigest(),
            )
            self.assertEqual(verified["compatibility"]["linux"]["status"], "ready")
            self.assertEqual(verified["terminalRestoration"]["linux"]["status"], "pass")
            self.assertEqual(verified["performance"]["iterations"], 200)
            self.assertTrue((output / "compat-linux.json").is_file())
            self.assertTrue((output / "terminal-linux.json").is_file())
            self.assertTrue((output / "performance-linux.json").is_file())

    def test_cross_sha_and_false_status_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = self.create_args(root)
            bad = json.loads(Path(args.terminal).read_text(encoding="utf-8"))
            bad["sourceSha"] = "1" * 40
            write_json(Path(args.terminal), bad)
            with self.assertRaisesRegex(SystemExit, "sourceSha does not match"):
                m.create_payload(args)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = self.create_args(root)
            bad = json.loads(Path(args.performance).read_text(encoding="utf-8"))
            bad["sampleQualified"] = False
            write_json(Path(args.performance), bad)
            with self.assertRaisesRegex(SystemExit, "sampleQualified"):
                m.create_payload(args)

    def test_optional_platform_must_be_a_pair(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = self.create_args(root)
            mac = root / "mac.json"
            write_json(
                mac,
                {
                    "schema": "codex-tui/compat/v2",
                    "generatedAtUnixMs": 1234,
                    "sourceSha": SHA,
                    "os": "macos",
                    "readiness": "ready",
                },
            )
            args.macos_compat = str(mac)
            with self.assertRaisesRegex(SystemExit, "must include both"):
                m.create_payload(args)

    def test_invalid_payload_is_rejected_before_json_trust(self):
        with self.assertRaisesRegex(SystemExit, "base64"):
            m.decode_payload("***", SHA)
        with self.assertRaisesRegex(SystemExit, "empty"):
            m.decode_payload("", SHA)


if __name__ == "__main__":
    unittest.main()
