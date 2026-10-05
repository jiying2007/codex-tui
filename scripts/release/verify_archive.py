#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import stat
import tarfile
import tempfile
import zipfile

from _compat import safe_extract_tar
from check_linux_abi import MAX_GLIBC, version
from archive_identity import validate_metadata, validate_compiled_source, validate_members


def extract(archive: pathlib.Path, destination: pathlib.Path) -> pathlib.Path:
    if archive.name.endswith(".tar.gz"):
        with tarfile.open(archive, "r:gz") as handle:
            validate_members([(entry.name, entry.isdir(), entry.isfile(), entry.size) for entry in handle.getmembers()])
            safe_extract_tar(handle, destination)
    elif archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as handle:
            validate_members([(entry.filename, entry.is_dir(),
                               stat.S_IFMT(entry.external_attr >> 16) in (0, stat.S_IFREG), entry.file_size)
                              for entry in handle.infolist()])
            handle.extractall(destination)
    else:
        raise SystemExit(f"unsupported archive: {archive}")

    roots = [path for path in destination.iterdir() if path.is_dir()]
    if len(roots) != 1:
        raise SystemExit(f"archive must contain exactly one top-level directory: {roots}")
    return roots[0]


def run_checked(command: list[str]) -> str:
    proc = subprocess.run(
        command,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=60,
    )
    return proc.stdout


def main() -> int:
    parser = argparse.ArgumentParser()
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--archive")
    source.add_argument("--archive-dir")
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()

    if args.archive:
        archive = pathlib.Path(args.archive)
    else:
        directory = pathlib.Path(args.archive_dir)
        archives = sorted(
            path
            for path in directory.iterdir()
            if path.is_file() and (path.name.endswith(".tar.gz") or path.suffix == ".zip")
        )
        if len(archives) != 1:
            raise SystemExit(
                f"expected exactly one release archive in {directory}, found {len(archives)}"
            )
        archive = archives[0]

    with tempfile.TemporaryDirectory() as temp:
        root = extract(archive, pathlib.Path(temp))
        binary = root / ("codex-tui.exe" if os.name == "nt" else "codex-tui")
        if not binary.is_file():
            raise SystemExit(f"binary missing from archive: {binary}")
        if os.name != "nt":
            binary.chmod(binary.stat().st_mode | 0o111)

        required = [
            "LICENSE",
            "README.md",
            "CHANGELOG.md",
            "INSTALL-UPGRADE.md",
            "TEAM-QUICKSTART.md",
            "THIRD_PARTY_NOTICES.txt",
            "STABLE-CRITERIA.json",
            "RELEASE-METADATA.json",
        ]
        missing = [name for name in required if not (root / name).is_file()]
        if missing:
            raise SystemExit("archive missing required files: " + ", ".join(missing))

        license_text = (root / "LICENSE").read_text(encoding="utf-8")
        if "Apache License" not in license_text or "Version 2.0" not in license_text:
            raise SystemExit("archive LICENSE is not Apache License 2.0")

        criteria = json.loads((root / "STABLE-CRITERIA.json").read_text(encoding="utf-8"))
        if criteria.get("schema") != "codex-tui/stable-criteria/v2":
            raise SystemExit("archive stable criteria schema mismatch")
        if criteria.get("stableVersion") != args.version:
            raise SystemExit(
                f"archive stable criteria version mismatch: {criteria.get('stableVersion')!r} != {args.version!r}"
            )

        metadata = json.loads((root / "RELEASE-METADATA.json").read_text(encoding="utf-8"))
        validate_metadata(metadata, binary, args.version, args.tag, args.commit)

        if metadata.get("platform") == "linux":
            abi_path = root / "LINUX-ABI.json"
            if not abi_path.is_file():
                raise SystemExit("Linux archive missing ABI receipt")
            abi = json.loads(abi_path.read_text(encoding="utf-8"))
            if (abi.get("schema") != "codex-tui/linux-abi/v1" or abi.get("passed") is not True
                or abi.get("sourceSha") != args.commit
                or abi.get("binarySha256") != hashlib.sha256(binary.read_bytes()).hexdigest()
                or abi.get("maximumGlibc") != MAX_GLIBC
                or version(abi.get("requiredGlibc", "invalid")) > version(MAX_GLIBC)
                or metadata.get("linuxRuntime", {}).get("minimumGlibc") != MAX_GLIBC):
                raise SystemExit("Linux ABI receipt or binary binding mismatch")

        if "Usage:" not in run_checked([str(binary), "--help"]):
            raise SystemExit("archive --help smoke failed")
        invalid = subprocess.run([str(binary), "--invalid-archive-smoke-option"], capture_output=True, timeout=10)
        if invalid.returncode != 2:
            raise SystemExit("archive invalid-usage exit code mismatch")
        version_output = run_checked([str(binary), "--version"]).strip()
        if version_output != f"codex-tui {args.version}":
            raise SystemExit(f"version smoke mismatch: {version_output!r}")

        # Use an existing no-backend diagnostic to read the embedded build identity.
        # One sample is sufficient for identity; this is NOT performance evidence.
        identity = json.loads(run_checked([
            str(binary), "release", "benchmark", "--warmup", "1", "--iterations", "1",
            "--source", "archive-identity-smoke", "--json",
        ]))
        validate_compiled_source(identity, args.commit)

        fixture_output = run_checked(
            [str(binary), "headless", "threads", "--fixture-10k", "--json"]
        )
        fixture = json.loads(fixture_output)
        if fixture.get("schema") != "codex-tui/headless-threads/v1":
            raise SystemExit("headless fixture schema mismatch")
        if fixture.get("degraded") is not False:
            raise SystemExit("headless fixture unexpectedly degraded")
        if len(fixture.get("threads", [])) != 10_000:
            raise SystemExit("headless fixture did not contain 10,000 threads")

    print(f"VERIFIED {archive}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
