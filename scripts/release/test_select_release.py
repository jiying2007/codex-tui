import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "select_release",
    Path(__file__).resolve().parent / "select_release.py",
)
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)


def release(identifier, tag, draft):
    return {
        "id": identifier,
        "tag_name": tag,
        "draft": draft,
        "assets": [],
        "body": "marker",
    }


class ReleaseSelection(unittest.TestCase):
    def test_selects_draft_from_paginated_authenticated_listing(self):
        payload = [
            [release(1, "v1.3.0", False)],
            [release(2, "v1.4.0", True)],
        ]
        selected = s.select_release(payload, "v1.4.0", True)
        self.assertEqual(selected["id"], 2)

    def test_published_and_draft_with_same_tag_do_not_alias(self):
        payload = [
            release(1, "v1.4.0", False),
            release(2, "v1.4.0", True),
        ]
        self.assertEqual(s.select_release(payload, "v1.4.0", True)["id"], 2)
        self.assertEqual(s.select_release(payload, "v1.4.0", False)["id"], 1)

    def test_missing_or_ambiguous_match_fails_closed(self):
        with self.assertRaisesRegex(SystemExit, "found 0"):
            s.select_release([], "v1.4.0", True)
        with self.assertRaisesRegex(SystemExit, "found 2"):
            s.select_release(
                [release(1, "v1.4.0", True), release(2, "v1.4.0", True)],
                "v1.4.0",
                True,
            )

    def test_malformed_pagination_is_rejected(self):
        with self.assertRaisesRegex(SystemExit, "flat list or paginated"):
            s.flatten_releases([[release(1, "v1.4.0", True)], {"bad": True}])


if __name__ == "__main__":
    unittest.main()
