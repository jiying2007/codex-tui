#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re

from _compat import write_text_lf

SCHEMA = "codex-tui/stable-package-equivalence/v1"
HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def release_archives(root: pathlib.Path, version: str, label: str) -> dict:
    require(root.is_dir(), f"{label} directory does not exist: {root}")
    prefix = f"codex-tui-{version}-"
    archives = {}
    for path in root.iterdir():
        if not path.name.startswith(prefix):
            continue
        if not (path.name.endswith(".tar.gz") or path.name.endswith(".zip")):
            continue
        require(not path.is_symlink(), f"{label} archive must not be a symlink: {path.name}")
        require(path.is_file(), f"{label} archive is not a regular file: {path.name}")
        archives[path.name] = path

    require(
        len(archives) == 3,
        f"{label} must contain exactly three native archives for {version}; "
        f"found {sorted(archives)}",
    )
    require(
        sum(name.endswith(".zip") for name in archives) == 1
        and sum(name.endswith(".tar.gz") for name in archives) == 2,
        f"{label} archive extensions must be two .tar.gz and one .zip",
    )
    return archives


def checksum_manifest(root: pathlib.Path, label: str) -> dict:
    path = root / "SHA256SUMS"
    require(path.is_file() and not path.is_symlink(), f"{label} SHA256SUMS is missing or unsafe")
    entries = {}
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        digest, separator, name = raw.partition("  ")
        require(separator == "  ", f"{label} SHA256SUMS line {number} is malformed")
        require(bool(HEX64.fullmatch(digest)), f"{label} SHA256SUMS line {number} has invalid digest")
        require(name != "" and "/" not in name and "\\" not in name, f"{label} SHA256SUMS line {number} has unsafe filename")
        require(name not in entries, f"{label} SHA256SUMS contains duplicate entry: {name}")
        entries[name] = digest
    require(entries, f"{label} SHA256SUMS is empty")
    return entries


def validated_archive_digests(root: pathlib.Path, version: str, label: str) -> dict:
    archives = release_archives(root, version, label)
    manifest = checksum_manifest(root, label)
    result = {}
    for name, path in sorted(archives.items()):
        digest = sha256(path)
        require(name in manifest, f"{label} SHA256SUMS is missing archive entry: {name}")
        require(
            manifest[name] == digest,
            f"{label} SHA256SUMS does not match archive bytes: {name}",
        )
        result[name] = {"sha256": digest, "size": path.stat().st_size}
    return result


def compare_release_archives(
    prior_dir: pathlib.Path,
    current_dir: pathlib.Path,
    version: str,
) -> list:
    prior = validated_archive_digests(prior_dir, version, "prior stable dry-run bundle")
    current = validated_archive_digests(current_dir, version, "current publication bundle")
    require(
        set(prior) == set(current),
        "stable native archive filenames drifted from the qualified dry-run: "
        f"prior={sorted(prior)} current={sorted(current)}",
    )

    rows = []
    for name in sorted(prior):
        require(
            prior[name]["sha256"] == current[name]["sha256"],
            f"stable native archive bytes drifted from the qualified dry-run: {name}",
        )
        require(
            prior[name]["size"] == current[name]["size"],
            f"stable native archive size drifted from the qualified dry-run: {name}",
        )
        rows.append(
            {
                "name": name,
                "sha256": current[name]["sha256"],
                "size": current[name]["size"],
            }
        )
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Fail closed unless the stable publication native archives are byte-for-byte "
            "identical to the prior qualified stable publish=false release bundle."
        )
    )
    parser.add_argument("--prior-dir", required=True)
    parser.add_argument("--current-dir", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--prior-run", required=True, type=int)
    parser.add_argument("--current-run", required=True, type=int)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    require(bool(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", args.version)), "--version must be X.Y.Z")
    source_sha = args.source_sha.lower()
    require(bool(HEX40.fullmatch(source_sha)), "--source-sha must be exactly 40 hexadecimal characters")
    require(args.prior_run > 0 and args.current_run > 0, "workflow run IDs must be positive")

    archives = compare_release_archives(
        pathlib.Path(args.prior_dir),
        pathlib.Path(args.current_dir),
        args.version,
    )
    receipt = {
        "schema": SCHEMA,
        "version": args.version,
        "sourceSha": source_sha,
        "priorStableDryRun": args.prior_run,
        "publicationRun": args.current_run,
        "archives": archives,
        "authority": "exact-byte-equality-against-prior-stable-publish-false-release-bundle",
    }
    write_text_lf(
        pathlib.Path(args.output),
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
    )
    print(
        "VALID stable native archives are byte-identical to dry-run "
        f"{args.prior_run}: {len(archives)} archives"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
