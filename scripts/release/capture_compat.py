#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import sys

from _compat import write_text_lf

HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--expected-source-sha", required=True)
    args = parser.parse_args()

    expected_sha = args.expected_source_sha.strip().lower()
    if not HEX40.fullmatch(expected_sha):
        raise SystemExit("--expected-source-sha must be exactly 40 hexadecimal characters")

    command = [args.binary, "doctor", "compat", "--json"]
    proc = subprocess.run(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if proc.returncode not in (0,):
        sys.stderr.write(proc.stderr)
        raise SystemExit(f"compat doctor failed with exit code {proc.returncode}")

    try:
        report = json.loads(proc.stdout)
    except json.JSONDecodeError as error:
        raise SystemExit(f"compat doctor did not emit valid JSON: {error}") from error

    if report.get("schema") != "codex-tui/compat/v2":
        raise SystemExit(f"unexpected compat schema: {report.get('schema')!r}")
    if report.get("readiness") != "ready":
        raise SystemExit(
            "stable compatibility capture requires readiness=ready; "
            f"got {report.get('readiness')!r}"
        )
    source_sha = str(report.get("sourceSha", "")).strip().lower()
    if source_sha != expected_sha:
        raise SystemExit(
            "compat source SHA mismatch: "
            f"expected {expected_sha}, observed {source_sha or 'unknown'}"
        )

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    encoded = (json.dumps(report, indent=2, sort_keys=True) + "\n").encode("utf-8")
    write_text_lf(output, encoded.decode("utf-8"))

    digest = hashlib.sha256(encoded).hexdigest()
    summary = {
        "schema": report["schema"],
        "platform": report.get("os"),
        "arch": report.get("arch"),
        "productVersion": report.get("productVersion"),
        "sourceSha": source_sha,
        "readiness": report["readiness"],
        "reportSha256": digest,
        "observedAt": f'unix-ms:{report.get("generatedAtUnixMs")}',
        "reportPath": str(output),
    }
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
