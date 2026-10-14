#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import subprocess

from _compat import require_python38

HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
DOCTOR_TIMEOUT_SECONDS = 45


def main() -> int:
    require_python38()
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--expected-source-sha", required=True)
    args = parser.parse_args()

    expected_sha = args.expected_source_sha.strip().lower()
    if not HEX40.fullmatch(expected_sha):
        raise SystemExit("--expected-source-sha must be exactly 40 hexadecimal characters")

    output = pathlib.Path(args.output)
    if output.exists() or output.is_symlink():
        raise SystemExit("refusing to overwrite retained compatibility report")

    command = [args.binary, "doctor", "compat", "--json"]
    try:
        proc = subprocess.run(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            timeout=DOCTOR_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired:
        raise SystemExit("compat doctor timed out after 45s (output redacted)") from None
    except (OSError, UnicodeError):
        raise SystemExit("compat doctor could not run or decode output (details redacted)") from None
    if proc.returncode != 0:
        raise SystemExit(
            f"compat doctor failed with exit code {proc.returncode} (output redacted)"
        )

    try:
        report = json.loads(proc.stdout)
    except json.JSONDecodeError:
        raise SystemExit("compat doctor did not emit valid JSON (output redacted)") from None
    if not isinstance(report, dict):
        raise SystemExit("compat doctor report must be a JSON object")

    if report.get("schema") != "codex-tui/compat/v2":
        raise SystemExit("unexpected compat schema (value redacted)")
    if report.get("readiness") != "ready":
        raise SystemExit(
            "stable compatibility capture requires readiness=ready (value redacted)"
        )
    observed_sha = report.get("sourceSha")
    if not isinstance(observed_sha, str) or observed_sha.strip().lower() != expected_sha:
        raise SystemExit("compat source SHA mismatch (value redacted)")
    source_sha = observed_sha.strip().lower()
    for field in ("os", "arch", "productVersion"):
        value = report.get(field)
        if not isinstance(value, str) or not value.strip():
            raise SystemExit(f"compat report {field} must be a nonempty string")
    observed_at = report.get("generatedAtUnixMs")
    if type(observed_at) is not int or observed_at <= 0:
        raise SystemExit("compat report observation time must be a positive integer")

    encoded = (json.dumps(report, indent=2, sort_keys=True) + "\n").encode("utf-8")
    output.parent.mkdir(parents=True, exist_ok=True)
    try:
        with output.open("xb") as handle:
            handle.write(encoded)
    except FileExistsError:
        # Recheck atomically: another collector may have won after preflight.
        raise SystemExit("refusing to overwrite retained compatibility report") from None
    except OSError:
        raise SystemExit("cannot retain compatibility report (details redacted)") from None

    digest = hashlib.sha256(encoded).hexdigest()
    summary = {
        "schema": report["schema"],
        "platform": report.get("os"),
        "arch": report.get("arch"),
        "productVersion": report.get("productVersion"),
        "sourceSha": source_sha,
        "readiness": report["readiness"],
        "reportSha256": digest,
        "observedAt": f"unix-ms:{observed_at}",
        "reportPath": str(output),
    }
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
