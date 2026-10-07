#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re

HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
RELEASE_WORKFLOW = ".github/workflows/release.yml"
VERIFY_SCHEMA = "codex-tui/release-verification/v1"
EVIDENCE_SCHEMA = "codex-tui/release-evidence/v6"
AUTOMATED_SCHEMA = "codex-tui/automated-qualification/v3"
REAL_BUNDLE_SCHEMA = "codex-tui/stable-real-evidence-bundle/v1"
HEX64 = re.compile(r"^[0-9a-f]{64}$")

STABLE_BINDING_KEYS = (
    "schema",
    "version",
    "commitSha",
    "canonicalCiRun",
    "compatSchema",
    "primaryPlatform",
    "secondaryPlatforms",
    "compatibility",
    "terminalRestoration",
    "performance",
    "realEvidenceBundle",
)


def load_json(path: str, label: str) -> dict:
    try:
        value = json.loads(pathlib.Path(path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise SystemExit(f"{label} must be a JSON object")
    return value


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def validate_evidence_shape(evidence: dict, commit: str, version: str, label: str) -> None:
    require(evidence.get("schema") == EVIDENCE_SCHEMA, f"{label} schema mismatch")
    require(evidence.get("version") == version, f"{label} version mismatch")
    require(
        str(evidence.get("commitSha", "")).lower() == commit,
        f"{label} commit SHA mismatch",
    )
    real = evidence.get("realEvidenceBundle")
    require(isinstance(real, dict), f"{label} realEvidenceBundle missing")
    require(
        real.get("schema") == REAL_BUNDLE_SCHEMA,
        f"{label} realEvidenceBundle schema mismatch",
    )
    require(
        str(real.get("sourceSha", "")).lower() == commit,
        f"{label} realEvidenceBundle source SHA mismatch",
    )
    payload_sha = str(real.get("payloadSha256", "")).lower()
    require(
        bool(HEX64.fullmatch(payload_sha)),
        f"{label} realEvidenceBundle payload SHA-256 invalid",
    )
    payload_chars = real.get("payloadChars")
    require(
        isinstance(payload_chars, int)
        and not isinstance(payload_chars, bool)
        and 0 < payload_chars <= 60000,
        f"{label} realEvidenceBundle payload size invalid",
    )
    files = real.get("files")
    require(isinstance(files, dict), f"{label} realEvidenceBundle files missing")
    for key in ("linuxCompat", "linuxTerminal", "performance"):
        row = files.get(key)
        require(isinstance(row, dict), f"{label} realEvidenceBundle file missing: {key}")
        require(
            bool(HEX64.fullmatch(str(row.get("sha256", "")).lower())),
            f"{label} realEvidenceBundle file SHA-256 invalid: {key}",
        )

    automated = evidence.get("automatedQualification")
    require(isinstance(automated, dict), f"{label} automated qualification missing")
    require(
        automated.get("schema") == AUTOMATED_SCHEMA,
        f"{label} automated qualification schema mismatch",
    )
    require(
        str(automated.get("sourceSha", "")).lower() == commit,
        f"{label} automated qualification source SHA mismatch",
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Require a successful prior stable publish=false release run before "
            "stable publication, bound to the same exact source and retained evidence."
        )
    )
    parser.add_argument("--run-json", required=True)
    parser.add_argument("--verification", required=True)
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--current-evidence", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()

    commit = args.commit.strip().lower()
    require(bool(HEX40.fullmatch(commit)), "--commit must be exactly 40 hexadecimal characters")
    require(args.version.strip() != "", "--version must not be empty")
    require(args.tag == f"v{args.version}", "--tag must be the exact stable tag for --version")

    run = load_json(args.run_json, "prior stable workflow run")
    checks = {
        "workflow name": run.get("name") == "release",
        "workflow path": run.get("path") == RELEASE_WORKFLOW,
        "workflow event": run.get("event") == "workflow_dispatch",
        "conclusion": run.get("conclusion") == "success",
        "head branch": run.get("head_branch") == "main",
        "head SHA": str(run.get("head_sha", "")).lower() == commit,
    }
    failed = [name for name, ok in checks.items() if not ok]
    require(
        not failed,
        "prior stable workflow run mismatch: " + ", ".join(failed),
    )

    verification = load_json(args.verification, "prior stable release verification")
    verification_checks = {
        "schema": verification.get("schema") == VERIFY_SCHEMA,
        "channel": verification.get("channel") == "stable",
        "tag": verification.get("tag") == args.tag,
        "version": verification.get("version") == args.version,
        "commit SHA": str(verification.get("commitSha", "")).lower() == commit,
        "publish=false": verification.get("publish") is False,
        "valid=true": verification.get("valid") is True,
        "evidenceStatus=verified": verification.get("evidenceStatus") == "verified",
    }
    failed = [name for name, ok in verification_checks.items() if not ok]
    require(
        not failed,
        "prior stable verification is not an eligible dry-run: " + ", ".join(failed),
    )

    prior = load_json(args.evidence, "prior stable release evidence")
    current = load_json(args.current_evidence, "current stable release evidence")
    validate_evidence_shape(prior, commit, args.version, "prior stable release evidence")
    validate_evidence_shape(current, commit, args.version, "current stable release evidence")

    drift = [key for key in STABLE_BINDING_KEYS if prior.get(key) != current.get(key)]
    require(
        not drift,
        (
            "stable publication evidence drifted from the successful publish=false dry-run: "
            + ", ".join(drift)
        ),
    )

    print(
        "VALID prior stable publish=false run "
        f"{run.get('id')} for {commit}; retained external evidence is unchanged"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
