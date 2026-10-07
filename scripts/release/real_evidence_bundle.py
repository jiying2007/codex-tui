#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import gzip
import hashlib
import io
import json
import math
import pathlib
import re
from typing import Optional, Tuple

from _compat import write_text_lf

BUNDLE_SCHEMA = "codex-tui/stable-real-evidence-bundle/v1"
SUMMARY_SCHEMA = "codex-tui/stable-real-evidence-summary/v1"
COMPAT_SCHEMA = "codex-tui/compat/v2"
TERMINAL_SCHEMA = "codex-tui/terminal-restoration/v1"
PERFORMANCE_SCHEMA = "codex-tui/performance/v2"
PERFORMANCE_FIXTURE = "resident-planning-10k"
HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
MAX_PAYLOAD_CHARS = 60000
MAX_DECOMPRESSED_BYTES = 1048576
MAX_FILE_BYTES = 65536

FILE_SPECS = {
    "linuxCompat": ("compat-linux.json", "compat", "linux"),
    "linuxTerminal": ("terminal-linux.json", "terminal", "linux"),
    "performance": ("performance-linux.json", "performance", "linux"),
    "macosCompat": ("compat-macos.json", "compat", "macos"),
    "macosTerminal": ("terminal-macos.json", "terminal", "macos"),
    "windowsCompat": ("compat-windows.json", "compat", "windows"),
    "windowsTerminal": ("terminal-windows.json", "terminal", "windows"),
}
REQUIRED_KEYS = {"linuxCompat", "linuxTerminal", "performance"}
OPTIONAL_PAIRS = (
    ("macosCompat", "macosTerminal"),
    ("windowsCompat", "windowsTerminal"),
)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def strict_b64decode(text: str, label: str) -> bytes:
    try:
        return base64.b64decode(
            text.encode("ascii"),
            altchars=b"-_",
            validate=True,
        )
    except (UnicodeEncodeError, ValueError) as error:
        raise SystemExit(f"{label} is not valid URL-safe base64: {error}") from error


def encode_b64(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).decode("ascii")


def load_json_bytes(data: bytes, label: str) -> dict:
    try:
        value = json.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise SystemExit(f"{label} is not valid UTF-8 JSON: {error}") from error
    require(isinstance(value, dict), f"{label} must be a JSON object")
    return value


def exact_source(value, commit: str, label: str) -> str:
    source = str(value or "").strip().lower()
    require(bool(HEX40.fullmatch(source)), f"{label} sourceSha must be exactly 40 hexadecimal characters")
    require(source == commit, f"{label} sourceSha does not match candidate commit")
    return source


def nonempty(value, label: str) -> str:
    text = str(value or "").strip()
    require(bool(text), f"{label} must not be empty")
    return text


def validate_compat(report: dict, platform: str, commit: str) -> dict:
    require(report.get("schema") == COMPAT_SCHEMA, f"{platform} compatibility schema mismatch")
    require(report.get("readiness") == "ready", f"{platform} compatibility readiness must be ready")
    require(report.get("os") == platform, f"{platform} compatibility os mismatch")
    source = exact_source(report.get("sourceSha"), commit, f"{platform} compatibility")
    observed = report.get("generatedAtUnixMs")
    require(
        isinstance(observed, int) and not isinstance(observed, bool) and observed >= 0,
        f"{platform} compatibility generatedAtUnixMs is invalid",
    )
    return {
        "status": "ready",
        "sourceSha": source,
        "observedAt": f"unix-ms:{observed}",
    }


def validate_terminal(report: dict, platform: str, commit: str) -> dict:
    require(report.get("schema") == TERMINAL_SCHEMA, f"{platform} terminal schema mismatch")
    require(report.get("platform") == platform, f"{platform} terminal platform mismatch")
    require(report.get("status") == "pass", f"{platform} terminal status must be pass")
    source = exact_source(report.get("sourceSha"), commit, f"{platform} terminal")
    observed = nonempty(report.get("observedAt"), f"{platform} terminal observedAt")
    nonempty(report.get("terminal"), f"{platform} terminal identity")
    return {
        "status": "pass",
        "sourceSha": source,
        "observedAt": observed,
    }


