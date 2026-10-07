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
        self.assertIn("stable-published-release.json", stable)
        self.assertIn("X-GitHub-Api-Version: 2026-03-10", stable)
        self.assertIn("PUBLISHED_DRAFT", stable)
        self.assertIn("PUBLISHED_PRERELEASE", stable)
        self.assertIn('test "$PUBLISHED_DRAFT" = "false"', stable)
        self.assertIn('test "$PUBLISHED_PRERELEASE" = "false"', stable)
        self.assertIn("PUBLISHED_TAG_SHA", stable)

    def test_stable_publish_skips_redundant_current_run_packaging(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        package = text.split("\n  package:\n", 1)[1].split("\n  bundle:\n", 1)[0]
        bundle = text.split("\n  bundle:\n", 1)[1].split("\n  publish:\n", 1)[0]
        publish = text.rsplit("\n  publish:\n", 1)[1]

        skip = "inputs.channel != 'stable' || inputs.publish != true"
        self.assertIn(skip, package)
        self.assertIn(skip, bundle)
        self.assertIn("always() && inputs.publish == true", publish)
        self.assertIn("needs.gate.result == 'success'", publish)
        self.assertIn("needs.gate.outputs.channel == 'stable' || needs.bundle.result == 'success'", publish)

        current_bundle_download = publish.split(
            "- name: Promote exact qualified stable dry-run bundle", 1
        )[0]
        self.assertIn("if: needs.gate.outputs.channel == 'preview'", current_bundle_download)

    def test_stable_publish_promotes_exact_qualified_dry_run_bundle(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        publish = text.rsplit("\n  publish:\n", 1)[1]
        stable_start = publish.index("- name: Publish immutable stable GitHub Release")

        self.assertIn("actions: read", publish)
        self.assertIn('gh run download "$STABLE_QUALIFICATION_RUN"', publish)
        self.assertIn("--name release-bundle", publish)
        self.assertIn("scripts/release/promote_release_bundle.py", publish)
        self.assertNotIn('CURRENT_BUNDLE=', publish)
        self.assertIn('mv "$PRIOR_BUNDLE" bundle', publish)
        self.assertIn("stable-bundle-promotion.json", publish)
        self.assertIn("name: stable-bundle-promotion", publish)
        self.assertNotIn("compare_release_archives.py", publish)
        self.assertLess(
            publish.index("scripts/release/promote_release_bundle.py"),
            stable_start,
        )

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

    def test_stable_revalidates_exact_draft_after_immutable_check_at_publish_point(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        stable = text.rsplit("\n  publish:\n", 1)[1].split(
            "- name: Publish immutable stable GitHub Release", 1
        )[1].split("- uses: actions/upload-artifact@", 1)[0]

        immutable = stable.index("scripts/release/immutable_releases.py")
        prepublish = stable.index("--phase prepublish")
        publish_edit = stable.index('gh release edit "$TAG"')
        self.assertLess(immutable, prepublish)
        self.assertLess(prepublish, publish_edit)
        self.assertIn("stable-prepublish-release.json", stable)
        self.assertIn("PREPUBLISH_TAG_SHA=", stable)
        self.assertIn("PREPUBLISH_BODY=", stable)
        self.assertIn("stable draft identity drifted at publication point", stable)

    def test_published_state_uses_one_version_pinned_release_snapshot(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        publish = text.rsplit("\n  publish:\n", 1)[1]
        preview = publish.split(
            "- name: Publish preview GitHub Release", 1
        )[1].split("- name: Publish immutable stable GitHub Release", 1)[0]
        stable = publish.split(
            "- name: Publish immutable stable GitHub Release", 1
        )[1].split("- uses: actions/upload-artifact@", 1)[0]

        for section, name in ((preview, "preview"), (stable, "stable")):
            after = section.split('gh release edit "$TAG"', 1)[1]
            self.assertEqual(after.count("releases/tags/$TAG"), 1)
            self.assertIn("X-GitHub-Api-Version: 2026-03-10", after)
            self.assertIn(f"{name}-published-release.json", after)
            self.assertIn("PUBLISHED_DRAFT=", after)
            self.assertIn("PUBLISHED_PRERELEASE=", after)
        self.assertIn("PUBLISHED_IMMUTABLE=", stable.split('gh release edit "$TAG"', 1)[1])

    def test_preview_and_stable_verify_uploaded_asset_digests_before_and_after_publish(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        publish = text.rsplit("\n  publish:\n", 1)[1]
        preview = publish.split(
            "- name: Publish preview GitHub Release", 1
        )[1].split("- name:", 1)[0]
        stable = publish.split(
            "- name: Publish immutable stable GitHub Release", 1
        )[1].split("- uses: actions/upload-artifact@", 1)[0]

        self.assertEqual(
            preview.count("scripts/release/verify_release_assets.py"),
            2,
        )
        self.assertEqual(
            stable.count("scripts/release/verify_release_assets.py"),
            3,
        )
        for section, prefix in ((preview, "preview"), (stable, "stable")):
            self.assertIn("X-GitHub-Api-Version: 2026-03-10", section)
            self.assertIn(f"{prefix}-draft-assets.json", section)
            self.assertIn(f"{prefix}-published-assets.json", section)
            self.assertIn(f"--channel {prefix}", section)
        self.assertIn("stable-prepublish-assets.json", stable)
        self.assertEqual(
            preview.count('--source-sha "$GITHUB_SHA"'),
            2,
        )
        # Stable has three asset verifiers plus the pre-existing exact-bundle
        # promotion source binding.
        self.assertEqual(
            stable.count('--source-sha "$GITHUB_SHA"'),
            4,
        )

        self.assertIn("name: release-asset-integrity", publish)
        self.assertIn("if: success()", publish)
        self.assertIn("if-no-files-found: error", publish)
        self.assertIn("name: release-asset-integrity-partial", publish)
        self.assertIn("if: failure()", publish)
        self.assertIn("if-no-files-found: ignore", publish)
        self.assertIn("*-assets.json", publish)
        self.assertIn("*-release.json", publish)
        self.assertIn("publish-immutable-releases.json", publish)

    def test_successful_publication_requires_complete_channel_receipts(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        publish = text.rsplit("\n  publish:\n", 1)[1]
        gate = publish.split(
            "- name: Require complete successful publication evidence", 1
        )[1].split("- uses: actions/upload-artifact@", 1)[0]

        self.assertIn("if: success()", gate)
        for name in (
            "preview-draft-release.json",
            "preview-draft-assets.json",
            "preview-published-release.json",
            "preview-published-assets.json",
            "stable-draft-release.json",
            "stable-draft-assets.json",
            "publish-immutable-releases.json",
            "stable-prepublish-release.json",
            "stable-prepublish-assets.json",
            "stable-published-release.json",
            "stable-published-assets.json",
        ):
            self.assertIn(name, gate)
        self.assertIn("successful publication is missing retained evidence", gate)

    def test_successful_publication_cannot_omit_integrity_receipts(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        publish = text.rsplit("\n  publish:\n", 1)[1]
        self.assertIn(
            "if: success()\n        with:\n          name: release-asset-integrity",
            publish,
        )
        self.assertIn(
            "name: release-asset-integrity\n          path:",
            publish,
        )
        self.assertIn("if-no-files-found: error", publish)
        self.assertIn("*-assets.json", publish)
        self.assertIn("*-release.json", publish)
        self.assertIn("publish-immutable-releases.json", publish)

        self.assertIn(
            "if: failure()\n        with:\n          name: release-asset-integrity-partial",
            publish,
        )
        partial = publish.split(
            "name: release-asset-integrity-partial", 1
        )[1]
        self.assertIn("if-no-files-found: ignore", partial)

    def test_draft_identity_uses_authenticated_release_listing_not_published_by_tag_endpoint(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        publish = text.rsplit("\n  publish:\n", 1)[1]
        preview = publish.split(
            "- name: Publish preview GitHub Release", 1
        )[1].split("- name: Publish immutable stable GitHub Release", 1)[0]
        stable = publish.split(
            "- name: Publish immutable stable GitHub Release", 1
        )[1].split("- uses: actions/upload-artifact@", 1)[0]

        for section in (preview, stable):
            before_publish = section.split('gh release edit "$TAG"', 1)[0]
            self.assertIn("releases?per_page=100", before_publish)
            self.assertIn("--paginate --slurp", before_publish)
            self.assertIn("scripts/release/select_release.py", before_publish)
            self.assertIn("--draft true", before_publish)
            self.assertNotIn("releases/tags/$TAG", before_publish)

        # Once draft=false has made the release public, the documented by-tag
        # endpoint is the correct post-publication identity/readback path.
        self.assertIn("releases/tags/$TAG", preview.split('gh release edit "$TAG"', 1)[1])
        self.assertIn("releases/tags/$TAG", stable.split('gh release edit "$TAG"', 1)[1])

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
