#!/usr/bin/env python3
"""Record exact test execution for retained Failure Matrix evidence.

The measured wall time includes cargo/test-harness overhead. It is NOT a
per-case application recovery latency SLO and must never be promoted as one.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import time

HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
IDENTIFIER = re.compile(r"^[a-zA-Z0-9_:]+$")
PASS_COUNT = re.compile(r"test result: ok\.\s+(\d+) passed;")


def run_one(identifier: str) -> dict:
    if not IDENTIFIER.fullmatch(identifier):
        raise ValueError("failure evidence identifier is not a Rust test path")
    start = time.monotonic()
    try:
        result = subprocess.run(
            [
                "cargo", "test", "--locked", "--all-targets", "--all-features",
                "--", identifier, "--exact",
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            universal_newlines=True,
            timeout=45,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise ValueError("failure evidence test process deadline exceeded: " + identifier) from error
    elapsed = max(0, round((time.monotonic() - start) * 1000))
    matched = sum(int(value) for value in PASS_COUNT.findall(result.stdout))
    if result.returncode != 0 or matched < 1:
        raise ValueError(
            "failure evidence did not execute and pass: "
            + identifier + " (exit=" + str(result.returncode)
            + ", matched=" + str(matched) + ")"
        )
    return {
        "identifier": identifier,
        "status": "pass",
        "matchedTests": matched,
        "processWallMs": elapsed,
        "outputSha256": hashlib.sha256(result.stdout.encode("utf-8")).hexdigest(),
    }


def build_receipt(matrix: dict, commit: str, run=run_one) -> dict:
    if not HEX40.fullmatch(commit):
        raise ValueError("commit must be exactly 40 hexadecimal characters")
    if matrix.get("schema") != "codex-tui/failure-matrix/v2":
        raise ValueError("invalid retained Failure Matrix schema")
    cases = matrix.get("cases")
    if not isinstance(cases, list) or not cases or len(cases) > 100:
        raise ValueError("invalid Failure Matrix case count")
    observed = {}
    output = []
    ids = set()
    for case in cases:
        ident = case.get("id")
        evidence = case.get("evidence")
        max_ms = case.get("maxRecoveryMs")
        if (
            not isinstance(ident, str) or not ident or ident in ids
            or not isinstance(max_ms, int) or max_ms < 1
            or not isinstance(evidence, list) or not evidence or len(evidence) > 12
        ):
            raise ValueError("invalid case identity, deadline, or evidence list")
        ids.add(ident)
        tests = []
        for item in evidence:
            if not isinstance(item, str) or not IDENTIFIER.fullmatch(item):
                raise ValueError("invalid failure evidence identifier")
            if item not in observed:
                result = run(item)
                if result.get("status") != "pass" or result.get("matchedTests", 0) < 1:
                    raise ValueError("failure evidence test not proven: " + item)
                observed[item] = result
            tests.append(observed[item])
        output.append({
            "id": ident,
            "expected": case.get("expected"),
            "writesAllowed": case.get("writesAllowed"),
            "declaredMaxRecoveryMs": max_ms,
            "recoveryLatencyMeasured": False,
            "evidence": tests,
        })
    return {
        "schema": "codex-tui/failure-case-execution/v1",
        "sourceSha": commit.lower(),
        "status": "tests-executed",
        "measurementScope": (
            "individual cargo test process wall time including harness startup; "
            "not a measurement of in-application recovery latency"
        ),
        "caseCount": len(output),
        "uniqueTestCount": len(observed),
        "cases": output,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--matrix", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    matrix = json.loads(pathlib.Path(args.matrix).read_text(encoding="utf-8"))
    receipt = build_receipt(matrix, args.commit)
    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        "VERIFIED " + str(receipt["caseCount"]) + " failure cases via "
        + str(receipt["uniqueTestCount"]) + " individually executed tests"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
