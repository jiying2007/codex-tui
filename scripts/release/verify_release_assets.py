#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re

from _compat import write_text_lf

SCHEMA = "codex-tui/release-asset-verification/v1"
SHA256 = re.compile(r"^sha256:([0-9a-f]{64})$")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def local_assets(root: pathlib.Path) -> dict:
    require(root.is_dir(), f"bundle directory does not exist: {root}")
    result = {}
    for path in sorted(root.iterdir(), key=lambda value: value.name):
        require(not path.is_symlink(), f"bundle entry must not be a symlink: {path.name}")
        require(path.is_file(), f"bundle entry must be a regular file: {path.name}")
        result[path.name] = {
            "sha256": sha256(path),
            "size": path.stat().st_size,
        }
    require(result, "bundle contains no release assets")
    return result


def remote_assets(release: dict) -> dict:
    assets = release.get("assets")
    require(isinstance(assets, list), "release JSON assets must be a list")
    result = {}
    for asset in assets:
        require(isinstance(asset, dict), "release asset must be an object")
        name = asset.get("name")
        require(isinstance(name, str) and name, "release asset name is missing")
        require("/" not in name and "\\" not in name, f"release asset has unsafe name: {name!r}")
        require(name not in result, f"release contains duplicate asset name: {name}")
        require(asset.get("state") == "uploaded", f"release asset is not uploaded: {name}")
        size = asset.get("size")
        require(
            isinstance(size, int) and not isinstance(size, bool) and size >= 0,
            f"release asset size is invalid: {name}",
        )
        digest = asset.get("digest")
        match = SHA256.fullmatch(digest) if isinstance(digest, str) else None
        require(match is not None, f"release asset SHA-256 digest is missing or invalid: {name}")
        result[name] = {
            "sha256": match.group(1),
            "size": size,
            "id": asset.get("id"),
        }
    return result


def validate_release_state(release: dict, phase: str, channel: str) -> None:
    require(channel in ("preview", "stable"), "release channel must be preview or stable")
    require(phase in ("draft", "prepublish", "published"), "release phase is invalid")
    if phase == "prepublish":
        require(channel == "stable", "prepublish phase is stable-only")

    draft = release.get("draft")
    prerelease = release.get("prerelease")
    immutable = release.get("immutable")
    require(isinstance(draft, bool), "release draft state is missing or invalid")
    require(isinstance(prerelease, bool), "release prerelease state is missing or invalid")
    require(isinstance(release.get("id"), int) and release["id"] > 0, "release id is missing or invalid")

    if phase in ("draft", "prepublish"):
        require(draft is True, f"{phase} release must remain draft=true")
        require(prerelease is False, f"{phase} release must remain prerelease=false")
    else:
        require(draft is False, "published release must have draft=false")
        if channel == "preview":
            require(prerelease is True, "published preview must have prerelease=true")
        else:
            require(prerelease is False, "published stable release must have prerelease=false")
            require(immutable is True, "published stable release must have immutable=true")


def verify(release: dict, root: pathlib.Path, tag: str) -> list:
    require(release.get("tag_name") == tag, "release tag mismatch")
    local = local_assets(root)
    remote = remote_assets(release)
    require(
        set(local) == set(remote),
        "GitHub release asset set does not exactly match local bundle: "
        f"local={sorted(local)} remote={sorted(remote)}",
    )
    rows = []
    for name in sorted(local):
        require(
            local[name]["size"] == remote[name]["size"],
            f"GitHub release asset size mismatch: {name}",
        )
        require(
            local[name]["sha256"] == remote[name]["sha256"],
            f"GitHub release asset SHA-256 mismatch: {name}",
        )
        rows.append(
            {
                "name": name,
                "sha256": local[name]["sha256"],
                "size": local[name]["size"],
                "assetId": remote[name]["id"],
            }
        )
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify GitHub release asset names, sizes and SHA-256 digests against a local bundle."
    )
    parser.add_argument("--release-json", required=True)
    parser.add_argument("--bundle", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--phase", choices=["draft", "prepublish", "published"], required=True)
    parser.add_argument("--channel", choices=["preview", "stable"], required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    try:
        release = json.loads(pathlib.Path(args.release_json).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read release JSON: {error}") from error
    require(isinstance(release, dict), "release JSON must be an object")

    validate_release_state(release, args.phase, args.channel)
    rows = verify(release, pathlib.Path(args.bundle), args.tag)
    receipt = {
        "schema": SCHEMA,
        "phase": args.phase,
        "channel": args.channel,
        "releaseId": release.get("id"),
        "tag": args.tag,
        "draft": release.get("draft"),
        "prerelease": release.get("prerelease"),
        "immutable": release.get("immutable"),
        "assets": rows,
        "authority": "github-rest-asset-digest-via-api-version-2026-03-10",
    }
    write_text_lf(
        pathlib.Path(args.output),
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
    )
    print(f"VALID GitHub release assets match local bundle: {len(rows)} assets ({args.phase})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
