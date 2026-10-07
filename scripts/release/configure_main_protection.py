#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys

from _compat import cargo_package

REQUIRED_CHECKS = (
    "rust-1.88-msrv",
    "ubuntu-24.04",
    "macos-latest",
    "windows-latest",
)
GITHUB_ACTIONS_SLUG = "github-actions"
API_VERSION = "2026-03-10"


def run(
    command: list[str],
    *,
    cwd: pathlib.Path,
    input_text: str | None = None,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(
        command,
        cwd=cwd,
        input=input_text,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
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


def github_repo_from_cargo(root: pathlib.Path) -> str:
    repository = cargo_package(root).get("repository") or ""
    match = re.fullmatch(r"https://github\.com/([^/]+/[^/]+?)(?:\.git)?/?", repository)
    if not match:
        raise SystemExit(f"unsupported GitHub repository URL: {repository}")
    return match.group(1)


def protection_payload(github_actions_app_id: int) -> dict:
    if (
        not isinstance(github_actions_app_id, int)
        or isinstance(github_actions_app_id, bool)
        or github_actions_app_id <= 0
    ):
        raise ValueError("GitHub Actions app id must be a positive integer")
    return {
        "required_status_checks": {
            "strict": True,
            "checks": [
                {"context": name, "app_id": github_actions_app_id}
                for name in REQUIRED_CHECKS
            ],
        },
        "enforce_admins": True,
        "required_pull_request_reviews": None,
        "restrictions": None,
        "required_linear_history": False,
        "allow_force_pushes": False,
        "allow_deletions": False,
        "block_creations": False,
        "required_conversation_resolution": False,
        "lock_branch": False,
        "allow_fork_syncing": False,
    }


def validate_ci_contract(ci_text: str) -> None:
    if "name: rust-1.88-msrv" not in ci_text:
        raise SystemExit("canonical CI no longer declares rust-1.88-msrv")
    expected_matrix = "os: [ubuntu-24.04, macos-latest, windows-latest]"
    if expected_matrix not in ci_text:
        raise SystemExit(
            "canonical CI platform matrix changed; review branch-protection required checks"
        )


def validate_check_runs(payload: dict, commit_sha: str) -> int:
    commit_sha = commit_sha.strip().lower()
    if not re.fullmatch(r"[0-9a-f]{40}", commit_sha):
        raise SystemExit("canonical check source SHA must be exactly 40 hexadecimal characters")
    runs = payload.get("check_runs")
    if not isinstance(runs, list):
        raise SystemExit("GitHub check-runs response is missing check_runs")

    selected = {}
    for item in runs:
        if not isinstance(item, dict):
            continue
        name = item.get("name")
        if name not in REQUIRED_CHECKS:
            continue
        head_sha = str(item.get("head_sha", "")).strip().lower()
        if head_sha != commit_sha:
            raise SystemExit(
                f"required check {name} is not bound to current main: "
                f"expected={commit_sha} actual={head_sha or '<missing>'}"
            )
        app = item.get("app") or {}
        if app.get("slug") != GITHUB_ACTIONS_SLUG:
            raise SystemExit(
                f"required check {name} is not produced by GitHub Actions"
            )
        if item.get("status") != "completed" or item.get("conclusion") != "success":
            raise SystemExit(
                f"required check {name} is not successful on current main"
            )
        selected[name] = app.get("id")

    missing = [name for name in REQUIRED_CHECKS if name not in selected]
    if missing:
        raise SystemExit(
            "current main is missing successful canonical checks: " + ", ".join(missing)
        )

    app_ids = {
        value
        for value in selected.values()
        if isinstance(value, int) and not isinstance(value, bool)
    }
    if len(app_ids) != 1:
        raise SystemExit(
            f"canonical checks must come from one GitHub Actions app; got {sorted(app_ids)}"
        )
    app_id = next(iter(app_ids))
    if app_id <= 0:
        raise SystemExit("GitHub Actions app id must be positive")
    return app_id


def validate_applied_protection(
    response: dict,
    github_actions_app_id: int | None = None,
) -> None:
    status = response.get("required_status_checks") or {}
    if status.get("strict") is not True:
        raise SystemExit("applied protection does not require strict status checks")

    if github_actions_app_id is None:
        contexts = set(status.get("contexts") or [])
        missing = [name for name in REQUIRED_CHECKS if name not in contexts]
        if missing:
            raise SystemExit(
                "applied protection is missing required checks: " + ", ".join(missing)
            )
    else:
        if (
            not isinstance(github_actions_app_id, int)
            or isinstance(github_actions_app_id, bool)
            or github_actions_app_id <= 0
        ):
            raise SystemExit("expected GitHub Actions app id must be positive")
        checks = status.get("checks")
        if not isinstance(checks, list):
            raise SystemExit("applied protection is missing app-bound required checks")
        observed = {}
        for item in checks:
            if not isinstance(item, dict):
                continue
            context = item.get("context")
            if context in REQUIRED_CHECKS:
                if context in observed:
                    raise SystemExit(
                        f"applied protection has duplicate required check: {context}"
                    )
                observed[context] = item.get("app_id")
        missing_bound = [name for name in REQUIRED_CHECKS if name not in observed]
        if missing_bound:
            raise SystemExit(
                "applied protection is missing app-bound checks: "
                + ", ".join(missing_bound)
            )
        wrong_app = [
            name
            for name in REQUIRED_CHECKS
            if observed.get(name) != github_actions_app_id
        ]
        if wrong_app:
            raise SystemExit(
                "applied protection required checks are not pinned to the "
                "GitHub Actions app: " + ", ".join(wrong_app)
            )

    enforce_admins = response.get("enforce_admins") or {}
    if enforce_admins.get("enabled") is not True:
        raise SystemExit("applied protection does not include administrators")

    force = response.get("allow_force_pushes") or {}
    if force.get("enabled") is not False:
        raise SystemExit("applied protection still allows force pushes")

    deletions = response.get("allow_deletions") or {}
    if deletions.get("enabled") is not False:
        raise SystemExit("applied protection still allows branch deletion")


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Preview or apply the codex-tui personal-first main branch protection policy. "
            "Default mode is non-mutating."
        )
    )
    parser.add_argument(
        "--apply",
        action="store_true",
        help="apply the reviewed protection policy through GitHub REST",
    )
    parser.add_argument(
        "--confirm-repository",
        default="",
        help="required with --apply; must exactly match owner/repo",
    )
    parser.add_argument("--github-repo", default="")
    args = parser.parse_args()

    root = pathlib.Path.cwd().resolve()
    dirty = run(["git", "status", "--porcelain"], cwd=root).stdout.strip()
    if dirty:
        raise SystemExit("branch-protection configuration requires a clean worktree")

    branch = run(["git", "branch", "--show-current"], cwd=root).stdout.strip()
    if branch != "main":
        raise SystemExit(f"branch-protection configuration must run from main; got {branch!r}")

    commit_sha = run(["git", "rev-parse", "HEAD"], cwd=root).stdout.strip().lower()
    repo = args.github_repo.strip() or github_repo_from_cargo(root)

    branch_proc = run(
        ["gh", "api", f"repos/{repo}/branches/main"],
        cwd=root,
    )
    branch_meta = json.loads(branch_proc.stdout)
    remote_sha = str((branch_meta.get("commit") or {}).get("sha", "")).lower()
    if remote_sha != commit_sha:
        raise SystemExit(
            "local main must equal GitHub main before protection configuration: "
            f"local={commit_sha} github={remote_sha or '<missing>'}"
        )

    ci_text = (root / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    validate_ci_contract(ci_text)

    checks_proc = run(
        [
            "gh",
            "api",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            f"X-GitHub-Api-Version: {API_VERSION}",
            f"repos/{repo}/commits/{commit_sha}/check-runs?filter=latest&per_page=100",
        ],
        cwd=root,
    )
    github_actions_app_id = validate_check_runs(
        json.loads(checks_proc.stdout),
        commit_sha,
    )

    payload = protection_payload(github_actions_app_id)
    plan = {
        "schema": "codex-tui/main-protection-plan/v1",
        "repository": repo,
        "branch": "main",
        "sourceSha": commit_sha,
        "currentlyProtected": branch_meta.get("protected") is True,
        "requiredChecks": list(REQUIRED_CHECKS),
        "githubActionsAppIdObserved": github_actions_app_id,
        "policy": payload,
        "applyRequested": args.apply,
    }

    if not args.apply:
        plan["next"] = (
            f"review this policy, then rerun with --apply --confirm-repository {repo}"
        )
        print(json.dumps(plan, indent=2, sort_keys=True))
        return 0

    if args.confirm_repository.strip() != repo:
        raise SystemExit(
            "--apply requires --confirm-repository exactly matching "
            f"{repo!r}"
        )

    applied = run(
        [
            "gh",
            "api",
            "--method",
            "PUT",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            f"X-GitHub-Api-Version: {API_VERSION}",
            f"repos/{repo}/branches/main/protection",
            "--input",
            "-",
        ],
        cwd=root,
        input_text=json.dumps(payload),
    )
    response = json.loads(applied.stdout)
    validate_applied_protection(response, github_actions_app_id)

    branch_after = json.loads(
        run(["gh", "api", f"repos/{repo}/branches/main"], cwd=root).stdout
    )
    if branch_after.get("protected") is not True:
        raise SystemExit("GitHub main branch still reports protected=false after apply")

    plan["currentlyProtected"] = True
    plan["applied"] = True
    plan["next"] = (
        "main protection applied and verified; stable publication branch-state gate "
        "can now observe main.protected=true"
    )
    print(json.dumps(plan, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
