#!/usr/bin/env python3
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import tarfile
import tempfile
import zipfile

from _compat import cargo_package, write_text_lf
from check_linux_abi import inspect_binary
from archive_identity import validate_metadata
from archive_doc_links import rewrite_packaged_links


def verbose_tool_identity(command: list[str], name: str) -> dict:
    proc = subprocess.run(
        command,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    lines = [line.strip() for line in proc.stdout.splitlines() if line.strip()]
    if not lines or not lines[0].startswith(name + " "):
        raise RuntimeError(f"{name} verbose version did not report an expected header")

    fields = {}
    for line in lines[1:]:
        key, separator, value = line.partition(": ")
        if separator:
            fields[key] = value.strip()

    required = ("release", "commit-hash", "commit-date", "host")
    missing = [key for key in required if not fields.get(key)]
    if missing:
        raise RuntimeError(
            f"{name} verbose version is missing fields: {', '.join(missing)}"
        )

    identity = {
        "version": lines[0],
        "release": fields["release"],
        "commitHash": fields["commit-hash"],
        "commitDate": fields["commit-date"],
        "host": fields["host"],
    }
    if name == "rustc" and fields.get("LLVM version"):
        identity["llvmVersion"] = fields["LLVM version"]
    return identity


def build_toolchain_identity() -> tuple[str, dict]:
    rustc = verbose_tool_identity(["rustc", "-vV"], "rustc")
    cargo = verbose_tool_identity(["cargo", "-Vv"], "cargo")
    if rustc["host"] != cargo["host"]:
        raise RuntimeError(
            "rustc/cargo host mismatch: "
            f"{rustc['host']!r} != {cargo['host']!r}"
        )
    return rustc["host"], {"rustc": rustc, "cargo": cargo}


def copy_file(source: pathlib.Path, destination: pathlib.Path, executable: bool = False) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    os.chmod(destination, 0o755 if executable else 0o644)


def add_tar_tree(tar: tarfile.TarFile, root: pathlib.Path, prefix: str) -> None:
    for path in sorted(root.rglob("*"), key=lambda value: value.as_posix()):
        relative = path.relative_to(root).as_posix()
        arcname = f"{prefix}/{relative}" if relative else prefix
        info = tar.gettarinfo(str(path), arcname)
        info.uid = 0
        info.gid = 0
        info.uname = ""
        info.gname = ""
        info.mtime = 0
        if path.is_file():
            with path.open("rb") as handle:
                tar.addfile(info, handle)
        else:
            tar.addfile(info)


def create_tar_gz(stage: pathlib.Path, package_name: str, output: pathlib.Path) -> None:
    with output.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as gz:
            with tarfile.open(fileobj=gz, mode="w") as tar:
                add_tar_tree(tar, stage, package_name)


def create_zip(stage: pathlib.Path, package_name: str, output: pathlib.Path) -> None:
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(stage.rglob("*"), key=lambda value: value.as_posix()):
            if not path.is_file():
                continue
            relative = path.relative_to(stage).as_posix()
            info = zipfile.ZipInfo(f"{package_name}/{relative}")
            info.date_time = (1980, 1, 1, 0, 0, 0)
            info.compress_type = zipfile.ZIP_DEFLATED
            mode = 0o755 if path.name in ("codex-tui", "codex-tui.exe") else 0o644
            info.external_attr = mode << 16
            archive.writestr(info, path.read_bytes())


def stage_active_bilingual_docs(root: pathlib.Path, stage: pathlib.Path) -> None:
    """Copy only declared current documentation, never untracked local files."""
    manifest_path = root / "docs/i18n/manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("schema") != "codex-tui/bilingual-docs/v1":
        raise SystemExit("invalid bilingual documentation manifest during packaging")
    pairs = manifest.get("pairs")
    if not isinstance(pairs, list) or not pairs:
        raise SystemExit("no active bilingual package pages")
    copy_file(manifest_path, stage / "docs/i18n/manifest.json")
    seen = set()
    for pair in pairs:
        if not isinstance(pair, dict):
            raise SystemExit("malformed bilingual package record")
        for locale in ("en", "zh-CN"):
            raw = pair.get(locale)
            if not isinstance(raw, str) or not raw.endswith(".md"):
                raise SystemExit("invalid bilingual package path")
            relative = pathlib.Path(raw)
            source = (root / relative).resolve()
            try:
                source.relative_to(root.resolve())
            except ValueError:
                raise SystemExit("bilingual package path escapes repository")
            if relative.is_absolute() or raw in seen or not source.is_file():
                raise SystemExit("missing, duplicate or absolute bilingual package file")
            seen.add(raw)
            copy_file(source, stage / relative)

    # Legacy English aliases remain available next to the binary. These
    # localized aliases make the same instructions discoverable offline.
    for source, alias in (
        ("docs/zh-CN/release/install-upgrade.md", "INSTALL-UPGRADE.zh-CN.md"),
        ("docs/zh-CN/team-quickstart.md", "TEAM-QUICKSTART.zh-CN.md"),
    ):
        copy_file(root / source, stage / alias)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--platform", choices=["linux", "macos", "windows"], required=True)
    parser.add_argument("--notices", required=True)
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()

    root = pathlib.Path.cwd()
    package = cargo_package(root)
    license_spdx = package.get("license")
    if license_spdx != "Apache-2.0":
        raise SystemExit(
            f"release packaging requires Cargo license Apache-2.0; got {license_spdx!r}"
        )
    project_license = root / "LICENSE"
    if not project_license.is_file():
        raise SystemExit("release packaging requires project LICENSE")

    binary = pathlib.Path(args.binary)
    notices = pathlib.Path(args.notices)
    if not binary.is_file():
        raise SystemExit(f"binary does not exist: {binary}")
    if not notices.is_file():
        raise SystemExit(f"notices do not exist: {notices}")

    version_parts = args.version.split(".")
    if len(version_parts) != 3 or not all(part.isdigit() for part in version_parts):
        raise SystemExit(f"release version must be X.Y.Z; got {args.version!r}")
    criteria = root / "release" / f"v{version_parts[0]}.{version_parts[1]}-criteria.json"
    if not criteria.is_file():
        raise SystemExit(f"release criteria file is missing: {criteria}")

    triple, build_toolchain = build_toolchain_identity()
    package_name = f"codex-tui-{args.version}-{triple}"
    output_dir = pathlib.Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory() as temp:
        stage = pathlib.Path(temp) / package_name
        stage.mkdir()
        binary_name = "codex-tui.exe" if args.platform == "windows" else "codex-tui"
        copy_file(binary, stage / binary_name, executable=True)
        copy_file(root / "README.md", stage / "README.md")
        copy_file(root / "CHANGELOG.md", stage / "CHANGELOG.md")
        copy_file(
            root / "docs/release/install-upgrade.md",
            stage / "INSTALL-UPGRADE.md",
        )
        copy_file(
            root / "docs/team-quickstart.md",
            stage / "TEAM-QUICKSTART.md",
        )
        stage_active_bilingual_docs(root, stage)
        copy_file(notices, stage / "THIRD_PARTY_NOTICES.txt")
        copy_file(criteria, stage / "STABLE-CRITERIA.json")

        copy_file(project_license, stage / "LICENSE")

        metadata = {
            "schema": "codex-tui/release-artifact/v2",
            "version": args.version,
            "tag": args.tag,
            "commitSha": args.commit,
            "platform": args.platform,
            "hostTriple": triple,
            "buildToolchain": build_toolchain,
            "binary": binary_name,
            "binarySha256": hashlib.sha256((stage / binary_name).read_bytes()).hexdigest(),
            "license": license_spdx,
        }
        validate_metadata(metadata, stage / binary_name, args.version, args.tag, args.commit)
        if args.platform == "linux":
            abi = inspect_binary(binary, args.commit)
            metadata["linuxRuntime"] = {"minimumGlibc": abi["maximumGlibc"], "baseline": abi["baseline"], "evidence": "LINUX-ABI.json"}
            write_text_lf(stage / "LINUX-ABI.json", json.dumps(abi, indent=2, sort_keys=True) + "\n")
        write_text_lf(
            stage / "RELEASE-METADATA.json",
            json.dumps(metadata, indent=2, sort_keys=True) + "\n",
        )

        # All shipped current guides must remain navigable. Unbundled historical
        # references point to immutable source-SHA GitHub files, not dead paths.
        rewrite_packaged_links(stage, root, args.commit)

        if args.platform == "windows":
            output = output_dir / f"{package_name}.zip"
            create_zip(stage, package_name, output)
        else:
            output = output_dir / f"{package_name}.tar.gz"
            create_tar_gz(stage, package_name, output)

    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
