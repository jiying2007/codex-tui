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

    receipt = {
        "schema": args.schema,
        "version": args.version,
        "commitSha": args.commit.lower(),
        "canonicalCiRun": args.canonical_ci_run,
        "compatSchema": args.compat_schema,
        "compatibility": compatibility,
        "terminalRestoration": terminal,
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
