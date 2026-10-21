"""Retain actionable WSS failures without disclosing the fixture credential."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import wss_transport_regression as smoke


class WssDiagnostics(unittest.TestCase):
    def test_certificates_ignore_conflicting_host_extensions(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            host_config = root / "host.cnf"
            host_config.write_text(
                "[req]\ndistinguished_name=host_dn\nprompt=no\nx509_extensions=host_ca\n"
                "[host_dn]\nCN=host fixture\n"
                "[host_ca]\nbasicConstraints=critical,CA:FALSE\n"
                "keyUsage=critical,digitalSignature\nsubjectAltName=DNS:host.invalid\n",
                encoding="utf-8",
            )
            with patch.dict(os.environ, {"OPENSSL_CONF": str(host_config)}):
                smoke.certificates(root)
            for name in ("ca", "untrusted"):
                with self.subTest(root=name):
                    certificate = root / (name + ".pem")
                    text = subprocess.run(
                        ["openssl", "x509", "-in", str(certificate), "-noout", "-text"],
                        check=True, capture_output=True, text=True, timeout=15,
                    ).stdout
                    self.assertEqual(text.count("X509v3 Basic Constraints:"), 1)
                    self.assertIn("X509v3 Basic Constraints: critical", text)
                    self.assertIn("CA:TRUE", text)
                    self.assertNotIn("CA:FALSE", text)
                    self.assertEqual(text.count("X509v3 Key Usage:"), 1)
                    self.assertIn("X509v3 Key Usage: critical", text)
                    self.assertIn("Certificate Sign, CRL Sign", text)
                    self.assertNotIn("Digital Signature", text)
                    self.assertNotIn("X509v3 Subject Alternative Name", text)
                    self.assertNotIn("host.invalid", text)
                    self.assertIn("ecdsa-with-SHA256", text)
                    subprocess.run(
                        ["openssl", "verify", "-CAfile", str(certificate), str(certificate)],
                        check=True, capture_output=True, timeout=15,
                    )
            subprocess.run(
                ["openssl", "verify", "-CAfile", str(root / "ca.pem"),
                 "-purpose", "sslserver", "-verify_hostname", "localhost", str(root / "server.pem")],
                check=True, capture_output=True, timeout=15,
            )

    def run_failed_case(self, completed=None, process_error=None, server_errors=None):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "output"
            output.mkdir()

            def serve(_listener, _context, _mode, events, failures):
                events.append("waiting-for-tcp")
                failures.extend(server_errors or [])

            with patch.object(smoke.socket, "socket") as socket_factory, \
                    patch.object(smoke, "serve", side_effect=serve), \
                    patch.object(smoke.subprocess, "run", return_value=completed,
                                 side_effect=process_error) as run:
                listener = socket_factory.return_value.__enter__.return_value
                listener.getsockname.return_value = ("127.0.0.1", 43210)
                with self.assertRaises(RuntimeError) as caught:
                    smoke.run_case(root / "codex-tui", output, root, None, "trusted")
                listener.settimeout.assert_called_once_with(8)
                self.assertEqual(run.call_args.kwargs["timeout"], 10)
            report = json.loads((output / "trusted.server.json").read_text(encoding="utf-8"))
            stdout = (output / "trusted.stdout.txt").read_text(encoding="utf-8")
            stderr = (output / "trusted.stderr.txt").read_text(encoding="utf-8")
            return str(caught.exception), report, stdout, stderr

    def test_listener_timeout_keeps_doctor_exit_and_output(self):
        error, report, stdout, stderr = self.run_failed_case(
            subprocess.CompletedProcess([], 2, b"doctor preflight\n", b"configuration failed\n"),
            server_errors=["TimeoutError('timed out')"],
        )
        self.assertEqual(report["returncode"], 2)
        self.assertEqual(report["events"], ["waiting-for-tcp"])
        self.assertEqual(report["errors"], ["TimeoutError('timed out')"])
        self.assertEqual(report["url"], "wss://localhost:43210/rpc")
        self.assertEqual(report["listener"], ["127.0.0.1", 43210])
        self.assertIn('"returncode": 2', error)
        self.assertIn(stdout.strip(), error)
        self.assertIn(stderr.strip(), error)
        self.assertIn("waiting-for-tcp", error)

    def test_process_timeout_keeps_partial_output_without_inventing_an_exit(self):
        error, report, stdout, stderr = self.run_failed_case(
            process_error=subprocess.TimeoutExpired(
                ["codex-tui", "doctor"], 10, output=b"partial stdout", stderr=b"partial stderr",
            ),
        )
        self.assertIsNone(report["returncode"])
        self.assertIn("TimeoutExpired", report["processError"])
        self.assertIn("doctor process did not complete", error)
        self.assertEqual(stdout, "partial stdout")
        self.assertEqual(stderr, "partial stderr")

    def test_redaction_does_not_hide_token_disclosure_failure(self):
        error, report, stdout, stderr = self.run_failed_case(
            subprocess.CompletedProcess([], 0, b"connected: true\n" + smoke.FIXTURE_TOKEN.encode(), b""),
        )
        self.assertIn("panic or token disclosure", error)
        self.assertNotIn(smoke.FIXTURE_TOKEN, error + stdout + stderr + json.dumps(report))
        self.assertIn("<redacted-fixture-token>", error)


if __name__ == "__main__":
    unittest.main()
