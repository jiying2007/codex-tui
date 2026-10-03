#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re

HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Validate that the release workflow is still bound to the current main "
            "branch head and, for publication, that main is protected."
        )
    )
    parser.add_argument("--branch-json", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--require-protected", action="store_true")
    args = parser.parse_args()

    commit = args.commit.strip().lower()
    if not HEX40.fullmatch(commit):
        raise SystemExit("--commit must be exactly 40 hexadecimal characters")

    try:
        branch = json.loads(pathlib.Path(args.branch_json).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read branch metadata: {error}") from error

    if not isinstance(branch, dict):
        raise SystemExit("branch metadata must be a JSON object")
    if branch.get("name") != "main":
        raise SystemExit(f"release branch metadata must describe main; got {branch.get('name')!r}")

    observed_sha = str((branch.get("commit") or {}).get("sha", "")).strip().lower()
    if observed_sha != commit:
        raise SystemExit(
            "main moved away from the release commit: "
            f"release={commit} current-main={observed_sha or '<missing>'}"
        )

    if args.require_protected and branch.get("protected") is not True:
        raise SystemExit(
            "stable publication requires main.protected=true; "
            "configure branch protection or an applicable repository ruleset first"
        )

    print(
        f"VALID release branch state: main={commit} "
        f"protected={branch.get('protected') is True}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
