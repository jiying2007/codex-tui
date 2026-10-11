#!/usr/bin/env python3
"""Observe public Codex stable releases; never qualify real App Server L3."""
import argparse
import json
import pathlib
import re
from datetime import datetime, timezone

SCHEMA = "codex-tui/upstream-codex-stable-watch/v1"
STABLE_TAG = re.compile(r"^rust-v([0-9]+)\.([0-9]+)\.([0-9]+)$")


def version(tag):
    match = STABLE_TAG.fullmatch(tag) if isinstance(tag, str) else None
    return tuple(map(int, match.groups())) if match else None


def observe(releases, manifest):
    pinned_info = manifest.get("upstreamBaseline", {})
    if pinned_info.get("repository") != "openai/codex":
        raise ValueError("unexpected upstream repository")
    pinned = pinned_info.get("releaseTag")
    pinned_version = version(pinned)
    if pinned_version is None:
        raise ValueError("missing stable protocol release tag")
    if not isinstance(releases, list):
        raise ValueError("releases must be a list")
    candidates = [
        (version(row.get("tag_name")), row["tag_name"])
        for row in releases
        if isinstance(row, dict)
        and not row.get("draft")
        and not row.get("prerelease")
        and version(row.get("tag_name")) is not None
    ]
    latest_version, latest_tag = max(candidates) if candidates else (None, None)
    status = (
        "unverified-no-stable-in-page" if latest_version is None
        else "newer-stable-observed" if latest_version > pinned_version
        else "pinned-stable-current" if latest_version == pinned_version
        else "unverified-observation-behind-pin"
    )
    return {
        "schema": SCHEMA,
        "status": status,
        "upstreamRepository": "openai/codex",
        "pinnedProtocolRelease": pinned,
        "latestObservedStable": latest_tag,
        "latestObservedUrl": (
            "https://github.com/openai/codex/releases/tag/" + latest_tag if latest_tag else None
        ),
        "stableQualified": False,
        "l3Qualified": False,
        "authority": "public-release-metadata-only",
        "policy": "New stable means review needed; public metadata is not live interoperability."
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--releases", required=True, type=pathlib.Path)
    parser.add_argument("--manifest", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()
    report = observe(
        json.loads(args.releases.read_text(encoding="utf-8")),
        json.loads(args.manifest.read_text(encoding="utf-8")),
    )
    report["observedAtUtc"] = datetime.now(timezone.utc).isoformat()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("Codex stable release watch: " + report["status"])
    if report["status"] != "pinned-stable-current":
        print("::warning::Codex stable metadata drift or unavailable: review pinned protocol fixtures; L3 remains unqualified.")


if __name__ == "__main__":
    main()
