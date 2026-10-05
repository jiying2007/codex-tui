"""Guard complete main-push preview coverage without a hand-maintained Rust list."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]


def release_push_patterns():
    text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    # This is a linter for this workflow's deliberately simple quoted paths list,
    # not a generic YAML or GitHub glob parser. Fail if its structure changes.
    prefix, separator, _ = text.partition("  workflow_dispatch:\n")
    if not separator or "    branches: [main]\n" not in prefix:
        raise ValueError("release main-push/dispatch structure needs explicit review")
    _, separator, paths = prefix.partition("    paths:\n")
    if not separator:
        raise ValueError("missing release main-push paths")
    result = []
    for line in paths.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        match = re.fullmatch(r'      - "([^"\n]+)"', line)
        if not match or match.group(1).startswith("!"):
            raise ValueError("unsupported or excluding release path: " + line)
        result.append(match.group(1))
    return result


class ReleasePathCoverage(unittest.TestCase):
    def test_all_source_and_test_subtrees_trigger_main_preview(self):
        self.assertTrue({"src/**", "tests/**"}.issubset(release_push_patterns()))

    def test_build_and_measurement_inputs_trigger_main_preview(self):
        self.assertTrue({"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "benches/**"}
                        .issubset(release_push_patterns()))