def validate_performance(report: dict, commit: str) -> dict:
    require(report.get("schema") == PERFORMANCE_SCHEMA, "performance schema mismatch")
    require(report.get("fixture") == PERFORMANCE_FIXTURE, "performance fixture mismatch")
    source_sha = exact_source(report.get("sourceSha"), commit, "performance")
    iterations = report.get("iterations")
    require(
        isinstance(iterations, int) and not isinstance(iterations, bool) and iterations >= 200,
        "performance iterations must be an integer >= 200",
    )
    require(report.get("sampleQualified") is True, "performance sampleQualified must be true")
    p95 = report.get("p95Ms")
    p99 = report.get("p99Ms")
    for label, value in (("p95Ms", p95), ("p99Ms", p99)):
        require(
            isinstance(value, (int, float))
            and not isinstance(value, bool)
            and math.isfinite(float(value))
            and float(value) >= 0,
            f"performance {label} must be finite and nonnegative",
        )
    require(float(p99) >= float(p95), "performance p99Ms must be >= p95Ms")
    return {
        "platform": "linux",
        "fixture": PERFORMANCE_FIXTURE,
        "sourceSha": source_sha,
        "iterations": iterations,
        "p95Ms": float(p95),
        "p99Ms": float(p99),
        "source": nonempty(report.get("source"), "performance source"),
        "observedAt": nonempty(report.get("observedAt"), "performance observedAt"),
    }


def validate_file_set(files: dict) -> None:
    require(isinstance(files, dict), "evidence bundle files must be an object")
    keys = set(files)
    require(REQUIRED_KEYS.issubset(keys), "evidence bundle is missing required Linux evidence")
    require(keys.issubset(set(FILE_SPECS)), f"evidence bundle has unknown files: {sorted(keys - set(FILE_SPECS))}")
    for left, right in OPTIONAL_PAIRS:
        require(
            (left in keys) == (right in keys),
            f"optional evidence must include both {left} and {right}",
        )


def validate_envelope(
    envelope: dict,
    *,
    commit: str,
    payload_sha256: str,
    payload_chars: int,
    output_dir: Optional[pathlib.Path] = None,
) -> dict:
    require(envelope.get("schema") == BUNDLE_SCHEMA, "real evidence bundle schema mismatch")
    exact_source(envelope.get("sourceSha"), commit, "real evidence bundle")
    files = envelope.get("files")
    validate_file_set(files)

    compatibility = {}
    terminal = {}
    performance = None
    file_receipts = {}

    if output_dir is not None:
        output_dir.mkdir(parents=True, exist_ok=True)

    for key in sorted(files):
        item = files[key]
        require(isinstance(item, dict), f"evidence bundle entry {key} must be an object")
        expected_name, kind, platform = FILE_SPECS[key]
        require(item.get("name") == expected_name, f"evidence bundle filename mismatch for {key}")
        encoded = item.get("contentBase64")
        require(isinstance(encoded, str) and encoded, f"evidence bundle content is missing for {key}")
        raw = strict_b64decode(encoded, f"evidence bundle {key}")
        require(len(raw) <= MAX_FILE_BYTES, f"evidence bundle {key} exceeds {MAX_FILE_BYTES} bytes")
        report = load_json_bytes(raw, f"evidence bundle {key}")
        digest = sha256_bytes(raw)
        file_receipts[key] = {
            "name": expected_name,
            "sha256": digest,
            "size": len(raw),
        }
        if kind == "compat":
            row = validate_compat(report, platform, commit)
            row["reportSha256"] = digest
            compatibility[platform] = row
        elif kind == "terminal":
            row = validate_terminal(report, platform, commit)
            row["receiptSha256"] = digest
            terminal[platform] = row
        else:
            performance = validate_performance(report, commit)
            performance["reportSha256"] = digest

        if output_dir is not None:
            (output_dir / expected_name).write_bytes(raw)

    require(performance is not None, "performance evidence is missing")
    return {
        "schema": SUMMARY_SCHEMA,
        "bundleSchema": BUNDLE_SCHEMA,
        "sourceSha": commit,
        "payloadSha256": payload_sha256,
        "payloadChars": payload_chars,
        "compatSchema": COMPAT_SCHEMA,
        "compatibility": compatibility,
        "terminalRestoration": terminal,
        "performance": performance,
        "files": file_receipts,
    }


def read_raw(path: pathlib.Path, label: str) -> bytes:
    require(path.is_file() and not path.is_symlink(), f"{label} file is missing or unsafe: {path}")
    raw = path.read_bytes()
    require(len(raw) <= MAX_FILE_BYTES, f"{label} exceeds {MAX_FILE_BYTES} bytes")
    return raw


