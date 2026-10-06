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

from _compat import cargo_package, write_text_lf

HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
STABLE_VERSION = re.compile(
    r"^[1-9][0-9]*\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)$"
)
TERMINAL_SCHEMA = "codex-tui/terminal-restoration/v1"
COMPAT_SCHEMA = "codex-tui/compat/v2"
PERFORMANCE_SCHEMA = "codex-tui/performance/v2"


def run(
    command: list[str],
    *,
    cwd: pathlib.Path,
    capture: bool = True,
    check: bool = True,
    env_overrides=None,
) -> subprocess.CompletedProcess[str]:
    environment = os.environ.copy()
    if env_overrides:
        environment.update(env_overrides)
    proc = subprocess.run(
        command,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
        env=environment,
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


def load_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def stable_version_allowed(version: str) -> bool:
    return STABLE_VERSION.fullmatch(version) is not None


def github_repo_from_cargo(root: pathlib.Path) -> str:
    repository = cargo_package(root).get("repository") or ""
    match = re.fullmatch(r"https://github\.com/([^/]+/[^/]+?)(?:\.git)?/?", repository)
    if not match:
        raise SystemExit(f"unsupported GitHub repository URL: {repository}")
    return match.group(1)


def select_canonical_ci_run(payload: dict, commit_sha: str) -> dict:
    runs = payload.get("workflow_runs")
    if not isinstance(runs, list):
        raise SystemExit("GitHub Actions response is missing workflow_runs")

    commit = commit_sha.strip().lower()
    candidates = [
        item
        for item in runs
        if isinstance(item, dict)
        and item.get("name") == "ci"
        and item.get("path") == ".github/workflows/ci.yml"
        and item.get("event") == "push"
        and item.get("status") == "completed"
        and item.get("conclusion") == "success"
        and item.get("head_branch") == "main"
        and str(item.get("head_sha", "")).lower() == commit
        and isinstance(item.get("id"), int)
        and item["id"] > 0
    ]
    if not candidates:
        raise SystemExit(
            "no successful canonical ci push run found for current main HEAD; "
            "wait for canonical CI to finish or pass --canonical-ci-run explicitly"
        )
    return max(candidates, key=lambda item: item["id"])


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run the Linux Tier-1 stable qualification chain."
    )
    parser.add_argument(
        "--canonical-ci-run",
        type=int,
        default=0,
        help=(
            "successful canonical main CI run ID; omit to auto-discover the "
            "latest successful ci push run bound to current HEAD"
        ),
    )
    parser.add_argument("--terminal-receipt", required=True)
    parser.add_argument("--output-dir", default="release/evidence/linux")
    parser.add_argument("--source", default="")
    parser.add_argument("--github-repo", default="")
    parser.add_argument(
        "--dispatch",
        action="store_true",
        help="dispatch the stable publish=false GitHub workflow after local qualification passes",
    )
    args = parser.parse_args()

    if not sys.platform.startswith("linux"):
        raise SystemExit("linux_qualify.py must run on Linux")
    if args.canonical_ci_run < 0:
        raise SystemExit("--canonical-ci-run must be zero/omitted or a positive integer")

    root = pathlib.Path.cwd().resolve()
    output_dir = (root / args.output_dir).resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    dirty = run(["git", "status", "--porcelain"], cwd=root).stdout.strip()
    if dirty:
        raise SystemExit(
            "qualification requires a clean worktree; commit/stash local changes first"
        )

    commit_sha = run(["git", "rev-parse", "HEAD"], cwd=root).stdout.strip()
    if not HEX40.fullmatch(commit_sha):
        raise SystemExit(f"unexpected git SHA: {commit_sha!r}")

    branch = run(["git", "branch", "--show-current"], cwd=root).stdout.strip()
    if branch != "main":
        raise SystemExit(f"qualification must run from main; got {branch!r}")

    package = cargo_package(root)
    version = package["version"]
    if not stable_version_allowed(version):
        raise SystemExit(
            "Linux stable qualification requires a release version X.Y.Z "
            f"with major >= 1; got {version}"
        )
    if package.get("license") != "Apache-2.0":
        raise SystemExit("Cargo package license must be Apache-2.0")
    if not (root / "LICENSE").is_file():
        raise SystemExit("root LICENSE is missing")

    github_repo = args.github_repo.strip() or github_repo_from_cargo(root)
    ci_json = output_dir / "canonical-ci.json"
    canonical_ci_run = args.canonical_ci_run
    if canonical_ci_run > 0:
        ci = run(
            [
                "gh",
                "api",
                f"repos/{github_repo}/actions/runs/{canonical_ci_run}",
            ],
            cwd=root,
        )
        write_text_lf(ci_json, ci.stdout)
    else:
        listed = run(
            [
                "gh",
                "api",
                f"repos/{github_repo}/actions/runs?branch=main&event=push&per_page=100",
            ],
            cwd=root,
        )
        selected = select_canonical_ci_run(json.loads(listed.stdout), commit_sha)
        canonical_ci_run = selected["id"]
        write_text_lf(
            ci_json,
            json.dumps(selected, indent=2, sort_keys=True) + "\n",
        )
        print(f"AUTO canonical CI run: {canonical_ci_run}", file=sys.stderr)
    run(
        [
            sys.executable,
            "scripts/release/validate_ci_run.py",
            "--run-json",
            str(ci_json),
            "--commit",
            commit_sha,
        ],
        cwd=root,
        capture=False,
    )

    run(
        ["cargo", "test", "--locked", "--all-targets", "--all-features"],
        cwd=root,
        capture=False,
    )
    run(
        ["cargo", "build", "--release", "--locked"],
        cwd=root,
        capture=False,
        env_overrides={"CODEX_TUI_GIT_SHA": commit_sha},
    )
    binary = root / "target/release/codex-tui"
    if not binary.is_file():
        raise SystemExit(f"release binary missing: {binary}")

    failure_test_list = output_dir / "failure-evidence-test-list.txt"
    listed = run(
        [
            "cargo",
            "test",
            "--locked",
            "--all-targets",
            "--all-features",
            "--",
            "--list",
        ],
        cwd=root,
    )
    write_text_lf(failure_test_list, listed.stdout)

    failure_matrix_path = output_dir / "failure-matrix.json"
    matrix = run(
        [str(binary), "release", "failure-matrix", "--json"],
        cwd=root,
    )
    write_text_lf(failure_matrix_path, matrix.stdout)
    run(
        [
            sys.executable,
            "scripts/release/check_failure_evidence.py",
            "--matrix",
            str(failure_matrix_path),
            "--test-list",
            str(failure_test_list),
        ],
        cwd=root,
        capture=False,
    )

    compat_path = output_dir / "compat-linux.json"
    compat_summary_proc = run(
        [
            sys.executable,
            "scripts/release/capture_compat.py",
            "--binary",
            str(binary),
            "--output",
            str(compat_path),
            "--expected-source-sha",
            commit_sha,
        ],
        cwd=root,
    )
    compat_summary = json.loads(compat_summary_proc.stdout)
    if compat_summary.get("schema") != COMPAT_SCHEMA:
        raise SystemExit("compat summary schema mismatch")
    if compat_summary.get("readiness") != "ready":
        raise SystemExit("Linux compatibility is not ready")
    if str(compat_summary.get("sourceSha", "")).lower() != commit_sha.lower():
        raise SystemExit("Linux compatibility source SHA mismatch")

    scale_path = output_dir / "scale-evidence.json"
    scale = run(
        [
            str(binary),
            "release",
            "scale",
            "--rows",
            "50000",
            "--warmup",
            "5",
            "--iterations",
            "50",
            "--source",
            args.source.strip() or f"linux:{commit_sha[:12]}",
            "--json",
        ],
        cwd=root,
    )
    write_text_lf(scale_path, scale.stdout)

    soak_path = output_dir / "soak-evidence.json"
    soak = run(
        [
            str(binary),
            "soak",
            "--rows",
            "50000",
            "--cycles",
            "256",
            "--json",
        ],
        cwd=root,
    )
    write_text_lf(soak_path, soak.stdout)

    support_dir = output_dir / "support-bundle"
    if support_dir.exists():
        raise SystemExit(
            f"support bundle destination already exists; remove the previous generated bundle: {support_dir}"
        )
    run(
        [
            str(binary),
            "doctor",
            "bundle",
            "--output",
            str(support_dir),
        ],
        cwd=root,
        capture=False,
    )
    support_manifest = support_dir / "manifest.json"
    support_snapshot = support_dir / "snapshot.json"
    if not support_manifest.is_file():
        raise SystemExit("doctor bundle did not produce manifest.json")
    if not support_snapshot.is_file():
        raise SystemExit("doctor bundle did not produce snapshot.json")

    automated_path = output_dir / "automated-qualification.json"
    run(
        [
            sys.executable,
            "scripts/release/create_automated_qualification.py",
            "--output",
            str(automated_path),
            "--commit",
            commit_sha,
            "--failure-matrix",
            str(failure_matrix_path),
            "--scale",
            str(scale_path),
            "--soak",
            str(soak_path),
            "--support-manifest",
            str(support_manifest),
            "--support-snapshot",
            str(support_snapshot),
        ],
        cwd=root,
        capture=False,
    )

    source = args.source.strip() or f"linux:{commit_sha[:12]}"
    perf_path = output_dir / "performance-linux.json"
    perf = run(
        [
            str(binary),
            "release",
            "benchmark",
            "--warmup",
            "20",
            "--iterations",
            "200",
            "--source",
            source,
            "--json",
        ],
        cwd=root,
    )
    write_text_lf(perf_path, perf.stdout)
    performance = json.loads(perf.stdout)
    if performance.get("schema") != PERFORMANCE_SCHEMA:
        raise SystemExit("performance schema mismatch")
    if performance.get("fixture") != "resident-planning-10k":
        raise SystemExit("performance fixture mismatch")
    if str(performance.get("sourceSha", "")).lower() != commit_sha.lower():
        raise SystemExit("performance source SHA mismatch")
    if performance.get("iterations", 0) < 200:
        raise SystemExit("performance sample count is below 200")
    if performance.get("sampleQualified") is not True:
        raise SystemExit(
            "Linux performance diagnostic sample is invalid or too small: "
            f"iterations={performance.get('iterations')}"
        )

    terminal_path = pathlib.Path(args.terminal_receipt).resolve()
    terminal = load_json(terminal_path)
    if terminal.get("schema") != TERMINAL_SCHEMA:
        raise SystemExit("terminal receipt schema mismatch")
    if terminal.get("platform") != "linux":
        raise SystemExit("terminal receipt must be for linux")
    if terminal.get("status") != "pass":
        raise SystemExit("terminal receipt must have status=pass")
    if str(terminal.get("sourceSha", "")).lower() != commit_sha.lower():
        raise SystemExit("terminal receipt source SHA mismatch")
    if not str(terminal.get("terminal", "")).strip():
        raise SystemExit("terminal receipt terminal name is empty")
    if not str(terminal.get("observedAt", "")).strip():
        raise SystemExit("terminal receipt observedAt is empty")
    terminal_sha256 = hashlib.sha256(terminal_path.read_bytes()).hexdigest()

    evidence_path = output_dir / "release-evidence-linux.json"
    run(
        [
            sys.executable,
            "scripts/release/create_evidence.py",
            "--output",
            str(evidence_path),
            "--compat-schema",
            COMPAT_SCHEMA,
            "--version",
            version,
            "--commit",
            commit_sha,
            "--canonical-ci-run",
            str(canonical_ci_run),
            "--automated-qualification",
            str(automated_path),
            "--linux-source-sha",
            commit_sha,
            "--linux-compat-sha256",
            compat_summary["reportSha256"],
            "--linux-compat-observed-at",
            compat_summary["observedAt"],
            "--linux-terminal-sha256",
            terminal_sha256,
            "--linux-terminal-observed-at",
            terminal["observedAt"],
            "--performance-source-sha",
            commit_sha,
            "--performance-iterations",
            str(performance["iterations"]),
            "--performance-p95-ms",
            str(performance["p95Ms"]),
            "--performance-p99-ms",
            str(performance["p99Ms"]),
            "--performance-source",
            performance["source"],
            "--performance-observed-at",
            performance["observedAt"],
        ],
        cwd=root,
        capture=False,
    )

    verify_path = output_dir / "release-verification.json"
    verify = run(
        [
            str(binary),
            "release",
            "verify",
            "--channel",
            "stable",
            "--tag",
            f"v{version}",
            "--commit",
            commit_sha,
            "--evidence",
            str(evidence_path),
            "--json",
        ],
        cwd=root,
    )
    write_text_lf(verify_path, verify.stdout)
    verification = json.loads(verify.stdout)
    if verification.get("valid") is not True:
        raise SystemExit(
            "local stable verification failed: "
            + ", ".join(verification.get("blockers", []))
        )

    workflow_inputs = {
        "channel": "stable",
        "publish": "false",
        "canonical_ci_run": str(canonical_ci_run),
        "linux_compat_sha256": compat_summary["reportSha256"],
        "linux_compat_observed_at": compat_summary["observedAt"],
        "linux_terminal_sha256": terminal_sha256,
        "linux_terminal_observed_at": terminal["observedAt"],
        "performance_source_sha": commit_sha,
        "performance_iterations": str(performance["iterations"]),
        "performance_p95_ms": str(performance["p95Ms"]),
        "performance_p99_ms": str(performance["p99Ms"]),
        "performance_source": performance["source"],
        "performance_observed_at": performance["observedAt"],
    }

    summary = {
        "schema": "codex-tui/linux-qualification/v2",
        "version": version,
        "commitSha": commit_sha,
        "canonicalCiRun": canonical_ci_run,
        "compatReport": str(compat_path),
        "compatReportSha256": compat_summary["reportSha256"],
        "realEvidenceSourceSha": commit_sha,
        "terminalReceipt": str(terminal_path),
        "terminalReceiptSha256": terminal_sha256,
        "failureMatrix": str(failure_matrix_path),
        "failureEvidenceTestList": str(failure_test_list),
        "scaleEvidence": str(scale_path),
        "soakEvidence": str(soak_path),
        "supportBundleManifest": str(support_manifest),
        "supportBundleSnapshot": str(support_snapshot),
        "automatedQualification": str(automated_path),
        "performanceReport": str(perf_path),
        "performanceDiagnostics": {
            "iterations": performance["iterations"],
            "p95Ms": performance["p95Ms"],
            "p99Ms": performance["p99Ms"],
        },
        "releaseEvidence": str(evidence_path),
        "releaseVerification": str(verify_path),
        "localStableVerify": "pass",
        "workflowInputs": workflow_inputs,
        "next": (
            "stable publish=false workflow dispatched; after it succeeds, run "
            "scripts/release/stable_publish.py with that run ID"
            if args.dispatch
            else "use workflowInputs for stable publish=false, or run a fresh qualification with --dispatch"
        ),
    }
    if args.dispatch:
        remote_main = run(
            ["git", "ls-remote", "origin", "refs/heads/main"],
            cwd=root,
        ).stdout.strip()
        remote_sha = remote_main.split()[0] if remote_main else ""
        if remote_sha != commit_sha:
            raise SystemExit(
                "refusing stable dispatch because origin/main drifted: "
                f"local={commit_sha} remote={remote_sha or '<missing>'}"
            )

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
        summary["next"] = (
            "stable publish=false workflow dispatched; after it succeeds, run "
            "scripts/release/stable_publish.py with that run ID"
        )

    summary_path = output_dir / "qualification-summary.json"
    write_text_lf(
        summary_path,
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
    )

    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
