#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
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
    "security-gate",
)
GITHUB_ACTIONS_SLUG = "github-actions"
API_VERSION = "2026-03-10"
PRESERVED_FLAGS = (
    "required_linear_history",
    "block_creations",
    "required_conversation_resolution",
    "lock_branch",
    "allow_fork_syncing",
)


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


def _object(value: object, label: str, allowed: set[str]) -> dict:
    if not isinstance(value, dict):
        raise SystemExit(f"{label} must be a JSON object")
    unknown = set(value) - allowed
    if unknown:
        raise SystemExit(f"unsupported {label} fields: {', '.join(sorted(unknown))}")
    return value


def _boolean(value: object, label: str) -> bool:
    if not isinstance(value, bool):
        raise SystemExit(f"{label} must be an explicit boolean")
    return value


def _enabled(value: object, label: str) -> bool:
    item = _object(value, label, {"enabled", "url"})
    return _boolean(item.get("enabled"), f"{label}.enabled")


def _actors(value: object, label: str) -> dict:
    item = _object(
        value, label,
        {"users", "teams", "apps", "url", "users_url", "teams_url", "apps_url"},
    )
    actors = {}
    for kind, identity in (("users", "login"), ("teams", "slug"), ("apps", "slug")):
        entries = item.get(kind)
        if not isinstance(entries, list):
            raise SystemExit(f"{label}.{kind} must be a complete actor list")
        names = []
        for actor in entries:
            name = actor.get(identity) if isinstance(actor, dict) else None
            if not isinstance(name, str) or not name or name.strip() != name:
                raise SystemExit(f"{label}.{kind} has an actor without {identity}")
            names.append(name)
        if len(set(names)) != len(names):
            raise SystemExit(f"{label}.{kind} has duplicate actors")
        actors[kind] = sorted(names)
    return actors


def _reviews(value: object) -> dict | None:
    if value is None:
        return None
    flags = ("dismiss_stale_reviews", "require_code_owner_reviews", "require_last_push_approval")
    item = _object(
        value, "required_pull_request_reviews",
        set(flags) | {"url", "required_approving_review_count", "dismissal_restrictions",
                      "bypass_pull_request_allowances"},
    )
    reviews = {name: _boolean(item.get(name), f"reviews.{name}") for name in flags}
    count = item.get("required_approving_review_count")
    if not isinstance(count, int) or isinstance(count, bool) or not 0 <= count <= 6:
        raise SystemExit("reviews.required_approving_review_count must be an integer from 0 to 6")
    reviews["required_approving_review_count"] = count
    for name in ("dismissal_restrictions", "bypass_pull_request_allowances"):
        if name in item:
            reviews[name] = _actors(item[name], f"reviews.{name}")
    return reviews


def _status_checks(value: object) -> dict | None:
    if value is None:
        return None
    item = _object(
        value, "required_status_checks",
        {"strict", "contexts", "checks", "url", "contexts_url", "enforcement_level"},
    )
    strict = _boolean(item.get("strict"), "required_status_checks.strict")
    contexts, checks = item.get("contexts"), item.get("checks")
    if not isinstance(contexts, list) or any(not isinstance(name, str) or not name for name in contexts):
        raise SystemExit("required_status_checks.contexts must be a complete name list")
    if not isinstance(checks, list):
        raise SystemExit("required_status_checks.checks must retain every app binding")
    normalized = {}
    for entry in checks:
        check = _object(entry, "required status check", {"context", "app_id"})
        name = check.get("context")
        if not isinstance(name, str) or not name or name.strip() != name or name in normalized:
            raise SystemExit("required status checks have an invalid or duplicate context")
        if "app_id" not in check:
            raise SystemExit(f"required status check {name} is missing app_id")
        app_id = check["app_id"]
        # GET uses null for an unbound check; PUT requires -1 to keep it unbound.
        if app_id is None:
            app_id = -1
        if not isinstance(app_id, int) or isinstance(app_id, bool) or (app_id != -1 and app_id <= 0):
            raise SystemExit(f"required status check {name} has an invalid app_id")
        normalized[name] = app_id
    if len(set(contexts)) != len(contexts) or set(contexts) != set(normalized):
        raise SystemExit("required status check contexts and app-bound checks disagree")
    return {
        "strict": strict,
        "checks": [{"context": name, "app_id": normalized[name]} for name in sorted(normalized)],
    }


