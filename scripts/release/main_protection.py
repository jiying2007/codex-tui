#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
from typing import Callable, Optional

from configure_main_protection import (
    API_VERSION,
    REQUIRED_CHECKS,
    validate_applied_protection,
    validate_check_runs,
)


SCHEMA = "codex-tui/main-protection-state/v2"
HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")


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


def sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def load_check_runs(path: pathlib.Path, source_sha: str) -> tuple[int, str]:
    try:
        raw = path.read_bytes()
    except OSError as error:
        raise SystemExit(f"cannot read exact-main check-runs snapshot: {error}") from error
    try:
        payload = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot decode exact-main check-runs snapshot: {error}") from error
    if not isinstance(payload, dict):
        raise SystemExit("exact-main check-runs snapshot must be a JSON object")
    app_id = validate_check_runs(payload, source_sha)
    return app_id, sha256_bytes(raw)


def validate_main_protection(payload: dict, github_actions_app_id: int) -> None:
    if not isinstance(payload, dict):
        raise SystemExit("GitHub main branch protection status must be a JSON object")
    validate_applied_protection(payload, github_actions_app_id)


def require_main_protection(
    root: pathlib.Path,
    github_repo: str,
    *,
    source_sha: str,
    check_runs_path: pathlib.Path,
    runner: Optional[Callable[..., subprocess.CompletedProcess[str]]] = None,
    snapshot_output: Optional[pathlib.Path] = None,
) -> dict:
    source_sha = source_sha.strip().lower()
    if not HEX40.fullmatch(source_sha):
        raise SystemExit("--source-sha must be exactly 40 hexadecimal characters")

    github_actions_app_id, check_runs_sha256 = load_check_runs(
        check_runs_path,
        source_sha,
    )

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

    raw = (response.stdout or "").encode("utf-8")
    try:
        payload = json.loads(raw.decode("utf-8"))
    except json.JSONDecodeError as error:
        raise SystemExit(
            f"cannot decode GitHub main branch protection status: {error}"
        ) from error
    validate_main_protection(payload, github_actions_app_id)

    if snapshot_output is not None:
        snapshot_output.parent.mkdir(parents=True, exist_ok=True)
        snapshot_output.write_bytes(raw)

    return {
        "schema": SCHEMA,
        "repository": github_repo,
        "branch": "main",
        "sourceSha": source_sha,
        "strictRequiredStatusChecks": True,
        "requiredChecks": list(REQUIRED_CHECKS),
        "githubActionsAppId": github_actions_app_id,
        "requiredChecksAppBound": True,
        "enforceAdmins": True,
        "allowForcePushes": False,
        "allowDeletions": False,
        "apiVersion": API_VERSION,
        "settingsSnapshotSha256": sha256_bytes(raw),
        "checkRunsSnapshotSha256": check_runs_sha256,
        "authority": "github-rest-main-protection-and-exact-main-check-runs-readback",
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Fail closed unless main retains the canonical protection policy and "
            "required checks are pinned to the GitHub Actions app observed on the "
            "exact source SHA."
        )
    )
    parser.add_argument("--repo", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--check-runs-json", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path)
    parser.add_argument("--snapshot-output", type=pathlib.Path)
    args = parser.parse_args()
    receipt = require_main_protection(
        pathlib.Path.cwd(),
        args.repo,
        source_sha=args.source_sha,
        check_runs_path=args.check_runs_json,
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
