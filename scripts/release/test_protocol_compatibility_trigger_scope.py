"""Fail closed if App Server event adapters no longer trigger protocol replay.

Keep push and PR triggers symmetric. Canonical CI runs all cargo tests
regardless; this independent workflow provides a narrower replay receipt.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/protocol-compatibility.yml"
REQUIRED = {
    "src/app_server.rs",
    "src/app_server/**",
    "src/app_server_*",
    "src/codex_protocol.rs",
    "src/compat.rs",
    "src/conversation.rs",
    "src/conversation/**",
    "src/replay_backend.rs",
    "src/domain.rs",
    "src/thread_queue.rs",
    "src/app/queue_*.rs",
    "src/user_response.rs",
    "src/app/user_*.rs",
    "src/runtime_connection.rs",
    "src/runtime_user_response.rs",
    "src/runtime_user_response/**",
    "tests/protocol_replay.rs",
    "tests/fixtures/protocol/**",
    "scripts/release/test_protocol_compatibility_trigger_scope.py",
    ".github/workflows/protocol-compatibility.yml",
}


def trigger_paths(event):
    if event not in ("push", "pull_request"):
        raise ValueError("unsupported protocol trigger")
    text = WORKFLOW.read_text(encoding="utf-8")
    start = "  " + event + ":\n"
    end = "  pull_request:\n" if event == "push" else "  schedule:\n"
    if text.count(start) != 1 or text.count(end) != 1:
        raise ValueError("protocol workflow event structure requires review")
    block = text.split(start, 1)[1].split(end, 1)[0]
    if event == "push" and "    branches: [main]\n" not in block:
        raise ValueError("protocol main push trigger missing")
    marker = "    paths:\n"
    if block.count(marker) != 1:
        raise ValueError("protocol path trigger missing or ambiguous")
    paths = []
    for line in block.split(marker, 1)[1].splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        match = re.fullmatch(r'      - "([^"\n]+)"', line)
        if not match or match.group(1).startswith("!"):
            raise ValueError("unsupported protocol path: " + line)
        paths.append(match.group(1))
    if len(paths) != len(set(paths)) or not paths:
        raise ValueError("duplicate or empty protocol path trigger")
    return paths


class ProtocolCompatibilityTriggerScopeTests(unittest.TestCase):
    def test_push_and_pull_request_scopes_are_identical(self):
        self.assertEqual(trigger_paths("push"), trigger_paths("pull_request"))

    def test_all_transport_registry_queue_and_response_adapters_are_covered(self):
        paths = set(trigger_paths("push"))
        self.assertFalse(REQUIRED - paths, "missing protocol adapter path triggers")

    def test_l2_transport_and_registry_scenarios_are_explicit(self):
        source = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("Exercise mock transport and registry lifecycle (L2)", source)
        self.assertIn("cargo test --locked --lib app_server_transport::tests::", source)
        self.assertIn("cargo test --locked --lib app_server::tests::", source)
        self.assertIn("src/app_server_*", trigger_paths("push"))

    def test_scheduled_and_manual_replay_are_retained(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn('  schedule:\n    - cron: "41 2 * * 3"', text)
        self.assertIn("  workflow_dispatch:\n", text)
        self.assertIn("cargo test --locked --test protocol_replay", text)
        self.assertIn("test_protocol_compatibility_trigger_scope.py", text)


if __name__ == "__main__":
    unittest.main()
