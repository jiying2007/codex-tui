#!/usr/bin/env python3
"""Validate REAL internal GitLab capability evidence for a first rollout.

Public stable releases are provider-neutral. A synthetic CI unit fixture
does not constitute internal-service qualification.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import pathlib
import re
import time

from _compat import write_text_lf

SCHEMA = "codex-tui/internal-gitlab-admission/v1"
INPUT_SCHEMA = "codex-tui/forge-capability-fixture/v1"
CAPABILITIES = ("issues", "merge-requests", "pipelines")
PRIVACY = ("no-authentication-tokens", "no-repository-paths",
           "no-remote-urls", "no-prompts-or-transcripts",
           "no-comment-bodies", "no-raw-errors")
MAX_AGE_MS = 7 * 24 * 3600 * 1000
CLOCK_SKEW_MS = 5 * 60 * 1000
HEX40 = re.compile(r"^[a-fA-F0-9]{40}$")


def validate(fixture: object, source_sha: str, now_ms: int) -> list[str]:
    if not isinstance(fixture, dict):
        return ["fixture-not-object"]
    failures = []
    if fixture.get("schema") != INPUT_SCHEMA:
        failures.append("schema-mismatch")
    if fixture.get("qualified") is not True or fixture.get("blockers") != []:
        failures.append("capture-not-qualified")
    build = fixture.get("build")
    build = build if isinstance(build, dict) else {}
    if str(build.get("sourceSha", "")).lower() != source_sha:
        failures.append("source-sha-mismatch")
    if build.get("os") != "linux":
        failures.append("linux-required")
    forge = fixture.get("forge")
    forge = forge if isinstance(forge, dict) else {}
    if forge.get("provider") != "gitlab" or forge.get("clientName") != "glab":
        failures.append("provider-mismatch")
    if forge.get("authenticated") is not True:
        failures.append("auth-unconfirmed")
    if forge.get("freshness") != "fresh" or forge.get("errorCode") is not None:
        failures.append("observation-not-fresh")
    observed_caps = forge.get("capabilities")
    observed_caps = observed_caps if isinstance(observed_caps, dict) else {}
    requirements = fixture.get("requirements")
    requirements = requirements if isinstance(requirements, dict) else {}
    if (requirements.get("expectedProvider") != "gitlab"
            or str(requirements.get("expectedSourceSha", "")).lower() != source_sha
            or requirements.get("authenticated") is not True):
        failures.append("capture-requirements-missing")
    required_caps = requirements.get("capabilities")
    required_caps = required_caps if isinstance(required_caps, dict) else {}
    for name in CAPABILITIES:
        if observed_caps.get(name) != "available" or required_caps.get(name) != "available":
            failures.append(name + "-not-qualified")
    privacy = fixture.get("privacy")
    if not isinstance(privacy, list) or not set(PRIVACY).issubset(set(map(str, privacy))):
        failures.append("privacy-contract-missing")
    observed = fixture.get("observedAtUnixMs")
    if type(observed) is not int or observed <= 0:
        failures.append("observation-time-invalid")
    elif observed > now_ms + CLOCK_SKEW_MS:
        failures.append("observation-in-future")
    elif now_ms - observed > MAX_AGE_MS:
        failures.append("observation-expired")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description="Gate first internal GitLab deployment.")
    parser.add_argument("--fixture", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    source_sha = args.source_sha.strip().lower()
    if not HEX40.fullmatch(source_sha):
        raise SystemExit("--source-sha must be a 40-character hexadecimal SHA")
    path = pathlib.Path(args.fixture)
    output = pathlib.Path(args.output)
    if output.exists():
        raise SystemExit("refusing to overwrite a retained admission receipt")
    raw = path.read_bytes()
    fixture = json.loads(raw)
    now = int(time.time() * 1000)
    failures = validate(fixture, source_sha, now)
    receipt = {
        "schema": SCHEMA,
        "sourceSha": source_sha,
        "qualified": not failures,
        "blockers": failures,
        "fixtureSha256": hashlib.sha256(raw).hexdigest(),
        "observedAtUnixMs": fixture.get("observedAtUnixMs") if isinstance(fixture, dict) else None,
        "checkedAtUnixMs": now,
        "requiredCapabilities": list(CAPABILITIES),
        "stablePublicationAuthority": False,
        "scope": "actual-representative-internal-gitlab-repository",
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    write_text_lf(output, json.dumps(receipt, sort_keys=True, indent=2) + "\n")
    print(json.dumps(receipt, sort_keys=True, indent=2))
    return 0 if not failures else 3


if __name__ == "__main__":
    raise SystemExit(main())
