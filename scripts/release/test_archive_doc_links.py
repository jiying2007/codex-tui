"""Deterministic offline archive navigation; no network, no real credentials."""
import tempfile
from pathlib import Path
import unittest

from archive_doc_links import (ALIASES, rewrite_packaged_links,
                               verify_packaged_links)

SHA = "a" * 40


class BundledNavigationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="bundle-doc-links-")
        self.addCleanup(self.tmp.cleanup)
        self.checkout = Path(self.tmp.name) / "source"
        self.stage = Path(self.tmp.name) / "stage"
        self.checkout.mkdir()
        self.stage.mkdir()

    def write(self, root, relative, content):
        target = root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")
        return target

    def test_missing_historical_doc_becomes_exact_sha_github_blob(self):
        self.write(self.checkout, "docs/design/old.md", "# Historical reference\n")
        self.write(self.checkout, "docs/README.md",
                   "# Docs\n[historical](design/old.md)\n[local](current.md)\n"
                   "~~~md\n[example](missing-in-code.md)\n~~~\n")
        self.write(self.checkout, "docs/current.md", "# Current\n")
        self.write(self.stage, "docs/README.md",
                   (self.checkout / "docs/README.md").read_text())
        self.write(self.stage, "docs/current.md", "# Current\n")
        rewrite_packaged_links(self.stage, self.checkout, SHA)
        output = (self.stage / "docs/README.md").read_text()
        self.assertIn(
            "https://github.com/jiying2007/codex-tui/blob/" + SHA +
            "/docs/design/old.md", output
        )
        self.assertIn("[local](current.md)", output)
        self.assertIn("[example](missing-in-code.md)", output)
        verify_packaged_links(self.stage)

    def test_root_localized_alias_links_rebase_to_packaged_source(self):
        self.assertIn("INSTALL-UPGRADE.zh-CN.md", ALIASES)
        self.write(self.checkout, "docs/zh-CN/release/install-upgrade.md",
                   "# Installation\n[English](../../release/install-upgrade.md)\n")
        self.write(self.checkout, "docs/release/install-upgrade.md", "# English\n")
        self.write(self.stage, "INSTALL-UPGRADE.zh-CN.md",
                   "# Installation\n[English](../../release/install-upgrade.md)\n")
        self.write(self.stage, "docs/release/install-upgrade.md", "# English\n")
        rewrite_packaged_links(self.stage, self.checkout, SHA)
        self.assertIn(
            "[English](docs/release/install-upgrade.md)",
            (self.stage / "INSTALL-UPGRADE.zh-CN.md").read_text()
        )
        verify_packaged_links(self.stage)

    def test_broken_local_link_fails_verifier(self):
        self.write(self.stage, "docs/index.md", "# Index\n[missing](elsewhere.md)\n")
        with self.assertRaisesRegex(ValueError, "broken local Markdown link"):
            verify_packaged_links(self.stage)

    def test_missing_checkout_target_fails_build_not_silently_rewrites(self):
        self.write(self.stage, "README.md", "# Main\n[missing](docs/no-file.md)\n")
        with self.assertRaisesRegex(ValueError, "missing source file"):
            rewrite_packaged_links(self.stage, self.checkout, SHA)

    def test_escape_query_and_unsafe_schemes_are_refused(self):
        for target in ("../outside.md", "inside.md?token=secret",
                       "javascript:alert", "C:\\private\\secret.md"):
            with self.subTest(target=target):
                self.write(self.stage, "README.md",
                           "# Main\n[unsafe](" + target + ")\n")
                with self.assertRaises(ValueError):
                    verify_packaged_links(self.stage)

    def test_permitted_external_and_fragment_links(self):
        self.write(self.stage, "README.md",
                   "# Title\n[upstream](https://github.com/openai/codex)\n"
                   "[same](#title)\n")
        verify_packaged_links(self.stage)

    def test_source_sha_must_be_exact(self):
        self.write(self.stage, "README.md", "# Title\n")
        with self.assertRaisesRegex(ValueError, "source SHA"):
            rewrite_packaged_links(self.stage, self.checkout, "not-a-sha")


if __name__ == "__main__":
    unittest.main()
