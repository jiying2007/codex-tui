#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import sys
from typing import Callable, Optional

from configure_main_protection import (
    API_VERSION,
    REQUIRED_CHECKS,
    validate_applied_protection,
)


SCHEMA = "codex-tui/main-protection-state/v1"


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


def validate_main_protection(payload: dict) -> None:
    if not isinstance(payload, dict):
        raise SystemExit("GitHub main branch protection status must be a JSON object")
    validate_applied_protection(payload)


def require_main_protection(
    root: pathlib.Path,
    github_repo: str,
    *,
    runner: Optional[Callable[..., subprocess.CompletedProcess[str]]] = None,
    snapshot_output: Optional[pathlib.Path] = None,
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
            f"repos/{github_repo}/branches/main/protection",
        ],
        cwd=root,
        check=False,
    )
    if response.returncode != 0:
        detail = (response.stderr or response.stdout or "").strip()
        suffix = f": {detail}" if detail else ""
        raise SystemExit(
            "stable publication requires the canonical main branch protection policy "
            "to be readable and verifiable with repository Administration(read)"
            + suffix
        )

    raw = response.stdout or ""
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as error:
        raise SystemExit(
            f"cannot decode GitHub main branch protection status: {error}"
        ) from error
    validate_main_protection(payload)

    snapshot_sha256 = hashlib.sha256(raw.encode("utf-8")).hexdigest()
    if snapshot_output is not None:
        snapshot_output.parent.mkdir(parents=True, exist_ok=True)
        snapshot_output.write_bytes(raw.encode("utf-8"))

    return {
        "schema": SCHEMA,
        "repository": github_repo,
        "branch": "main",
        "strictRequiredStatusChecks": True,
        "requiredChecks": list(REQUIRED_CHECKS),
        "enforceAdmins": True,
        "allowForcePushes": False,
        "allowDeletions": False,
        "apiVersion": API_VERSION,
        "settingsSnapshotSha256": snapshot_sha256,
        "authority": "github-rest-main-branch-protection-readback",
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Fail closed unless main retains the canonical protection policy."
    )
    parser.add_argument("--repo", required=True)
    parser.add_argument("--output", type=pathlib.Path)
    parser.add_argument("--snapshot-output", type=pathlib.Path)
    args = parser.parse_args()
    receipt = require_main_protection(
        pathlib.Path.cwd(),
        args.repo,
        snapshot_output=args.snapshot_output,
    )
    encoded = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    print(encoded, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
