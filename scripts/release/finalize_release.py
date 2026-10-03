#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime as dt
import json
import pathlib
import re
import subprocess
import sys
import tempfile

from _compat import cargo_package, write_text_lf

STABLE_VERSION = re.compile(
    r"^[1-9][0-9]*\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)$"
)
RELEASE_DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


def run(command: list[str], *, cwd: pathlib.Path, check: bool = True) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(
        command,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if check and proc.returncode != 0:
        if proc.stdout:
            sys.stderr.write(proc.stdout)
        if proc.stderr:
            sys.stderr.write(proc.stderr)
        raise SystemExit(f"command failed ({proc.returncode}): {' '.join(command)}")
    return proc


def finalize_changelog_text(text: str, version: str, release_date: str) -> str:
    if not STABLE_VERSION.fullmatch(version):
        raise SystemExit(f"stable release version required; got {version!r}")
    if not RELEASE_DATE.fullmatch(release_date):
        raise SystemExit("release date must use YYYY-MM-DD")
    try:
        dt.date.fromisoformat(release_date)
    except ValueError as error:
        raise SystemExit(f"invalid release date: {release_date}") from error

    unreleased = f"## [{version}] - Unreleased"
    released_prefix = f"## [{version}] - "
    matches = [line for line in text.splitlines() if line.startswith(released_prefix)]
    if matches != [unreleased]:
        raise SystemExit(
            f"CHANGELOG must contain exactly one unreleased heading for {version}; "
            f"found {matches!r}"
        )
    return text.replace(unreleased, f"## [{version}] - {release_date}", 1)


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Prepare the final stable CHANGELOG date for the current Cargo version. "
            "Default mode is dry-run; --write is required to modify CHANGELOG.md."
        )
    )
    parser.add_argument("--changelog", default="CHANGELOG.md")
    parser.add_argument(
        "--date",
        default="",
        help="release date YYYY-MM-DD; default is current UTC date",
    )
    parser.add_argument(
        "--write",
        action="store_true",
        help="write the validated final heading to CHANGELOG.md",
    )
    args = parser.parse_args()

    root = pathlib.Path.cwd().resolve()
    dirty = run(["git", "status", "--porcelain"], cwd=root).stdout.strip()
    if dirty:
        raise SystemExit(
            "release finalization requires a clean worktree before editing CHANGELOG"
        )

    branch = run(["git", "branch", "--show-current"], cwd=root).stdout.strip()
    if branch != "main":
        raise SystemExit(f"release finalization must run from main; got {branch!r}")

    commit_sha = run(["git", "rev-parse", "HEAD"], cwd=root).stdout.strip().lower()
    remote = run(["git", "ls-remote", "origin", "refs/heads/main"], cwd=root).stdout.strip()
    remote_sha = remote.split()[0].lower() if remote else ""
    if remote_sha != commit_sha:
        raise SystemExit(
            "origin/main must equal local main before finalization: "
            f"local={commit_sha} remote={remote_sha or '<missing>'}"
        )

    version = str(cargo_package(root)["version"])
    if not STABLE_VERSION.fullmatch(version):
        raise SystemExit(f"stable release version required; got {version!r}")

    tag = f"v{version}"
    tag_check = run(
        ["git", "ls-remote", "--exit-code", "--tags", "origin", f"refs/tags/{tag}"],
        cwd=root,
        check=False,
    )
    if tag_check.returncode == 0:
        raise SystemExit(f"stable tag {tag} already exists")
    if tag_check.returncode not in (2,):
        raise SystemExit(f"unable to determine whether stable tag {tag} exists")

    release_date = args.date.strip() or dt.datetime.now(dt.timezone.utc).date().isoformat()
    changelog = (root / args.changelog).resolve()
    original = changelog.read_text(encoding="utf-8")
    proposed = finalize_changelog_text(original, version, release_date)

    with tempfile.TemporaryDirectory(prefix="codex-tui-release-finalize-") as temp:
        candidate = pathlib.Path(temp) / "CHANGELOG.md"
        write_text_lf(candidate, proposed)
        notes = pathlib.Path(temp) / "RELEASE_NOTES.md"
        run(
            [
                sys.executable,
                "scripts/release/extract_changelog.py",
                "--version",
                version,
                "--changelog",
                str(candidate),
                "--require-released",
                "--output",
                str(notes),
            ],
            cwd=root,
        )

    if args.write:
        write_text_lf(changelog, proposed)

    summary = {
        "schema": "codex-tui/release-finalization-preflight/v1",
        "version": version,
        "tag": tag,
        "sourceShaBeforeFinalization": commit_sha,
        "releaseDate": release_date,
        "changelog": str(changelog),
        "releasedHeading": f"## [{version}] - {release_date}",
        "writeApplied": args.write,
        "next": (
            "review CHANGELOG diff, commit/push the release-notes change, then wait for "
            "fresh exact-SHA CI/development/performance/release-preview evidence"
            if args.write
            else "rerun with --write when ready to create the final release-notes change"
        ),
    }
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