def normalize_protection(response: dict) -> dict:
    """Translate a complete GET into writable policy without inventing absent controls."""
    item = _object(
        response, "main protection",
        set(PRESERVED_FLAGS) | {
            "url", "name", "enabled", "protection_url", "required_status_checks",
            "enforce_admins", "required_pull_request_reviews", "restrictions",
            "allow_force_pushes", "allow_deletions", "required_signatures",
        },
    )
    for name in ("required_status_checks", "required_pull_request_reviews", "restrictions"):
        if name not in item:
            raise SystemExit(f"main protection is missing {name}; absence must be confirmed by its API")
    policy = {
        "required_status_checks": _status_checks(item["required_status_checks"]),
        "required_pull_request_reviews": _reviews(item["required_pull_request_reviews"]),
        "restrictions": None if item["restrictions"] is None else _actors(item["restrictions"], "restrictions"),
    }
    for name in PRESERVED_FLAGS + ("enforce_admins", "allow_force_pushes", "allow_deletions", "required_signatures"):
        policy[name] = _enabled(item.get(name), name)
    return policy


def _read_protection_api(root: pathlib.Path, repo: str, suffix: str, *, allow_absent: bool = False) -> dict | None:
    response = run(
        ["gh", "api", "--method", "GET", "--include",
         "-H", "Accept: application/vnd.github+json",
         "-H", f"X-GitHub-Api-Version: {API_VERSION}",
         f"repos/{repo}/branches/main/protection{suffix}"],
        cwd=root, check=False,
    )
    headers, separator, body = response.stdout.replace("\r\n", "\n").partition("\n\n")
    status = re.match(r"HTTP/\S+\s+(\d{3})(?:\s|$)", headers)
    if not separator or status is None:
        raise SystemExit("cannot verify authoritative main protection HTTP response; stopping")
    status_code = int(status.group(1))
    try:
        payload = json.loads(body)
    except (ValueError, TypeError) as error:
        raise SystemExit("cannot decode main protection response; stopping") from error
    if not isinstance(payload, dict):
        raise SystemExit("main protection API must return a JSON object; stopping")
    if allow_absent and status_code == 404 and response.returncode != 0 and isinstance(payload.get("message"), str):
        return None
    if status_code != 200 or response.returncode != 0:
        raise SystemExit(f"cannot verify main protection (HTTP {status_code}); stopping")
    return payload


def read_protection(root: pathlib.Path, repo: str) -> dict:
    # Never infer an unprotected branch from a failed full GET, including HTTP 404.
    response = _read_protection_api(root, repo, "")
    assert response is not None
    for name in ("required_status_checks", "required_pull_request_reviews", "restrictions", "required_signatures"):
        if name not in response or response[name] is None:
            # Full GET already proved this branch's Administration(read) access.
            # An optional resource is absent only after its own explicit 404.
            resource = _read_protection_api(root, repo, "/" + name, allow_absent=True)
            response[name] = {"enabled": False} if name == "required_signatures" and resource is None else resource
    normalize_protection(response)
    return response


def protection_payload(github_actions_app_id: int, current: dict) -> dict:
    if (
        not isinstance(github_actions_app_id, int)
        or isinstance(github_actions_app_id, bool)
        or github_actions_app_id <= 0
    ):
        raise ValueError("GitHub Actions app id must be a positive integer")
    policy = normalize_protection(current)
    # Signature protection has a separate API. This helper must never write it.
    del policy["required_signatures"]
    checks = {entry["context"]: entry["app_id"] for entry in (policy["required_status_checks"] or {}).get("checks", [])}
    for name in REQUIRED_CHECKS:
        if name in checks and checks[name] not in (-1, github_actions_app_id):
            raise SystemExit(f"existing canonical check {name} is bound to another app; refusing to replace it")
        checks[name] = github_actions_app_id
    policy["required_status_checks"] = {
        "strict": True,
        "checks": [{"context": name, "app_id": checks[name]} for name in sorted(checks)],
    }
    policy.update(enforce_admins=True, allow_force_pushes=False, allow_deletions=False)
    return policy


def policy_digest(policy: dict) -> str:
    return hashlib.sha256(json.dumps(policy, sort_keys=True, separators=(",", ":")).encode("utf-8")).hexdigest()


