#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import tempfile
from typing import Optional

from _compat import require_python38

HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
SCHEMA = "codex-tui/forge-capability-fixture/v1"
CAPABILITY_NAME = re.compile(r"^[a-z][a-z0-9-]{0,63}$")
CAPABILITY_STATES = ("available", "unavailable", "unknown")
FRESHNESS_STATES = ("fresh", "aging", "stale", "unavailable")


def normalize_provider(value: object) -> Optional[str]:
    if value is None:
        return None
    if not isinstance(value, str):
        raise SystemExit("invalid Forge provider (value redacted)")
    provider = {
        "git-lab": "gitlab", "gitlab": "gitlab",
        "git-hub": "github", "github": "github",
    }.get(value.strip().lower())
    if provider is None:
        raise SystemExit("invalid Forge provider (value redacted)")
    return provider


def validate_snapshot(snapshot: object) -> None:
    # Check types before projecting or printing any child-supplied values.
    if not isinstance(snapshot, dict):
        raise SystemExit("support snapshot must be a JSON object")
    if snapshot.get("schema") != "codex-tui/support-bundle/v1":
        raise SystemExit("unexpected support bundle schema")
    build, forge = snapshot.get("build"), snapshot.get("forge")
    if not isinstance(build, dict) or not isinstance(forge, dict):
        raise SystemExit("support snapshot is missing build/forge sections")
    for field in ("productVersion", "sourceSha", "os", "arch"):
        value = build.get(field)
        if not isinstance(value, str) or not value.strip():
            raise SystemExit("invalid support build identity (value redacted)")
    source = build["sourceSha"].strip().lower()
    if source != "unknown" and not HEX40.fullmatch(source):
        raise SystemExit("invalid support source identity (value redacted)")
    observed = snapshot.get("generatedAtUnixMs")
    if type(observed) is not int or observed <= 0:
        raise SystemExit("support observation time must be a positive integer")
    normalize_provider(forge.get("provider"))
    if forge.get("authenticated") is not None and type(forge["authenticated"]) is not bool:
        raise SystemExit("invalid Forge authentication state (value redacted)")
    if forge.get("freshness") not in FRESHNESS_STATES:
        raise SystemExit("invalid Forge freshness state (value redacted)")
    capabilities = forge.get("capabilities")
    if not isinstance(capabilities, dict) or any(
        not isinstance(name, str) or not CAPABILITY_NAME.fullmatch(name)
        or state not in CAPABILITY_STATES
        for name, state in capabilities.items()
    ):
        raise SystemExit("invalid Forge capabilities (values redacted)")
    for field in ("clientName", "clientVersion", "serverVersion", "serverEdition", "serverTier"):
        value = forge.get(field)
        if value is not None and (not isinstance(value, str) or not value.strip()):
            raise SystemExit("invalid Forge metadata (value redacted)")
    for field in ("recentIssues", "openChangeRequests", "recentPipelines", "issueBoards"):
        value = forge.get(field)
        if value is not None and (type(value) is not int or value < 0):
            raise SystemExit("invalid Forge count (value redacted)")
    error = forge.get("errorCode")
    if error is not None and (not isinstance(error, str) or not CAPABILITY_NAME.fullmatch(error)):
        raise SystemExit("invalid Forge error code (value redacted)")


def main() -> int:
    require_python38()
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
    output = pathlib.Path(args.output)
    if not binary.is_file():
        raise SystemExit("binary does not exist or is not a regular file")
    if output.exists() or output.is_symlink():
        raise SystemExit("refusing to overwrite retained fixture")

    expected_sha = args.expected_source_sha.strip().lower()
    if expected_sha and not HEX40.fullmatch(expected_sha):
        raise SystemExit("--expected-source-sha must be exactly 40 hexadecimal characters")

    required_capabilities = {}
    for raw in args.required_capability:
        name, sep, state = raw.partition("=")
        name = name.strip()
        state = state.strip()
        if not sep or not CAPABILITY_NAME.fullmatch(name) or state not in CAPABILITY_STATES:
            raise SystemExit("--required-capability must be NAME=available|unavailable|unknown")
        if name in required_capabilities and required_capabilities[name] != state:
            raise SystemExit("conflicting duplicate capability requirements")
        required_capabilities[name] = state

    with tempfile.TemporaryDirectory() as temp:
        bundle = pathlib.Path(temp) / "bundle"
        try:
            proc = subprocess.run(
                [str(binary), "doctor", "bundle", "--output", str(bundle)],
                text=True,
                encoding="utf-8",
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=45,
            )
        except subprocess.TimeoutExpired:
            # Child stdout/stderr can contain server URLs or auth diagnostics.
            # Never echo arbitrary failure output into user or CI logs.
            raise SystemExit("doctor bundle timed out after 45s (output redacted)") from None
        except (OSError, UnicodeError):
            raise SystemExit("doctor bundle could not run or decode output (details redacted)") from None
        if proc.returncode != 0:
            raise SystemExit(
                f"doctor bundle failed with exit code {proc.returncode} (output redacted)"
            )

        snapshot_path = bundle / "snapshot.json"
        try:
            snapshot = json.loads(snapshot_path.read_text(encoding="utf-8"))
        except (OSError, ValueError, UnicodeError, RecursionError):
            raise SystemExit("cannot read valid support snapshot (details redacted)") from None

    validate_snapshot(snapshot)
    build, forge = snapshot["build"], snapshot["forge"]
    provider = normalize_provider(forge.get("provider"))
    authenticated = forge.get("authenticated")
    capabilities = forge["capabilities"]

    blockers = []
    if args.expected_provider and provider != args.expected_provider:
        blockers.append(
            "provider mismatch (observed value redacted)"
        )
    source_sha = str(build.get("sourceSha", "")).strip().lower()
    if expected_sha and source_sha != expected_sha:
        blockers.append(
            "source SHA mismatch (observed value redacted)"
        )
    if args.require_authenticated and authenticated is not True:
        blockers.append("forge authentication is not confirmed")
    for name, state in sorted(required_capabilities.items()):
        observed = capabilities.get(name)
        if observed != state:
            blockers.append(
                f"capability {name} mismatch (observed value redacted)"
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

    encoded = json.dumps(fixture, indent=2, sort_keys=True) + "\n"
    try:
        output.parent.mkdir(parents=True, exist_ok=True)
        with output.open("xb") as handle:
            handle.write(encoded.encode("utf-8"))
    except FileExistsError:
        raise SystemExit("refusing to overwrite retained fixture") from None
    except OSError:
        raise SystemExit("cannot retain Forge fixture (details redacted)") from None
    print(encoded, end="")
    return 0 if fixture["qualified"] else 3


if __name__ == "__main__":
    raise SystemExit(main())
