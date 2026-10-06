#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import pathlib
import subprocess
import sys
from typing import Callable, Optional

API_VERSION = "2026-03-10"


def _run(
    command: list[str],
    *,
    cwd: pathlib.Path,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(
        command,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=os.environ.copy(),
    )
    if check and proc.returncode != 0:
        if proc.stdout:
            sys.stderr.write(proc.stdout)
        if proc.stderr:
            sys.stderr.write(proc.stderr)
        raise SystemExit(
            f"command failed ({proc.returncode}): {' '.join(command)}"
        )
    return proc


def require_immutable_releases(
    root: pathlib.Path,
    github_repo: str,
    *,
    runner: Optional[Callable[..., subprocess.CompletedProcess[str]]] = None,
) -> dict:
    runner = runner or _run
    response = runner(
        [
            "gh",
            "api",
            "--method",
            "GET",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            f"X-GitHub-Api-Version: {API_VERSION}",
            f"repos/{github_repo}/immutable-releases",
        ],
        cwd=root,
        check=False,
    )
    if response.returncode != 0:
        detail = (response.stderr or response.stdout or "").strip()
        suffix = f": {detail}" if detail else ""
        raise SystemExit(
            "stable publication requires GitHub immutable releases to be enabled "
            "and verifiable for this repository; enable release immutability with "
            "an administrator-read credential before publishing" + suffix
        )
    try:
        payload = json.loads(response.stdout or "")
    except json.JSONDecodeError as error:
        raise SystemExit(
            f"cannot decode GitHub immutable-releases status: {error}"
        ) from error
    if not isinstance(payload, dict) or payload.get("enabled") is not True:
        raise SystemExit(
            "stable publication requires GitHub immutable releases enabled=true"
        )
    return {
        "schema": "codex-tui/immutable-releases/v1",
        "repository": github_repo,
        "enabled": True,
        "enforcedByOwner": payload.get("enforced_by_owner") is True,
        "apiVersion": API_VERSION,
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Fail closed unless GitHub immutable releases are enabled."
    )
    parser.add_argument("--repo", required=True)
    parser.add_argument("--output", type=pathlib.Path)
    args = parser.parse_args()
    receipt = require_immutable_releases(pathlib.Path.cwd(), args.repo)
    encoded = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    print(encoded, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
