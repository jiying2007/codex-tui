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

        self.assertIn("actions/checkout@", publish)
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
        self.assertIn('gh release edit "$TAG"', stable)
        self.assertIn("--notes-file bundle/RELEASE_NOTES.md", stable)
        self.assertIn("--draft=false", stable)
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
        self.assertIn("DRAFT_ATTEMPTED=false", stable)
        self.assertIn("DRAFT_ATTEMPTED=true", stable)
        self.assertIn(
            'DRAFT_OWNER_MARKER="<!-- codex-tui-publish-run:${GITHUB_RUN_ID}:${GITHUB_RUN_ATTEMPT}:${GITHUB_SHA} -->"',
            stable,
        )
        self.assertIn('CURRENT_DRAFT=', stable)
        self.assertIn('CURRENT_BODY=', stable)
        self.assertIn('CURRENT_TAG_SHA=', stable)
        self.assertIn(
            '[ "$CURRENT_DRAFT" = "true" ] && [ "$CURRENT_TAG_SHA" = "$GITHUB_SHA" ] && [ "$CURRENT_BODY" = "$DRAFT_OWNER_MARKER" ]',
            stable,
        )
        self.assertIn("--cleanup-tag", stable)
        self.assertIn('gh release upload "$TAG" bundle/*', stable)
        self.assertIn("PUBLISHED=true", stable)
        self.assertIn("trap - EXIT", stable)

        attempt = stable.index("DRAFT_ATTEMPTED=true")
        create_start = stable.index('gh release create "$TAG"')
        self.assertLess(attempt, create_start)

        create = stable.split('gh release create "$TAG"', 1)[1].split(
            'gh release upload "$TAG"', 1
        )[0]
        self.assertNotIn("bundle/*", create)
        self.assertIn('--notes "$DRAFT_OWNER_MARKER"', create)
        self.assertNotIn("--notes-file bundle/RELEASE_NOTES.md", create)

        publish = stable.split('gh release edit "$TAG"', 1)[1].split(
            "PUBLISHED=true", 1
        )[0]
        self.assertIn("--notes-file bundle/RELEASE_NOTES.md", publish)
        self.assertIn("--draft=false", publish)

    def test_preview_publish_recovers_only_owned_unpublished_draft_and_tag(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        publish = text.rsplit("\n  publish:\n", 1)[1]
        preview = publish.split(
            "- name: Publish preview GitHub Release", 1
        )[1].split("- name:", 1)[0]

        self.assertIn(
            'git ls-remote --exit-code --tags origin "refs/tags/$TAG"',
            preview,
        )
        self.assertIn("trap cleanup_unpublished_preview EXIT", preview)
        self.assertIn("DRAFT_ATTEMPTED=false", preview)
        self.assertIn("DRAFT_ATTEMPTED=true", preview)
        self.assertIn(
            'DRAFT_OWNER_MARKER="<!-- codex-tui-preview-publish-run:${GITHUB_RUN_ID}:${GITHUB_RUN_ATTEMPT}:${GITHUB_SHA} -->"',
            preview,
        )
        self.assertIn('CURRENT_DRAFT=', preview)
        self.assertIn('CURRENT_BODY=', preview)
        self.assertIn('CURRENT_TAG_SHA=', preview)
        self.assertIn(
            '[ "$CURRENT_DRAFT" = "true" ] && [ "$CURRENT_TAG_SHA" = "$GITHUB_SHA" ] && [ "$CURRENT_BODY" = "$DRAFT_OWNER_MARKER" ]',
            preview,
        )
        self.assertIn("--cleanup-tag", preview)
        self.assertIn('gh release upload "$TAG" bundle/*', preview)

        attempt = preview.index("DRAFT_ATTEMPTED=true")
        create_start = preview.index('gh release create "$TAG"')
        self.assertLess(attempt, create_start)
        create = preview.split('gh release create "$TAG"', 1)[1].split(
            'gh release upload "$TAG"', 1
        )[0]
        self.assertNotIn("bundle/*", create)
        self.assertIn('--notes "$DRAFT_OWNER_MARKER"', create)
        self.assertIn("--draft", create)
        self.assertNotIn("--prerelease", create)

        final_publish = preview.split('gh release edit "$TAG"', 1)[1].split(
            "PUBLISHED=true", 1
        )[0]
        self.assertIn("--notes-file bundle/RELEASE_NOTES.md", final_publish)
        self.assertIn("--prerelease", final_publish)
        self.assertIn("--draft=false", final_publish)
        self.assertIn("PUBLISHED_DRAFT", preview)
        self.assertIn("PUBLISHED_PRERELEASE", preview)
        self.assertIn("PUBLISHED_TAG_SHA", preview)

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
