#!/usr/bin/env python3
import argparse
import hashlib
import json
import pathlib
import subprocess
import sys


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

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

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    encoded = (json.dumps(report, indent=2, sort_keys=True) + "\n").encode("utf-8")
    output.write_bytes(encoded)

    digest = hashlib.sha256(encoded).hexdigest()
    summary = {
        "schema": report["schema"],
        "platform": report.get("os"),
        "arch": report.get("arch"),
        "productVersion": report.get("productVersion"),
        "readiness": report["readiness"],
        "reportSha256": digest,
        "observedAt": f'unix-ms:{report.get("generatedAtUnixMs")}',
        "reportPath": str(output),
    }
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
