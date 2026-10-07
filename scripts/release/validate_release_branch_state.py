#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re

from _compat import write_text_lf

SCHEMA = "codex-tui/release-branch-state/v1"
HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def validate_branch_state(branch: dict, commit: str, require_protected: bool) -> dict:
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

    protected = branch.get("protected") is True
    if require_protected and not protected:
        raise SystemExit(
            "stable publication requires main.protected=true; "
            "configure branch protection or an applicable repository ruleset first"
        )

    return {
        "schema": SCHEMA,
        "sourceSha": commit,
        "mainSha": observed_sha,
        "protected": protected,
        "requireProtected": require_protected,
        "authority": "github-rest-main-branch-readback",
    }


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
    parser.add_argument("--output")
    args = parser.parse_args()

    commit = args.commit.strip().lower()
    if not HEX40.fullmatch(commit):
        raise SystemExit("--commit must be exactly 40 hexadecimal characters")

    branch_path = pathlib.Path(args.branch_json)
    try:
        branch = json.loads(branch_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read branch metadata: {error}") from error

    receipt = validate_branch_state(branch, commit, args.require_protected)
    receipt["branchSnapshotSha256"] = sha256(branch_path)

    if args.output:
        write_text_lf(
            pathlib.Path(args.output),
            json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        )

    print(
        f"VALID release branch state: main={commit} "
        f"protected={receipt['protected']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
