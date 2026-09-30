#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import pathlib
import subprocess
import sys
import tarfile
from typing import Any, Dict


MIN_PYTHON = (3, 8)


def require_python38() -> None:
    if sys.version_info < MIN_PYTHON:
        raise SystemExit(
            "codex-tui release tools require Python 3.8+; "
            f"got {sys.version_info.major}.{sys.version_info.minor}"
        )


def write_text_lf(path: pathlib.Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write(text)


def cargo_package(root: pathlib.Path, package_name: str = "codex-tui") -> Dict[str, Any]:
    proc = subprocess.run(
        [
            "cargo",
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--no-deps",
        ],
        cwd=str(root),
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    metadata = json.loads(proc.stdout)
    matches = [
        package for package in metadata.get("packages", [])
        if package.get("name") == package_name
    ]
    if len(matches) != 1:
        raise SystemExit(
            f"expected exactly one Cargo package named {package_name!r}, found {len(matches)}"
        )
    return matches[0]


def safe_extract_tar(handle: tarfile.TarFile, destination: pathlib.Path) -> None:
    destination = destination.resolve()
    members = handle.getmembers()
    for member in members:
        if member.issym() or member.islnk() or member.isdev():
            raise SystemExit(f"unsafe archive member type: {member.name}")
        target = (destination / member.name).resolve()
        try:
            common = os.path.commonpath([str(destination), str(target)])
        except ValueError:
            raise SystemExit(f"unsafe archive member path: {member.name}")
        if common != str(destination):
            raise SystemExit(f"archive member escapes destination: {member.name}")
    handle.extractall(str(destination), members=members)


require_python38()
