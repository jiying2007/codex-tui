#!/usr/bin/env python3
import argparse
import json
import pathlib
import re

HEX64 = re.compile(r"^[0-9a-fA-F]{64}$")
HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
PLATFORMS = ("linux", "macos", "windows")


def nonempty(value: str, label: str) -> str:
    value = value.strip()
    if not value:
        raise SystemExit(f"{label} must not be empty")
    return value


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True)
    parser.add_argument("--schema", default="codex-tui/release-evidence/v1")
    parser.add_argument("--compat-schema", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--canonical-ci-run", required=True, type=int)
    parser.add_argument("--performance-iterations", required=True, type=int)
    parser.add_argument("--performance-p95-ms", required=True, type=float)
    parser.add_argument("--performance-p99-ms", required=True, type=float)
    parser.add_argument("--performance-source", required=True)
    parser.add_argument("--performance-observed-at", required=True)
    for platform in PLATFORMS:
        parser.add_argument(f"--{platform}-compat-sha256", required=True)
        parser.add_argument(f"--{platform}-compat-observed-at", required=True)
        parser.add_argument(f"--{platform}-terminal", required=True)
        parser.add_argument(f"--{platform}-terminal-observed-at", required=True)
    args = parser.parse_args()

    if not HEX40.fullmatch(args.commit):
        raise SystemExit("commit must be exactly 40 hexadecimal characters")
    if args.canonical_ci_run <= 0:
        raise SystemExit("canonical CI run must be nonzero")

    compatibility = {}
    terminal = {}
    for platform in PLATFORMS:
        compat_sha = getattr(args, f"{platform}_compat_sha256")
        if not HEX64.fullmatch(compat_sha):
            raise SystemExit(f"{platform} compatibility SHA-256 must be 64 hex characters")
        compatibility[platform] = {
            "status": "ready",
            "reportSha256": compat_sha.lower(),
            "observedAt": nonempty(
                getattr(args, f"{platform}_compat_observed_at"),
                f"{platform} compatibility observed-at",
            ),
        }
        terminal[platform] = {
            "status": "pass",
            "terminal": nonempty(
                getattr(args, f"{platform}_terminal"),
                f"{platform} terminal",
            ),
            "observedAt": nonempty(
                getattr(args, f"{platform}_terminal_observed_at"),
                f"{platform} terminal observed-at",
            ),
        }

    if args.performance_iterations < 200:
        raise SystemExit("stable performance evidence requires at least 200 iterations")
    if not (0 <= args.performance_p95_ms <= 50):
        raise SystemExit("stable performance p95 must be between 0 and 50 ms")
    if not (0 <= args.performance_p99_ms <= 100):
        raise SystemExit("stable performance p99 must be between 0 and 100 ms")

    receipt = {
        "schema": args.schema,
        "version": args.version,
        "commitSha": args.commit.lower(),
        "canonicalCiRun": args.canonical_ci_run,
        "compatSchema": args.compat_schema,
        "compatibility": compatibility,
        "terminalRestoration": terminal,
        "performance": {
            "fixture": "resident-planning-10k",
            "iterations": args.performance_iterations,
            "p95Ms": args.performance_p95_ms,
            "p99Ms": args.performance_p99_ms,
            "source": nonempty(args.performance_source, "performance source"),
            "observedAt": nonempty(
                args.performance_observed_at,
                "performance observed-at",
            ),
        },
    }

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(f"WROTE {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