def validate_preserved_protection(response: dict, payload: dict, previous: dict) -> None:
    actual = normalize_protection(response)
    expected = dict(payload)
    expected["required_signatures"] = normalize_protection(previous)["required_signatures"]
    if actual != expected:
        changed = sorted(name for name in expected if actual.get(name) != expected[name])
        raise SystemExit("independent protection readback differs from reviewed policy: " + ", ".join(changed))


def validate_ci_contract(ci_text: str) -> None:
    if "name: rust-1.88-msrv" not in ci_text:
        raise SystemExit("canonical CI no longer declares rust-1.88-msrv")
    expected_matrix = "os: [ubuntu-24.04, macos-latest, windows-latest]"
    if expected_matrix not in ci_text:
        raise SystemExit(
            "canonical CI platform matrix changed; review branch-protection required checks"
        )


def validate_security_contract(security_text: str) -> None:
    if re.search(r"^\s+(?:paths|paths-ignore):", security_text, re.MULTILINE):
        raise SystemExit("required security workflow must not skip paths")
    expected = (
        "  push:\n    branches: [main]\n  pull_request:",
        "  security-gate:\n    name: security-gate\n    if: ${{ always() }}",
        "    needs: [cargo-deny, rustsec]",
    )
    if any(value not in security_text for value in expected):
        raise SystemExit("required security-gate must always summarize both security jobs")


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

    invalid_ids = [
        name
        for name, value in selected.items()
        if (
            not isinstance(value, int)
            or isinstance(value, bool)
            or value <= 0
        )
    ]
    if invalid_ids:
        raise SystemExit(
            "canonical checks have invalid GitHub Actions app ids: "
            + ", ".join(invalid_ids)
        )

    app_ids = set(selected.values())
    if len(app_ids) != 1:
        raise SystemExit(
            f"canonical checks must come from one GitHub Actions app; got {sorted(app_ids)}"
        )
    app_id = next(iter(app_ids))
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
            "Preview or apply the codex-tui minimum main protection while retaining existing controls. "
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
    security_text = (root / ".github/workflows/security.yml").read_text(encoding="utf-8")
    validate_security_contract(security_text)

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

    previous = read_protection(root, repo)
    observed_policy = normalize_protection(previous)
    payload = protection_payload(github_actions_app_id, previous)
    plan = {
        "schema": "codex-tui/main-protection-plan/v2",
        "repository": repo,
        "branch": "main",
        "sourceSha": commit_sha,
        "currentlyProtected": branch_meta.get("protected") is True,
        "requiredChecks": list(REQUIRED_CHECKS),
        "githubActionsAppIdObserved": github_actions_app_id,
        "observedPolicySha256": policy_digest(observed_policy),
        "signatureProtectionObserved": observed_policy["required_signatures"],
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

    current = read_protection(root, repo)
    if normalize_protection(current) != observed_policy:
        raise SystemExit("main protection changed after planning; no settings written; review a fresh plan")
    branch_before = json.loads(
        run(["gh", "api", f"repos/{repo}/branches/main"], cwd=root).stdout
    )
    if str((branch_before.get("commit") or {}).get("sha", "")).lower() != commit_sha:
        raise SystemExit("GitHub main moved after planning; no settings written; review a fresh plan")
    if branch_before.get("protected") is not True:
        raise SystemExit("GitHub main protection changed after planning; no settings written")

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
    readback = read_protection(root, repo)
    validate_applied_protection(readback, github_actions_app_id)
    validate_preserved_protection(readback, payload, previous)

    branch_after = json.loads(
        run(["gh", "api", f"repos/{repo}/branches/main"], cwd=root).stdout
    )
    if branch_after.get("protected") is not True:
        raise SystemExit("GitHub main branch still reports protected=false after apply")
    if str((branch_after.get("commit") or {}).get("sha", "")).lower() != commit_sha:
        raise SystemExit("GitHub main moved during protection configuration; review the current source and policy")

    plan["currentlyProtected"] = True
    plan["applied"] = True
    plan["verifiedPolicySha256"] = policy_digest(normalize_protection(readback))
    plan["existingProtectionPreserved"] = True
    plan["next"] = (
        "main protection independently read back with existing controls retained; "
        "stable publication still requires its own current-source governance checks"
    )
    print(json.dumps(plan, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
