"""Native archive identity. Self-reported hashes do not authenticate an untrusted binary."""
from __future__ import annotations

import hashlib
from pathlib import Path
import re
import sys

RELEASE_ARTIFACT_SCHEMA = "codex-tui/release-artifact/v2"


def native_platform() -> str:
    names = {"linux": "linux", "darwin": "macos", "win32": "windows"}
    if sys.platform not in names:
        raise SystemExit("unsupported archive smoke host: " + sys.platform)
    return names[sys.platform]


def _require_text(value, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise SystemExit(f"archive build toolchain {label} must be non-empty")
    return value.strip()


def _validate_tool(toolchain: dict, name: str, triple: str) -> None:
    value = toolchain.get(name)
    if not isinstance(value, dict):
        raise SystemExit(f"archive buildToolchain.{name} must be an object")

    version = _require_text(value.get("version"), f"{name}.version")
    release = _require_text(value.get("release"), f"{name}.release")
    commit_hash = _require_text(value.get("commitHash"), f"{name}.commitHash")
    commit_date = _require_text(value.get("commitDate"), f"{name}.commitDate")
    host = _require_text(value.get("host"), f"{name}.host")

    if not version.startswith(name + " "):
        raise SystemExit(f"archive buildToolchain.{name}.version has unexpected format")
    if not re.fullmatch(r"[0-9A-Za-z.+_-]+", release):
        raise SystemExit(f"archive buildToolchain.{name}.release has unexpected format")
    if not re.fullmatch(r"[0-9a-f]{40}", commit_hash):
        raise SystemExit(f"archive buildToolchain.{name}.commitHash must be a full lowercase SHA")
    if not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", commit_date):
        raise SystemExit(f"archive buildToolchain.{name}.commitDate must be YYYY-MM-DD")
    if host != triple:
        raise SystemExit(
            f"archive buildToolchain.{name}.host does not match hostTriple"
        )


def validate_metadata(metadata: dict, binary: Path, version: str, tag: str, commit: str) -> None:
    if not isinstance(metadata, dict):
        raise SystemExit("archive metadata must be an object")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise SystemExit("archive expected commit must be a full lowercase SHA")
    platform = native_platform()
    expected = {"schema": RELEASE_ARTIFACT_SCHEMA, "version": version, "tag": tag,
                "commitSha": commit, "license": "Apache-2.0", "platform": platform,
                "binary": "codex-tui.exe" if platform == "windows" else "codex-tui",
                "binarySha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
    for key, value in expected.items():
        if metadata.get(key) != value:
            raise SystemExit("metadata %s mismatch: %r != %r" % (key, metadata.get(key), value))
    triple = metadata.get("hostTriple")
    valid_triple = isinstance(triple, str) and bool(re.fullmatch(r"[a-zA-Z0-9_.-]+", triple))
    if valid_triple:
        valid_triple = {"linux": triple.endswith("-unknown-linux-gnu"),
                        "macos": triple.endswith("-apple-darwin"),
                        "windows": triple.endswith("-pc-windows-msvc")}[platform]
    if not valid_triple:
        raise SystemExit("archive hostTriple does not match the native platform")

    toolchain = metadata.get("buildToolchain")
    if not isinstance(toolchain, dict):
        raise SystemExit("archive buildToolchain must be an object")
    _validate_tool(toolchain, "rustc", triple)
    _validate_tool(toolchain, "cargo", triple)


def validate_compiled_source(report: dict, commit: str) -> None:
    if (not isinstance(report, dict) or report.get("schema") != "codex-tui/performance/v2"
            or report.get("sourceSha") != commit):
        raise SystemExit("archive compiled source identity differs from expected commit")


def validate_members(members) -> None:
    """Fail before extraction on aliases, cross-platform collisions or oversized content."""
    if not 1 <= len(members) <= 4096:
        raise SystemExit("archive member count is outside 1..4096")
    seen, files, roots, total = set(), set(), set(), 0
    for name, is_directory, is_regular, size in members:
        path = name[:-1] if is_directory and name.endswith("/") else name
        parts = path.split("/")
        if (not path or "\\" in path or ":" in path
                or any(part in ("", ".", "..") or part.endswith((".", " ")) for part in parts)
                or (not is_directory and len(parts) < 2)):
            raise SystemExit("unsafe archive member path: " + name)
        key = path.casefold()
        if key in seen or not (is_directory or is_regular):
            raise SystemExit("duplicate or unsupported archive member: " + name)
        seen.add(key)
        roots.add(parts[0].casefold())
        if not is_directory:
            files.add(key)
        total += size
        if size < 0 or total > 256 * 1024 * 1024:
            raise SystemExit("archive exceeds 256 MiB extracted size bound")
    if len(roots) != 1:
        raise SystemExit("archive must contain exactly one top-level directory")
    for key in seen:
        parts = key.split("/")
        if any("/".join(parts[:index]) in files for index in range(1, len(parts))):
            raise SystemExit("archive file is also used as a directory: " + key)
