"""Guard stable publish=true against bypassing immutable-release verification."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ImmutableReleaseWorkflowGate(unittest.TestCase):
    def test_direct_stable_publish_requires_admin_read_secret_and_shared_check(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertIn("inputs.channel == 'stable' && inputs.publish == true", text)
        self.assertIn("secrets.CODEX_TUI_ADMIN_READ_TOKEN", text)
        self.assertIn(
            "python3 scripts/release/immutable_releases.py",
            text,
        )
        self.assertIn(
            "qualification/immutable-releases.json",
            text,
        )
        self.assertIn(
            "gate/immutable-releases.json",
            text,
        )

    def test_preview_path_does_not_require_admin_secret(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        step = text.split(
            "- name: Require immutable releases before stable publication", 1
        )[1].split("- name:", 1)[0]
        self.assertIn(
            "if: inputs.channel == 'stable' && inputs.publish == true",
            step,
        )


if __name__ == "__main__":
    unittest.main()