def create_payload(args) -> Tuple[str, dict]:
    commit = args.commit.strip().lower()
    require(bool(HEX40.fullmatch(commit)), "--commit must be exactly 40 hexadecimal characters")

    paths = {
        "linuxCompat": pathlib.Path(args.linux_compat),
        "linuxTerminal": pathlib.Path(args.linux_terminal),
        "performance": pathlib.Path(args.performance),
    }
    optional = (
        ("macosCompat", args.macos_compat),
        ("macosTerminal", args.macos_terminal),
        ("windowsCompat", args.windows_compat),
        ("windowsTerminal", args.windows_terminal),
    )
    for key, value in optional:
        if value:
            paths[key] = pathlib.Path(value)

    files = {}
    for key, path in paths.items():
        name = FILE_SPECS[key][0]
        files[key] = {
            "name": name,
            "contentBase64": encode_b64(read_raw(path, key)),
        }
    envelope = {
        "schema": BUNDLE_SCHEMA,
        "sourceSha": commit,
        "files": files,
    }
    encoded = json.dumps(envelope, sort_keys=True, separators=(",", ":")).encode("utf-8")
    compressed = gzip.compress(encoded, compresslevel=9, mtime=0)
    payload = encode_b64(compressed)
    require(
        len(payload) <= MAX_PAYLOAD_CHARS,
        f"stable real evidence payload is {len(payload)} characters; maximum is {MAX_PAYLOAD_CHARS}",
    )
    summary = validate_envelope(
        envelope,
        commit=commit,
        payload_sha256=sha256_bytes(compressed),
        payload_chars=len(payload),
    )
    return payload, summary


def decode_payload(payload: str, commit: str, output_dir: Optional[pathlib.Path] = None) -> dict:
    payload = payload.strip()
    require(bool(payload), "stable real evidence payload is empty")
    require(len(payload) <= MAX_PAYLOAD_CHARS, f"stable real evidence payload exceeds {MAX_PAYLOAD_CHARS} characters")
    compressed = strict_b64decode(payload, "stable real evidence payload")
    try:
        with gzip.GzipFile(fileobj=io.BytesIO(compressed), mode="rb") as handle:
            decoded = handle.read(MAX_DECOMPRESSED_BYTES + 1)
    except OSError as error:
        raise SystemExit(f"stable real evidence payload is not valid gzip: {error}") from error
    require(
        len(decoded) <= MAX_DECOMPRESSED_BYTES,
        f"stable real evidence payload exceeds {MAX_DECOMPRESSED_BYTES} decompressed bytes",
    )
    envelope = load_json_bytes(decoded, "stable real evidence envelope")
    commit = commit.strip().lower()
    require(bool(HEX40.fullmatch(commit)), "--commit must be exactly 40 hexadecimal characters")
    return validate_envelope(
        envelope,
        commit=commit,
        payload_sha256=sha256_bytes(compressed),
        payload_chars=len(payload),
        output_dir=output_dir,
    )


def write_summary(path: pathlib.Path, summary: dict) -> None:
    write_text_lf(path, json.dumps(summary, indent=2, sort_keys=True) + "\n")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Create or verify a source-bound stable real-environment evidence bundle."
    )
    sub = parser.add_subparsers(dest="command", required=True)

    create = sub.add_parser("create")
    create.add_argument("--commit", required=True)
    create.add_argument("--linux-compat", required=True)
    create.add_argument("--linux-terminal", required=True)
    create.add_argument("--performance", required=True)
    create.add_argument("--macos-compat", default="")
    create.add_argument("--macos-terminal", default="")
    create.add_argument("--windows-compat", default="")
    create.add_argument("--windows-terminal", default="")
    create.add_argument("--output", required=True)
    create.add_argument("--summary", required=True)

    verify = sub.add_parser("verify")
    verify.add_argument("--payload-file", required=True)
    verify.add_argument("--commit", required=True)
    verify.add_argument("--output-dir", required=True)
    verify.add_argument("--summary", required=True)

    args = parser.parse_args()
    if args.command == "create":
        payload, summary = create_payload(args)
        write_text_lf(pathlib.Path(args.output), payload + "\n")
        write_summary(pathlib.Path(args.summary), summary)
        print(
            f"VALID stable real evidence bundle: chars={len(payload)} "
            f"sha256={summary['payloadSha256']}"
        )
        return 0

    payload = pathlib.Path(args.payload_file).read_text(encoding="utf-8")
    summary = decode_payload(payload, args.commit, pathlib.Path(args.output_dir))
    write_summary(pathlib.Path(args.summary), summary)
    print(
        f"VALID stable real evidence bundle: chars={summary['payloadChars']} "
        f"sha256={summary['payloadSha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
