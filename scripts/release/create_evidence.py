#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re

from _compat import write_text_lf

RELEASE_EVIDENCE_SCHEMA = "codex-tui/release-evidence/v6"
REAL_SUMMARY_SCHEMA = "codex-tui/stable-real-evidence-summary/v1"
REAL_BUNDLE_SCHEMA = "codex-tui/stable-real-evidence-bundle/v1"
AUTOMATED_SCHEMA = "codex-tui/automated-qualification/v3"
HEX64 = re.compile(r"^[0-9a-f]{64}$")
HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
PRIMARY_PLATFORM = "linux"
SECONDARY_PLATFORMS = ("macos", "windows")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def load_json(path: pathlib.Path, label: str) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read {label}: {error}") from error
    require(isinstance(value, dict), f"{label} must be a JSON object")
    return value


def exact_source(value, commit: str, label: str) -> str:
    source = str(value or "").strip().lower()
    require(bool(HEX40.fullmatch(source)), f"{label} source SHA is invalid")
    require(source == commit, f"{label} source SHA mismatch")
    return source


def validate_real_summary(summary: dict, commit: str) -> dict:
    require(summary.get("schema") == REAL_SUMMARY_SCHEMA, "real evidence summary schema mismatch")
    require(summary.get("bundleSchema") == REAL_BUNDLE_SCHEMA, "real evidence bundle schema mismatch")
    exact_source(summary.get("sourceSha"), commit, "real evidence bundle")
    payload_sha = str(summary.get("payloadSha256", "")).strip().lower()
    require(bool(HEX64.fullmatch(payload_sha)), "real evidence payload SHA-256 is invalid")
    payload_chars = summary.get("payloadChars")
    require(
        isinstance(payload_chars, int) and not isinstance(payload_chars, bool) and 0 < payload_chars <= 60000,
        "real evidence payload character count is invalid",
    )

    compatibility = summary.get("compatibility")
    terminal = summary.get("terminalRestoration")
    performance = summary.get("performance")
    files = summary.get("files")
    require(isinstance(compatibility, dict), "real evidence compatibility map is missing")
    require(isinstance(terminal, dict), "real evidence terminal map is missing")
    require(isinstance(performance, dict), "real evidence performance receipt is missing")
    require(isinstance(files, dict) and files, "real evidence file receipts are missing")

    require(PRIMARY_PLATFORM in compatibility, "Linux compatibility evidence is missing")
    require(PRIMARY_PLATFORM in terminal, "Linux terminal evidence is missing")
    for platform in (PRIMARY_PLATFORM, *SECONDARY_PLATFORMS):
        compat = compatibility.get(platform)
        tty = terminal.get(platform)
        if platform != PRIMARY_PLATFORM and compat is None and tty is None:
            continue
        require(isinstance(compat, dict) and isinstance(tty, dict), f"{platform} evidence must include compatibility and terminal receipts")
        require(compat.get("status") == "ready", f"{platform} compatibility must be ready")
        require(tty.get("status") == "pass", f"{platform} terminal restoration must pass")
        exact_source(compat.get("sourceSha"), commit, f"{platform} compatibility")
        exact_source(tty.get("sourceSha"), commit, f"{platform} terminal")
        compat_sha = str(compat.get("reportSha256", "")).lower()
        tty_sha = str(tty.get("receiptSha256", "")).lower()
        require(bool(HEX64.fullmatch(compat_sha)), f"{platform} compatibility SHA-256 is invalid")
        require(bool(HEX64.fullmatch(tty_sha)), f"{platform} terminal SHA-256 is invalid")
        require(bool(str(compat.get("observedAt", "")).strip()), f"{platform} compatibility observedAt is missing")
        require(bool(str(tty.get("observedAt", "")).strip()), f"{platform} terminal observedAt is missing")

    exact_source(performance.get("sourceSha"), commit, "performance")
    require(performance.get("platform") == PRIMARY_PLATFORM, "performance platform must be linux")
    require(performance.get("fixture") == "resident-planning-10k", "performance fixture mismatch")
    perf_sha = str(performance.get("reportSha256", "")).lower()
    require(bool(HEX64.fullmatch(perf_sha)), "performance report SHA-256 is invalid")
    iterations = performance.get("iterations")
    require(isinstance(iterations, int) and not isinstance(iterations, bool) and iterations >= 200, "performance iterations must be >= 200")
    for field in ("p95Ms", "p99Ms"):
        value = performance.get(field)
        require(isinstance(value, (int, float)) and not isinstance(value, bool) and value >= 0, f"performance {field} is invalid")
    require(float(performance["p99Ms"]) >= float(performance["p95Ms"]), "performance p99Ms must be >= p95Ms")
    require(bool(str(performance.get("source", "")).strip()), "performance source is missing")
    require(bool(str(performance.get("observedAt", "")).strip()), "performance observedAt is missing")

    compact_files = {}
    for key, row in files.items():
        require(isinstance(row, dict), f"real evidence file receipt {key} is invalid")
        digest = str(row.get("sha256", "")).lower()
        size = row.get("size")
        name = str(row.get("name", "")).strip()
        require(bool(name), f"real evidence file receipt {key} name is missing")
        require(bool(HEX64.fullmatch(digest)), f"real evidence file receipt {key} SHA-256 is invalid")
        require(isinstance(size, int) and not isinstance(size, bool) and size >= 0, f"real evidence file receipt {key} size is invalid")
        compact_files[key] = {"name": name, "sha256": digest, "size": size}

    return {
        "schema": REAL_BUNDLE_SCHEMA,
        "sourceSha": commit,
        "payloadSha256": payload_sha,
        "payloadChars": payload_chars,
        "files": compact_files,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--canonical-ci-run", required=True, type=int)
    parser.add_argument("--automated-qualification", required=True)
    parser.add_argument("--real-evidence-summary", required=True)
    args = parser.parse_args()

    commit = args.commit.strip().lower()
    require(bool(HEX40.fullmatch(commit)), "commit must be exactly 40 hexadecimal characters")
    require(args.canonical_ci_run > 0, "canonical CI run must be nonzero")

    automated = load_json(pathlib.Path(args.automated_qualification), "automated qualification")
    require(automated.get("schema") == AUTOMATED_SCHEMA, "unexpected automated qualification schema")
    exact_source(automated.get("sourceSha"), commit, "automated qualification")
    gates = automated.get("gates", {})
    for gate in (
        "failureMatrix",
        "scaleEvidence",
        "soakStructural",
        "uiContract",
        "stateMigrationRecovery",
        "supportBundleRedaction",
    ):
        require(gates.get(gate) == "pass", f"automated qualification gate did not pass: {gate}")

    real = load_json(pathlib.Path(args.real_evidence_summary), "stable real evidence summary")
    bundle_receipt = validate_real_summary(real, commit)

    receipt = {
        "schema": RELEASE_EVIDENCE_SCHEMA,
        "version": args.version,
        "commitSha": commit,
        "canonicalCiRun": args.canonical_ci_run,
        "compatSchema": real.get("compatSchema"),
        "primaryPlatform": PRIMARY_PLATFORM,
        "secondaryPlatforms": list(SECONDARY_PLATFORMS),
        "compatibility": real["compatibility"],
        "terminalRestoration": real["terminalRestoration"],
        "automatedQualification": automated,
        "performance": real["performance"],
        "realEvidenceBundle": bundle_receipt,
    }

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    write_text_lf(output, json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(f"WROTE {output}")


if __name__ == "__main__":
    raise SystemExit(main())
