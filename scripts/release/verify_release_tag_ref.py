#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re

from _compat import write_text_lf

SCHEMA = "codex-tui/release-tag-ref/v1"
HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def validate_tag_ref(ref: dict, tag: str, source_sha: str) -> dict:
    require(isinstance(ref, dict), "tag ref metadata must be a JSON object")
    expected_ref = f"refs/tags/{tag}"
    require(ref.get("ref") == expected_ref, "tag ref name mismatch")
    obj = ref.get("object")
    require(isinstance(obj, dict), "tag ref object is missing")
    require(obj.get("type") == "commit", "release tag must be a lightweight commit ref")
    observed_sha = obj.get("sha")
    require(
        isinstance(observed_sha, str) and bool(HEX40.fullmatch(observed_sha)),
        "tag ref object SHA must be exactly 40 hexadecimal characters",
    )
    require(
        observed_sha.lower() == source_sha,
        "release tag ref does not match the expected source SHA",
    )
    return {
        "ref": expected_ref,
        "objectType": "commit",
        "objectSha": observed_sha.lower(),
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate and retain the exact GitHub release tag ref identity."
    )
    parser.add_argument("--ref-json", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--phase", choices=["draft", "prepublish", "published"], required=True)
    parser.add_argument("--channel", choices=["preview", "stable"], required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    if args.phase == "prepublish" and args.channel != "stable":
        raise SystemExit("prepublish tag evidence is stable-only")

    source_sha = args.source_sha.strip().lower()
    require(
        bool(HEX40.fullmatch(source_sha)),
        "--source-sha must be exactly 40 hexadecimal characters",
    )
    path = pathlib.Path(args.ref_json)
    try:
        ref = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read tag ref metadata: {error}") from error

    identity = validate_tag_ref(ref, args.tag, source_sha)
    receipt = {
        "schema": SCHEMA,
        "phase": args.phase,
        "channel": args.channel,
        "sourceSha": source_sha,
        "tag": args.tag,
        **identity,
        "refSnapshotSha256": sha256(path),
        "authority": "github-rest-git-tag-ref-readback",
    }
    write_text_lf(
        pathlib.Path(args.output),
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
    )
    print(
        f"VALID release tag ref: {args.tag} -> {source_sha} "
        f"({args.channel}/{args.phase})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
