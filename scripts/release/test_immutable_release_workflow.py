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

    def test_stable_publish_revalidates_live_state_after_packaging(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        publish = text.rsplit("\n  publish:\n", 1)[1]
        stable = publish.split(
            "- name: Publish immutable stable GitHub Release", 1
        )[1]

        self.assertIn("actions/checkout@v7", publish)
        self.assertIn("validate_release_branch_state.py", stable)
        self.assertIn(
            'git ls-remote --exit-code --tags origin "refs/tags/$TAG"',
            stable,
        )
        self.assertIn(
            "secrets.CODEX_TUI_ADMIN_READ_TOKEN",
            stable,
        )
        self.assertIn(
            "python3 scripts/release/immutable_releases.py",
            stable,
        )
        self.assertIn("--draft", stable)
        self.assertIn(
            'gh release edit "$TAG" \\\n            --repo "$GITHUB_REPOSITORY" \\\n            --draft=false',
            stable,
        )
        self.assertIn("releases/tags/$TAG", stable)
        self.assertIn("--jq .immutable", stable)
        self.assertIn("PUBLISHED_TAG_SHA", stable)

    def test_stable_publish_cleans_only_exact_unpublished_draft_and_tag(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        publish = text.rsplit("\n  publish:\n", 1)[1]
        stable = publish.split(
            "- name: Publish immutable stable GitHub Release", 1
        )[1]

        self.assertIn("group: codex-tui-release-publish", publish)
        self.assertIn("cancel-in-progress: false", publish)
        self.assertIn("trap cleanup_unpublished_draft EXIT", stable)
        self.assertIn('CURRENT_DRAFT=', stable)
        self.assertIn('CURRENT_TAG_SHA=', stable)
        self.assertIn(
            '[ "$CURRENT_DRAFT" = "true" ] && [ "$CURRENT_TAG_SHA" = "$GITHUB_SHA" ]',
            stable,
        )
        self.assertIn("--cleanup-tag", stable)
        self.assertIn('gh release upload "$TAG" bundle/*', stable)
        self.assertIn("DRAFT_CREATED=true", stable)
        self.assertIn("PUBLISHED=true", stable)
        self.assertIn("trap - EXIT", stable)

        create = stable.split('gh release create "$TAG"', 1)[1].split(
            "DRAFT_CREATED=true", 1
        )[0]
        self.assertNotIn("bundle/*", create)

    def test_preview_publish_does_not_read_admin_secret(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        publish = text.rsplit("\n  publish:\n", 1)[1]
        preview = publish.split(
            "- name: Publish preview GitHub Release", 1
        )[1].split("- name:", 1)[0]

        self.assertIn("--prerelease", preview)
        self.assertNotIn("CODEX_TUI_ADMIN_READ_TOKEN", preview)
        self.assertNotIn("ADMIN_READ_TOKEN", preview)


if __name__ == "__main__":
    unittest.main()
