#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
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
) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(
        command,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
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


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run the Linux Tier-1 stable qualification chain."
    )
    parser.add_argument("--canonical-ci-run", required=True, type=int)
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
    if args.canonical_ci_run <= 0:
        raise SystemExit("--canonical-ci-run must be nonzero")

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
    ci = run(
        [
            "gh",
            "api",
            f"repos/{github_repo}/actions/runs/{args.canonical_ci_run}",
        ],
        cwd=root,
    )
    write_text_lf(ci_json, ci.stdout)
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
    run(["cargo", "build", "--release", "--locked"], cwd=root, capture=False)
    binary = root / "target/release/codex-tui"
    if not binary.is_file():
        raise SystemExit(f"release binary missing: {binary}")

    compat_path = output_dir / "compat-linux.json"
    compat_summary_proc = run(
        [
            sys.executable,
            "scripts/release/capture_compat.py",
            "--binary",
            str(binary),
            "--output",
            str(compat_path),
        ],
        cwd=root,
    )
    compat_summary = json.loads(compat_summary_proc.stdout)
    if compat_summary.get("schema") != COMPAT_SCHEMA:
        raise SystemExit("compat summary schema mismatch")
    if compat_summary.get("readiness") != "ready":
        raise SystemExit("Linux compatibility is not ready")

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
    if not support_manifest.is_file():
        raise SystemExit("doctor bundle did not produce manifest.json")

    automated_path = output_dir / "automated-qualification.json"
    run(
        [
            sys.executable,
            "scripts/release/create_automated_qualification.py",
            "--output",
            str(automated_path),
            "--commit",
            commit_sha,
            "--soak",
            str(soak_path),
            "--support-manifest",
            str(support_manifest),
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
    if not str(terminal.get("terminal", "")).strip():
        raise SystemExit("terminal receipt terminal name is empty")
    if not str(terminal.get("observedAt", "")).strip():
        raise SystemExit("terminal receipt observedAt is empty")

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
            str(args.canonical_ci_run),
            "--automated-qualification",
            str(automated_path),
            "--linux-compat-sha256",
            compat_summary["reportSha256"],
            "--linux-compat-observed-at",
            compat_summary["observedAt"],
            "--linux-terminal",
            terminal["terminal"],
            "--linux-terminal-observed-at",
            terminal["observedAt"],
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
        "canonical_ci_run": str(args.canonical_ci_run),
        "linux_compat_sha256": compat_summary["reportSha256"],
        "linux_compat_observed_at": compat_summary["observedAt"],
        "linux_terminal": terminal["terminal"],
        "linux_terminal_observed_at": terminal["observedAt"],
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
        "canonicalCiRun": args.canonical_ci_run,
        "compatReport": str(compat_path),
        "compatReportSha256": compat_summary["reportSha256"],
        "terminalReceipt": str(terminal_path),
        "soakEvidence": str(soak_path),
        "supportBundleManifest": str(support_manifest),
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
            "stable publish=false workflow dispatched"
            if args.dispatch
            else "rerun with --dispatch to start stable publish=false qualification"
        ),
    }
    summary_path = output_dir / "qualification-summary.json"
    write_text_lf(
        summary_path,
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
    )

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

    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
