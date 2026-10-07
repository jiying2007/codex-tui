#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile

from _compat import cargo_package, write_text_lf

HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
STABLE_VERSION = re.compile(
    r"^[1-9][0-9]*\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)$"
)
RELEASE_EVIDENCE_SCHEMA = "codex-tui/release-evidence/v6"
REAL_EVIDENCE_BUNDLE_SCHEMA = "codex-tui/stable-real-evidence-bundle/v1"


def run(
    command: list[str],
    *,
    cwd: pathlib.Path,
    capture: bool = True,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(
        command,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
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


def github_repo_from_cargo(root: pathlib.Path) -> str:
    repository = cargo_package(root).get("repository") or ""
    match = re.fullmatch(r"https://github\.com/([^/]+/[^/]+?)(?:\.git)?/?", repository)
    if not match:
        raise SystemExit(f"unsupported GitHub repository URL: {repository}")
    return match.group(1)



from immutable_releases import require_immutable_releases as _require_immutable_releases


def require_immutable_releases(root: pathlib.Path, github_repo: str) -> dict:
    return _require_immutable_releases(root, github_repo, runner=run)

def load_json(path: pathlib.Path, label: str) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise SystemExit(f"{label} must be a JSON object")
    return value


def nonempty(value, label: str) -> str:
    text = str(value).strip()
    if not text:
        raise SystemExit(f"{label} must not be empty")
    return text


def stable_publish_inputs(
    evidence: dict,
    stable_qualification_run: int,
    real_evidence_payload: str,
) -> dict[str, str]:
    if evidence.get("schema") != RELEASE_EVIDENCE_SCHEMA:
        raise SystemExit(
            "prior stable evidence schema mismatch: "
            f"{evidence.get('schema')!r}"
        )
    if stable_qualification_run <= 0:
        raise SystemExit("stable qualification run must be nonzero")
    if not real_evidence_payload.strip():
        raise SystemExit("stable real evidence bundle payload must not be empty")

    canonical_ci_run = nonempty(
        evidence.get("canonicalCiRun"),
        "canonical CI run",
    )
    bundle = evidence.get("realEvidenceBundle")
    if not isinstance(bundle, dict):
        raise SystemExit("prior stable evidence realEvidenceBundle is missing")
    if bundle.get("schema") != REAL_EVIDENCE_BUNDLE_SCHEMA:
        raise SystemExit("prior stable evidence bundle schema mismatch")

    inputs = {
        "channel": "stable",
        "publish": "true",
        "canonical_ci_run": canonical_ci_run,
        "stable_qualification_run": str(stable_qualification_run),
        "stable_real_evidence_bundle": real_evidence_payload.strip(),
    }
    if len(inputs) > 25:
        raise SystemExit(
            f"stable publication requires {len(inputs)} workflow inputs; "
            "GitHub maximum is 25"
        )
    return inputs


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Prepare or dispatch stable publication from a successful prior "
            "stable publish=false release run without manually re-entering evidence."
        )
    )
    parser.add_argument("--stable-qualification-run", required=True, type=int)
    parser.add_argument("--github-repo", default="")
    parser.add_argument(
        "--real-evidence-bundle",
        default="release/evidence/linux/stable-real-evidence.bundle",
        help="source-bound raw real-evidence bundle created by linux_qualify.py",
    )
    parser.add_argument(
        "--output",
        default="release/evidence/linux/stable-publication-summary.json",
    )
    parser.add_argument(
        "--dispatch",
        action="store_true",
        help="dispatch stable publish=true after all local and retained-evidence checks pass",
    )
    args = parser.parse_args()

    if args.stable_qualification_run <= 0:
        raise SystemExit("--stable-qualification-run must be nonzero")

    root = pathlib.Path.cwd().resolve()
    dirty = run(["git", "status", "--porcelain"], cwd=root).stdout.strip()
    if dirty:
        raise SystemExit(
            "stable publication preparation requires a clean worktree; "
            "commit/stash local changes first"
        )

    commit_sha = run(["git", "rev-parse", "HEAD"], cwd=root).stdout.strip().lower()
    if not HEX40.fullmatch(commit_sha):
        raise SystemExit(f"unexpected git SHA: {commit_sha!r}")

    branch = run(["git", "branch", "--show-current"], cwd=root).stdout.strip()
    if branch != "main":
        raise SystemExit(f"stable publication must run from main; got {branch!r}")

    package = cargo_package(root)
    version = nonempty(package.get("version"), "Cargo package version")
    if not STABLE_VERSION.fullmatch(version):
        raise SystemExit(
            "stable publication requires a release version X.Y.Z with major >= 1; "
            f"got {version}"
        )
    tag = f"v{version}"
    github_repo = args.github_repo.strip() or github_repo_from_cargo(root)

    remote_main = run(
        ["git", "ls-remote", "origin", "refs/heads/main"],
        cwd=root,
    ).stdout.strip()
    remote_sha = remote_main.split()[0].lower() if remote_main else ""
    if remote_sha != commit_sha:
        raise SystemExit(
            "refusing stable publication because origin/main drifted: "
            f"local={commit_sha} remote={remote_sha or '<missing>'}"
        )

    existing_tag = run(
        ["git", "ls-remote", "--exit-code", "--tags", "origin", f"refs/tags/{tag}"],
        cwd=root,
        check=False,
    )
    if existing_tag.returncode == 0:
        raise SystemExit(f"refusing stable publication because tag {tag} already exists")
    if existing_tag.returncode not in (2,):
        raise SystemExit(
            f"failed to check existing tag {tag}; git ls-remote returned "
            f"{existing_tag.returncode}"
        )

    immutable_releases = require_immutable_releases(root, github_repo)

    with tempfile.TemporaryDirectory(prefix="codex-tui-stable-publish-") as temp:
        temp_dir = pathlib.Path(temp)
        branch_json = temp_dir / "main-branch.json"
        branch_meta = run(
            ["gh", "api", f"repos/{github_repo}/branches/main"],
            cwd=root,
        )
        write_text_lf(branch_json, branch_meta.stdout)
        run(
            [
                sys.executable,
                "scripts/release/validate_release_branch_state.py",
                "--branch-json",
                str(branch_json),
                "--commit",
                commit_sha,
                "--require-protected",
            ],
            cwd=root,
            capture=False,
        )

        run_json = temp_dir / "prior-stable-run.json"
        prior_run = run(
            [
                "gh",
                "api",
                f"repos/{github_repo}/actions/runs/{args.stable_qualification_run}",
            ],
            cwd=root,
        )
        write_text_lf(run_json, prior_run.stdout)

        gate_dir = temp_dir / "release-gate"
        run(
            [
                "gh",
                "run",
                "download",
                str(args.stable_qualification_run),
                "--repo",
                github_repo,
                "--name",
                "release-gate",
                "--dir",
                str(gate_dir),
            ],
            cwd=root,
            capture=False,
        )

        verification_path = gate_dir / "release-verification.json"
        evidence_path = gate_dir / "release-evidence.json"
        if not verification_path.is_file():
            raise SystemExit(
                "prior stable run release-gate artifact is missing release-verification.json"
            )
        if not evidence_path.is_file():
            raise SystemExit(
                "prior stable run release-gate artifact is missing release-evidence.json"
            )

        run(
            [
                sys.executable,
                "scripts/release/validate_stable_dry_run.py",
                "--run-json",
                str(run_json),
                "--verification",
                str(verification_path),
                "--evidence",
                str(evidence_path),
                "--current-evidence",
                str(evidence_path),
                "--commit",
                commit_sha,
                "--version",
                version,
                "--tag",
                tag,
            ],
            cwd=root,
            capture=False,
        )

        notes_path = temp_dir / "RELEASE_NOTES.md"
        run(
            [
                sys.executable,
                "scripts/release/extract_changelog.py",
                "--version",
                version,
                "--require-released",
                "--output",
                str(notes_path),
            ],
            cwd=root,
            capture=False,
        )

        evidence = load_json(evidence_path, "prior stable release evidence")

        real_bundle_path = (root / args.real_evidence_bundle).resolve()
        if not real_bundle_path.is_file():
            raise SystemExit(
                "stable publication requires the exact local real-evidence bundle "
                f"used for the successful dry-run: {real_bundle_path}"
            )
        real_payload = real_bundle_path.read_text(encoding="utf-8").strip()
        real_summary_path = temp_dir / "real-evidence-summary.json"
        run(
            [
                sys.executable,
                "scripts/release/real_evidence_bundle.py",
                "verify",
                "--payload-file",
                str(real_bundle_path),
                "--commit",
                commit_sha,
                "--output-dir",
                str(temp_dir / "real-evidence"),
                "--summary",
                str(real_summary_path),
            ],
            cwd=root,
            capture=False,
        )
        real_summary = load_json(real_summary_path, "stable real evidence summary")
        prior_bundle = evidence.get("realEvidenceBundle")
        if not isinstance(prior_bundle, dict):
            raise SystemExit("prior stable release evidence is missing realEvidenceBundle")
        if prior_bundle.get("schema") != REAL_EVIDENCE_BUNDLE_SCHEMA:
            raise SystemExit("prior stable release evidence bundle schema mismatch")
        if str(prior_bundle.get("sourceSha", "")).lower() != commit_sha:
            raise SystemExit("prior stable release evidence bundle source SHA mismatch")
        if prior_bundle.get("payloadSha256") != real_summary.get("payloadSha256"):
            raise SystemExit(
                "local real-evidence bundle differs from the successful stable dry-run"
            )

        workflow_inputs = stable_publish_inputs(
            evidence,
            args.stable_qualification_run,
            real_payload,
        )

    summary = {
        "schema": "codex-tui/stable-publication-preflight/v1",
        "version": version,
        "tag": tag,
        "commitSha": commit_sha,
        "stableQualificationRun": args.stable_qualification_run,
        "realEvidenceBundle": str((root / args.real_evidence_bundle).resolve()),
        "realEvidenceBundleSha256": evidence["realEvidenceBundle"]["payloadSha256"],
        "mainProtected": True,
        "priorDryRun": "verified",
        "releasedChangelog": "verified",
        "tagCollision": False,
        "immutableReleases": immutable_releases,
        "workflowInputs": workflow_inputs,
        "dispatchRequested": args.dispatch,
        "next": (
            "stable publish=true workflow dispatched"
            if args.dispatch
            else "rerun with --dispatch to submit these exact retained inputs"
        ),
    }

    if args.dispatch:
        command = [
            "gh",
            "workflow",
            "run",
            "release.yml",
            "--repo",
            github_repo,
            "--ref",
            "main",
        ]
        for key, value in workflow_inputs.items():
            command.extend(["-f", f"{key}={value}"])
        run(command, cwd=root, capture=False)

    output_path = (root / args.output).resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    write_text_lf(
        output_path,
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
    )
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
