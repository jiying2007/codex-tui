#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re

from _compat import write_text_lf

HEX64 = re.compile(r"^[0-9a-fA-F]{64}$")
HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
PRIMARY_PLATFORM = "linux"
SECONDARY_PLATFORMS = ("macos", "windows")
PLATFORMS = (PRIMARY_PLATFORM, *SECONDARY_PLATFORMS)


def nonempty(value: str, label: str) -> str:
    value = value.strip()
    if not value:
        raise SystemExit(f"{label} must not be empty")
    return value


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True)
    parser.add_argument("--schema", default="codex-tui/release-evidence/v4")
    parser.add_argument("--compat-schema", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--canonical-ci-run", required=True, type=int)
    parser.add_argument("--automated-qualification", required=True)
    parser.add_argument("--performance-source-sha", required=True)
    parser.add_argument("--performance-iterations", required=True, type=int)
    parser.add_argument("--performance-p95-ms", required=True, type=float)
    parser.add_argument("--performance-p99-ms", required=True, type=float)
    parser.add_argument("--performance-source", required=True)
    parser.add_argument("--performance-observed-at", required=True)
    for platform in PLATFORMS:
        required = platform == PRIMARY_PLATFORM
        parser.add_argument(f"--{platform}-source-sha", required=required, default="")
        parser.add_argument(f"--{platform}-compat-sha256", required=required, default="")
        parser.add_argument(
            f"--{platform}-compat-observed-at", required=required, default=""
        )
        parser.add_argument(f"--{platform}-terminal", required=required, default="")
        parser.add_argument(
            f"--{platform}-terminal-observed-at", required=required, default=""
        )
    args = parser.parse_args()

    if not HEX40.fullmatch(args.commit):
        raise SystemExit("commit must be exactly 40 hexadecimal characters")
    if args.canonical_ci_run <= 0:
        raise SystemExit("canonical CI run must be nonzero")

    compatibility = {}
    terminal = {}
    for platform in PLATFORMS:
        source_sha = getattr(args, f"{platform}_source_sha").strip().lower()
        compat_sha = getattr(args, f"{platform}_compat_sha256").strip()
        compat_at = getattr(args, f"{platform}_compat_observed_at").strip()
        terminal_name = getattr(args, f"{platform}_terminal").strip()
        terminal_at = getattr(args, f"{platform}_terminal_observed_at").strip()

        supplied = [
            bool(source_sha),
            bool(compat_sha),
            bool(compat_at),
            bool(terminal_name),
            bool(terminal_at),
        ]
        if platform != PRIMARY_PLATFORM and not any(supplied):
            continue
        if not all(supplied):
            raise SystemExit(
                f"{platform} secondary evidence must be either fully omitted or fully provided"
            )
        if not HEX40.fullmatch(source_sha):
            raise SystemExit(f"{platform} source SHA must be exactly 40 hexadecimal characters")
        if source_sha != args.commit.lower():
            raise SystemExit(
                f"{platform} evidence source SHA mismatch: {source_sha} != {args.commit.lower()}"
            )
        if not HEX64.fullmatch(compat_sha):
            raise SystemExit(f"{platform} compatibility SHA-256 must be 64 hex characters")
        compatibility[platform] = {
            "status": "ready",
            "sourceSha": source_sha,
            "reportSha256": compat_sha.lower(),
            "observedAt": nonempty(compat_at, f"{platform} compatibility observed-at"),
        }
        terminal[platform] = {
            "status": "pass",
            "sourceSha": source_sha,
            "terminal": nonempty(terminal_name, f"{platform} terminal"),
            "observedAt": nonempty(terminal_at, f"{platform} terminal observed-at"),
        }

    performance_source_sha = args.performance_source_sha.strip().lower()
    if not HEX40.fullmatch(performance_source_sha):
        raise SystemExit("--performance-source-sha must be exactly 40 hexadecimal characters")
    if performance_source_sha != args.commit.lower():
        raise SystemExit("performance source SHA mismatch")
    if args.performance_iterations < 200:
        raise SystemExit("stable performance diagnostics require at least 200 iterations")
    if args.performance_p95_ms < 0 or args.performance_p99_ms < 0:
        raise SystemExit("performance diagnostics must be nonnegative")

    automated_path = pathlib.Path(args.automated_qualification)
    automated = json.loads(automated_path.read_text(encoding="utf-8"))
    if automated.get("schema") != "codex-tui/automated-qualification/v3":
        raise SystemExit("unexpected automated qualification schema")
    if str(automated.get("sourceSha", "")).lower() != args.commit.lower():
        raise SystemExit("automated qualification source SHA mismatch")
    gates = automated.get("gates", {})
    required_gates = (
        "failureMatrix",
        "scaleEvidence",
        "soakStructural",
        "uiContract",
        "stateMigrationRecovery",
        "supportBundleRedaction",
    )
    failed = [gate for gate in required_gates if gates.get(gate) != "pass"]
    if failed:
        raise SystemExit("automated qualification gate did not pass: " + ", ".join(failed))

    receipt = {
        "schema": args.schema,
        "version": args.version,
        "commitSha": args.commit.lower(),
        "canonicalCiRun": args.canonical_ci_run,
        "compatSchema": args.compat_schema,
        "primaryPlatform": PRIMARY_PLATFORM,
        "secondaryPlatforms": list(SECONDARY_PLATFORMS),
        "compatibility": compatibility,
        "terminalRestoration": terminal,
        "automatedQualification": automated,
        "performance": {
            "platform": PRIMARY_PLATFORM,
            "fixture": "resident-planning-10k",
            "sourceSha": performance_source_sha,
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
    write_text_lf(output, json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(f"WROTE {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
