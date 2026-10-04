#!/usr/bin/env python3
"""Fail closed on strong imported GLIBC requirements above our Linux ABI floor."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

SCHEMA = "codex-tui/linux-abi/v1"
MAX_GLIBC = "2.31"

def version(value: str) -> tuple[int, ...]:
    if not re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", value):
        raise ValueError("invalid GLIBC version: " + value)
    parts = tuple(int(part) for part in value.split("."))
    return parts + (0,) * (3 - len(parts))

def inspect_symbols(text: str, maximum: str = MAX_GLIBC) -> dict:
    strong, weak = set(), set()
    for line in text.splitlines():
        columns = line.split()
        if len(columns) < 8 or columns[6] != "UND" or "@GLIBC_" not in columns[7]:
            continue
        name = columns[7]
        match = re.search(r"@+GLIBC_([0-9.]+)$", name)
        if not match:
            raise ValueError("unsupported imported GLIBC symbol: " + name)
        value = match.group(1)
        version(value)
        (weak if columns[4] == "WEAK" else strong).add(value)
    if not strong:
        raise ValueError("no strong versioned GLIBC imports found; wrong binary or incomplete readelf output")
    required = max(strong, key=version)
    if version(required) > version(maximum):
        raise ValueError("required GLIBC %s exceeds declared baseline %s" % (required, maximum))
    return {"maximumGlibc": maximum, "requiredGlibc": required,
            "strongVersions": sorted(strong, key=version), "weakVersions": sorted(weak, key=version)}

def inspect_binary(binary: Path, source_sha: str) -> dict:
    if not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        raise ValueError("source SHA must be a full lowercase Git SHA")
    text = subprocess.run(["readelf", "--dyn-syms", "--wide", str(binary)], check=True,
                          capture_output=True, text=True, timeout=30).stdout
    return dict(inspect_symbols(text), schema=SCHEMA, sourceSha=source_sha,
                binarySha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                baseline="Ubuntu 20.04 / glibc 2.31", policy="strong-imports-only", passed=True)

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = inspect_binary(args.binary, args.commit)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("Linux ABI PASS: required GLIBC <= " + MAX_GLIBC)
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
