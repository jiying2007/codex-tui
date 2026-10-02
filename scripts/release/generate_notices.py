#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys

from _compat import write_text_lf

LICENSE_PREFIXES = ("license", "copying", "notice", "unlicense", "copyright")
MAX_LICENSE_BYTES = 1024 * 1024


def cargo_metadata() -> dict:
    proc = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1"],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    return json.loads(proc.stdout)


def normal_dependency_ids(metadata: dict) -> set[str]:
    resolve = metadata.get("resolve") or {}
    root = resolve.get("root")
    if not root:
        raise SystemExit("cargo metadata did not report a root package")

    nodes = {node["id"]: node for node in resolve.get("nodes", [])}
    seen = {root}
    stack = [root]
    while stack:
        package_id = stack.pop()
        node = nodes.get(package_id)
        if not node:
            continue
        for dep in node.get("deps", []):
            dep_kinds = dep.get("dep_kinds") or []
            is_normal = not dep_kinds or any(kind.get("kind") is None for kind in dep_kinds)
            if not is_normal:
                continue
            target = dep["pkg"]
            if target not in seen:
                seen.add(target)
                stack.append(target)
    seen.discard(root)
    return seen


def candidate_license_files(package: dict) -> list[pathlib.Path]:
    manifest = pathlib.Path(package["manifest_path"])
    package_dir = manifest.parent
    candidates: list[pathlib.Path] = []

    license_file = package.get("license_file")
    if license_file:
        path = pathlib.Path(license_file)
        if not path.is_absolute():
            path = package_dir / path
        if path.is_file():
            candidates.append(path)

    try:
        entries = sorted(package_dir.iterdir(), key=lambda path: path.name.lower())
    except OSError:
        entries = []
    for path in entries:
        if not path.is_file():
            continue
        lowered = path.name.lower()
        if lowered.startswith(LICENSE_PREFIXES):
            candidates.append(path)

    unique = []
    seen = set()
    for path in candidates:
        resolved = path.resolve()
        if resolved not in seen:
            seen.add(resolved)
            unique.append(path)
    return unique


def read_license(path: pathlib.Path) -> str:
    data = path.read_bytes()
    if len(data) > MAX_LICENSE_BYTES:
        raise RuntimeError(f"license file is unexpectedly large: {path} ({len(data)} bytes)")
    return data.decode("utf-8", errors="replace").replace("\r\n", "\n").replace("\r", "\n")


def embedded_asset_notices(package_names: set[str]) -> str | None:
    if "two-face" not in package_names:
        return None
    proc = subprocess.run(
        [
            "cargo",
            "run",
            "--release",
            "--locked",
            "--quiet",
            "--bin",
            "syntax-asset-notices",
        ],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    text = proc.stdout.replace("\r\n", "\n").replace("\r", "\n").strip()
    if "# Syntaxes" not in text or "# Themes" not in text:
        raise RuntimeError("two-face embedded asset acknowledgements are incomplete")
    return text


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    metadata = cargo_metadata()
    package_by_id = {package["id"]: package for package in metadata["packages"]}
    dependency_ids = normal_dependency_ids(metadata)

    rows = []
    errors = []
    for package_id in sorted(
        dependency_ids,
        key=lambda package_id: (
            package_by_id[package_id]["name"].lower(),
            package_by_id[package_id]["version"],
        ),
    ):
        package = package_by_id[package_id]
        files = candidate_license_files(package)
        license_expression = (package.get("license") or "").strip()
        if not license_expression and not files:
            errors.append(
                f'{package["name"]} {package["version"]}: no license expression or license file'
            )
        rows.append((package, files))

    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 2

    asset_notices = embedded_asset_notices(
        {package["name"] for package, _files in rows}
    )

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)

    parts = [
        "codex-tui THIRD-PARTY NOTICES",
        "==============================",
        "",
        "Generated from Cargo.lock / cargo metadata for the normal dependency closure.",
        "Build-only and dev-only dependencies are not represented as shipped runtime dependencies.",
        "",
    ]

    for package, files in rows:
        parts.extend(
            [
                f'{package["name"]} {package["version"]}',
                "-" * (len(package["name"]) + len(package["version"]) + 1),
                f'License: {package.get("license") or "<not-declared>"}',
                f'Repository: {package.get("repository") or "<not-declared>"}',
                f'Homepage: {package.get("homepage") or "<not-declared>"}',
            ]
        )
        if files:
            for path in files:
                parts.extend(
                    [
                        f"License file: {path.name}",
                        "",
                        read_license(path).rstrip(),
                        "",
                    ]
                )
        else:
            parts.extend(
                [
                    "License file: <not present in package source; SPDX/license expression retained above>",
                    "",
                ]
            )
        parts.append("")

    if asset_notices:
        parts.extend(
            [
                "Embedded syntax/theme asset acknowledgements",
                "-------------------------------------------",
                "",
                asset_notices,
                "",
            ]
        )

    write_text_lf(output, "\n".join(parts).rstrip() + "\n")
    print(f"WROTE {output} ({len(rows)} runtime dependency packages)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
