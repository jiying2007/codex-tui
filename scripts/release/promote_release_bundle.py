#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re

from _compat import write_text_lf

SCHEMA = "codex-tui/stable-bundle-promotion/v1"
VERIFY_SCHEMA = "codex-tui/release-verification/v1"
EVIDENCE_SCHEMA = "codex-tui/release-evidence/v5"
HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json(path: pathlib.Path, label: str) -> dict:
    require(path.is_file() and not path.is_symlink(), f"{label} is missing or unsafe")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read {label}: {error}") from error
    require(isinstance(value, dict), f"{label} must be a JSON object")
    return value


def validate_manifest(root: pathlib.Path) -> list:
    manifest_path = root / "SHA256SUMS"
    require(
        manifest_path.is_file() and not manifest_path.is_symlink(),
        "qualified bundle SHA256SUMS is missing or unsafe",
    )
    entries = {}
    for number, raw in enumerate(
        manifest_path.read_text(encoding="utf-8").splitlines(),
        start=1,
    ):
        digest, separator, name = raw.partition("  ")
        require(separator == "  ", f"SHA256SUMS line {number} is malformed")
        require(bool(HEX64.fullmatch(digest)), f"SHA256SUMS line {number} has invalid digest")
        require(name and "/" not in name and "\\" not in name, f"SHA256SUMS line {number} has unsafe filename")
        require(name != "SHA256SUMS", "SHA256SUMS must not checksum itself")
        require(name not in entries, f"SHA256SUMS contains duplicate entry: {name}")
        entries[name] = digest

    actual = {}
    for path in root.iterdir():
        require(not path.is_symlink(), f"qualified bundle entry must not be a symlink: {path.name}")
        require(path.is_file(), f"qualified bundle entry must be a regular file: {path.name}")
        if path.name == "SHA256SUMS":
            continue
        actual[path.name] = path

    require(entries, "qualified bundle SHA256SUMS is empty")
    require(
        set(entries) == set(actual),
        "qualified bundle SHA256SUMS set does not exactly cover bundle files: "
        f"manifest={sorted(entries)} actual={sorted(actual)}",
    )

    rows = []
    for name in sorted(actual):
        digest = sha256(actual[name])
        require(entries[name] == digest, f"qualified bundle SHA256SUMS mismatch: {name}")
        rows.append({"name": name, "sha256": digest, "size": actual[name].stat().st_size})
    return rows


def validate_bundle(root: pathlib.Path, version: str, tag: str, source_sha: str) -> list:
    require(root.is_dir(), f"qualified bundle directory does not exist: {root}")
    files = validate_manifest(root)

    archives = [
        row
        for row in files
        if row["name"].startswith(f"codex-tui-{version}-")
        and (row["name"].endswith(".tar.gz") or row["name"].endswith(".zip"))
    ]
    require(
        len(archives) == 3,
        f"qualified bundle must contain exactly three native archives for {version}; "
        f"found {[row['name'] for row in archives]}",
    )
    require(
        sum(row["name"].endswith(".tar.gz") for row in archives) == 2
        and sum(row["name"].endswith(".zip") for row in archives) == 1,
        "qualified bundle native archives must be two .tar.gz and one .zip",
    )

    verification = load_json(root / "release-verification.json", "release-verification.json")
    require(verification.get("schema") == VERIFY_SCHEMA, "qualified release verification schema mismatch")
    require(verification.get("channel") == "stable", "qualified release verification must be stable")
    require(verification.get("tag") == tag, "qualified release verification tag mismatch")
    require(verification.get("version") == version, "qualified release verification version mismatch")
    require(
        str(verification.get("commitSha", "")).lower() == source_sha.lower(),
        "qualified release verification source SHA mismatch",
    )
    require(verification.get("publish") is False, "promoted bundle must originate from publish=false")
    require(verification.get("valid") is True, "qualified release verification is not valid")
    require(verification.get("blockers") == [], "qualified release verification has blockers")
    require(verification.get("evidenceStatus") == "verified", "qualified release evidence was not verified")

    evidence = load_json(root / "release-evidence.json", "release-evidence.json")
    require(evidence.get("schema") == EVIDENCE_SCHEMA, "qualified release evidence schema mismatch")
    require(evidence.get("version") == version, "qualified release evidence version mismatch")
    require(
        str(evidence.get("commitSha", "")).lower() == source_sha.lower(),
        "qualified release evidence source SHA mismatch",
    )

    require((root / "RELEASE_NOTES.md").is_file(), "qualified bundle RELEASE_NOTES.md is missing")
    require((root / "STABLE-CRITERIA.json").is_file(), "qualified bundle STABLE-CRITERIA.json is missing")
    return files


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Validate a retained stable publish=false release-bundle as the exact bytes "
            "that will be promoted to GitHub Release."
        )
    )
    parser.add_argument("--bundle", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--qualification-run", required=True, type=int)
    parser.add_argument("--publication-run", required=True, type=int)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    require(bool(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", args.version)), "--version must be X.Y.Z")
    require(args.tag == f"v{args.version}", "--tag must exactly match v<version>")
    require(bool(HEX40.fullmatch(args.source_sha)), "--source-sha must be exactly 40 hexadecimal characters")
    require(args.qualification_run > 0 and args.publication_run > 0, "workflow run IDs must be positive")

    files = validate_bundle(
        pathlib.Path(args.bundle),
        args.version,
        args.tag,
        args.source_sha,
    )
    receipt = {
        "schema": SCHEMA,
        "version": args.version,
        "tag": args.tag,
        "sourceSha": args.source_sha.lower(),
        "qualifiedRun": args.qualification_run,
        "publicationRun": args.publication_run,
        "files": files,
        "authority": "exact-retained-publish-false-release-bundle-promotion",
    }
    write_text_lf(
        pathlib.Path(args.output),
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
    )
    print(
        f"VALID stable bundle promoted from dry-run {args.qualification_run}: "
        f"{len(files)} checksummed files"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
