#!/usr/bin/env python3
import argparse
import json
import os
import pathlib
import subprocess
import tarfile
import tempfile
import zipfile


def extract(archive: pathlib.Path, destination: pathlib.Path) -> pathlib.Path:
    if archive.name.endswith(".tar.gz"):
        with tarfile.open(archive, "r:gz") as handle:
            handle.extractall(destination, filter="data")
    elif archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as handle:
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
            "THIRD_PARTY_NOTICES.txt",
            "V1-STABLE-CRITERIA.json",
            "RELEASE-METADATA.json",
        ]
        missing = [name for name in required if not (root / name).is_file()]
        if missing:
            raise SystemExit("archive missing required files: " + ", ".join(missing))

        license_text = (root / "LICENSE").read_text(encoding="utf-8")
        if "Apache License" not in license_text or "Version 2.0" not in license_text:
            raise SystemExit("archive LICENSE is not Apache License 2.0")

        metadata = json.loads((root / "RELEASE-METADATA.json").read_text(encoding="utf-8"))
        expected = {
            "version": args.version,
            "tag": args.tag,
            "commitSha": args.commit,
            "license": "Apache-2.0",
        }
        for key, value in expected.items():
            if metadata.get(key) != value:
                raise SystemExit(f"metadata {key} mismatch: {metadata.get(key)!r} != {value!r}")

        version_output = run_checked([str(binary), "--version"]).strip()
        if version_output != f"codex-tui {args.version}":
            raise SystemExit(f"version smoke mismatch: {version_output!r}")

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
