#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib

from _compat import write_text_lf


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def flatten_releases(payload) -> list:
    require(isinstance(payload, list), "release listing JSON must be a list")
    if not payload:
        return []
    if all(isinstance(item, dict) for item in payload):
        return payload
    require(
        all(isinstance(page, list) for page in payload),
        "release listing JSON must be a flat list or paginated list-of-lists",
    )
    releases = []
    for page in payload:
        require(
            all(isinstance(item, dict) for item in page),
            "release listing page contains a non-object entry",
        )
        releases.extend(page)
    return releases


def select_release(payload, tag: str, draft: bool) -> dict:
    releases = flatten_releases(payload)
    matches = [
        release
        for release in releases
        if release.get("tag_name") == tag and release.get("draft") is draft
    ]
    require(
        len(matches) == 1,
        f"expected exactly one {'draft' if draft else 'published'} release for {tag}; "
        f"found {len(matches)}",
    )
    release = matches[0]
    require(isinstance(release.get("id"), int), "selected release is missing numeric id")
    require(isinstance(release.get("assets"), list), "selected release assets must be a list")
    return release


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Select one exact draft/published release from the authenticated List releases "
            "REST response. This is used for drafts because the by-tag endpoint is published-only."
        )
    )
    parser.add_argument("--releases-json", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--draft", choices=["true", "false"], required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    try:
        payload = json.loads(pathlib.Path(args.releases_json).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read release listing JSON: {error}") from error

    release = select_release(payload, args.tag, args.draft == "true")
    write_text_lf(
        pathlib.Path(args.output),
        json.dumps(release, indent=2, sort_keys=True) + "\n",
    )
    print(
        f"VALID selected {'draft' if args.draft == 'true' else 'published'} "
        f"release {args.tag} id={release['id']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
