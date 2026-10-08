#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import tempfile
from typing import Optional

from _compat import write_text_lf

HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
SCHEMA = "codex-tui/forge-capability-fixture/v1"


def normalize_provider(value: object) -> Optional[str]:
    if value is None:
        return None
    text = str(value).strip().lower()
    return {
        "git-lab": "gitlab",
        "gitlab": "gitlab",
        "git-hub": "github",
        "github": "github",
    }.get(text, text or None)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Capture a secret-safe, observed Forge capability fixture from doctor bundle."
    )
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--expected-provider", choices=("gitlab", "github"))
    parser.add_argument("--expected-source-sha", default="")
    parser.add_argument("--require-authenticated", action="store_true")
    parser.add_argument(
        "--required-capability",
        action="append",
        default=[],
        metavar="NAME=STATE",
        help="require an observed capability state such as issues=available",
    )
    args = parser.parse_args()

    binary = pathlib.Path(args.binary).resolve()
    output = pathlib.Path(args.output).resolve()
    if not binary.is_file():
        raise SystemExit(f"binary does not exist: {binary}")
    if output.exists():
        raise SystemExit(f"refusing to overwrite retained fixture: {output}")

    expected_sha = args.expected_source_sha.strip().lower()
    if expected_sha and not HEX40.fullmatch(expected_sha):
        raise SystemExit("--expected-source-sha must be exactly 40 hexadecimal characters")

    required_capabilities = {}
    for raw in args.required_capability:
        name, sep, state = raw.partition("=")
        name = name.strip()
        state = state.strip()
        if not sep or not name or not state:
            raise SystemExit("--required-capability must be NAME=STATE")
        required_capabilities[name] = state

    with tempfile.TemporaryDirectory() as temp:
        bundle = pathlib.Path(temp) / "bundle"
        try:
            proc = subprocess.run(
                [str(binary), "doctor", "bundle", "--output", str(bundle)],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=45,
            )
        except subprocess.TimeoutExpired:
            # Child stdout/stderr can contain server URLs or auth diagnostics.
            # Never echo arbitrary failure output into user or CI logs.
            raise SystemExit("doctor bundle timed out after 45s (output redacted)")
        if proc.returncode != 0:
            raise SystemExit(
                f"doctor bundle failed with exit code {proc.returncode} (output redacted)"
            )

        snapshot_path = bundle / "snapshot.json"
        snapshot = json.loads(snapshot_path.read_text(encoding="utf-8"))

    if snapshot.get("schema") != "codex-tui/support-bundle/v1":
        raise SystemExit("unexpected support bundle schema")

    build = snapshot.get("build")
    forge = snapshot.get("forge")
    if not isinstance(build, dict) or not isinstance(forge, dict):
        raise SystemExit("support snapshot is missing build/forge sections")

    provider = normalize_provider(forge.get("provider"))
    authenticated = forge.get("authenticated")
    capabilities = forge.get("capabilities")
    if not isinstance(capabilities, dict):
        capabilities = {}

    blockers = []
    if args.expected_provider and provider != args.expected_provider:
        blockers.append(
            f"provider mismatch: expected {args.expected_provider}, observed {provider or 'unavailable'}"
        )
    source_sha = str(build.get("sourceSha", "")).strip().lower()
    if expected_sha and source_sha != expected_sha:
        blockers.append(
            f"source SHA mismatch: expected {expected_sha}, observed {source_sha or 'unknown'}"
        )
    if args.require_authenticated and authenticated is not True:
        blockers.append("forge authentication is not confirmed")
    for name, state in sorted(required_capabilities.items()):
        observed = capabilities.get(name)
        if observed != state:
            blockers.append(
                f"capability {name} mismatch: expected {state}, observed {observed!r}"
            )

    fixture = {
        "schema": SCHEMA,
        "qualified": not blockers,
        "blockers": blockers,
        "observedAtUnixMs": snapshot.get("generatedAtUnixMs"),
        "build": {
            "productVersion": build.get("productVersion"),
            "sourceSha": source_sha or "unknown",
            "os": build.get("os"),
            "arch": build.get("arch"),
        },
        "forge": {
            "provider": provider,
            "clientName": forge.get("clientName"),
            "clientVersion": forge.get("clientVersion"),
            "authenticated": authenticated,
            "serverVersion": forge.get("serverVersion"),
            "serverEdition": forge.get("serverEdition"),
            "serverTier": forge.get("serverTier"),
            "freshness": forge.get("freshness"),
            "capabilities": capabilities,
            "recentIssues": forge.get("recentIssues"),
            "openChangeRequests": forge.get("openChangeRequests"),
            "recentPipelines": forge.get("recentPipelines"),
            "issueBoards": forge.get("issueBoards"),
            "errorCode": forge.get("errorCode"),
        },
        "requirements": {
            "expectedProvider": args.expected_provider,
            "expectedSourceSha": expected_sha or None,
            "authenticated": args.require_authenticated,
            "capabilities": required_capabilities,
        },
        "privacy": [
            "no-authentication-tokens",
            "no-repository-paths",
            "no-remote-urls",
            "no-prompts-or-transcripts",
            "no-comment-bodies",
            "no-raw-errors",
        ],
    }

    output.parent.mkdir(parents=True, exist_ok=True)
    write_text_lf(output, json.dumps(fixture, indent=2, sort_keys=True) + "\n")
    print(json.dumps(fixture, indent=2, sort_keys=True))
    return 0 if fixture["qualified"] else 3


if __name__ == "__main__":
    raise SystemExit(main())
