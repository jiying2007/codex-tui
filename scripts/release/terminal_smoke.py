#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import pathlib
import subprocess
import sys
import termios

from _compat import write_text_lf

PENDING_SCHEMA = "codex-tui/terminal-smoke-pending/v1"


def run(command: list[str], *, cwd: pathlib.Path, capture: bool = True) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(
        command,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
    )
    if proc.returncode != 0:
        if proc.stdout:
            sys.stderr.write(proc.stdout)
        if proc.stderr:
            sys.stderr.write(proc.stderr)
        raise SystemExit(f"command failed ({proc.returncode}): {' '.join(command)}")
    return proc


def require_real_tty() -> str:
    if not sys.platform.startswith("linux"):
        raise SystemExit("terminal_smoke.py is the Linux Tier-1 real-TTY helper")
    for stream, label in ((sys.stdin, "stdin"), (sys.stdout, "stdout"), (sys.stderr, "stderr")):
        if not stream.isatty():
            raise SystemExit(f"{label} is not attached to a real TTY")
    try:
        with open("/dev/tty", "rb", buffering=0):
            pass
    except OSError as error:
        raise SystemExit(f"no controlling TTY is available: {error}") from error
    return os.ttyname(sys.stdin.fileno())


def validate_restored_lflag(lflag: int) -> None:
    missing = []
    for bit, name in (
        (termios.ICANON, "ICANON"),
        (termios.ECHO, "ECHO"),
        (termios.ISIG, "ISIG"),
    ):
        if not (lflag & bit):
            missing.append(name)
    if missing:
        raise SystemExit(
            "parent terminal is not restored to canonical interactive mode; missing "
            + ", ".join(missing)
        )


def terminal_label(tty_path: str, override: str = "") -> str:
    if override.strip():
        return override.strip()
    term = os.environ.get("TERM", "unknown").strip() or "unknown"
    transport = "ssh" if os.environ.get("SSH_TTY") else "local"
    return f"{transport}:{term}:{tty_path}"


def repo_identity(root: pathlib.Path) -> str:
    dirty = run(["git", "status", "--porcelain"], cwd=root).stdout.strip()
    if dirty:
        raise SystemExit("real-TTY smoke requires a clean worktree")
    branch = run(["git", "branch", "--show-current"], cwd=root).stdout.strip()
    if branch != "main":
        raise SystemExit(f"real-TTY smoke must run from main; got {branch!r}")
    sha = run(["git", "rev-parse", "HEAD"], cwd=root).stdout.strip().lower()
    remote = run(["git", "ls-remote", "origin", "refs/heads/main"], cwd=root).stdout.strip()
    remote_sha = remote.split()[0].lower() if remote else ""
    if remote_sha != sha:
        raise SystemExit(
            "origin/main must equal local main for real-TTY evidence: "
            f"local={sha} remote={remote_sha or '<missing>'}"
        )
    return sha


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Two-phase Linux real controlling-TTY evidence helper. Prepare before the "
            "interactive smoke; record-pass only after the documented smoke succeeded."
        )
    )
    sub = parser.add_subparsers(dest="command", required=True)

    prepare = sub.add_parser("prepare")
    prepare.add_argument(
        "--pending",
        default="release/evidence/linux/terminal-smoke-pending.json",
    )
    prepare.add_argument("--terminal-label", default="")

    record = sub.add_parser("record-pass")
    record.add_argument(
        "--pending",
        default="release/evidence/linux/terminal-smoke-pending.json",
    )
    record.add_argument(
        "--output",
        default="release/evidence/linux/terminal-linux.json",
    )
    record.add_argument("--notes", default="")
    record.add_argument(
        "--pass",
        dest="passed",
        action="store_true",
        help="explicitly attest the documented real interactive smoke passed",
    )

    args = parser.parse_args()
    root = pathlib.Path.cwd().resolve()
    tty_path = require_real_tty()
    attrs = termios.tcgetattr(sys.stdin.fileno())
    validate_restored_lflag(attrs[3])
    source_sha = repo_identity(root)

    if args.command == "prepare":
        label = terminal_label(tty_path, args.terminal_label)
        candidate_command = (
            f'CODEX_TUI_GIT_SHA="{source_sha}" '
            "cargo run --release --locked --bin codex-tui"
        )
        pending = {
            "schema": PENDING_SCHEMA,
            "platform": "linux",
            "sourceSha": source_sha,
            "terminal": label,
            "ttyPath": tty_path,
            "term": os.environ.get("TERM", ""),
            "preparedAt": utc_now(),
            "humanObservationRequired": True,
            "procedure": "docs/implementation/m7c3-pty-lifecycle.md#interactive-smoke-procedure",
            "candidateCommand": candidate_command,
        }
        pending_path = (root / args.pending).resolve()
        write_text_lf(pending_path, json.dumps(pending, indent=2, sort_keys=True) + "\n")
        print(
            "\nREAL TTY SMOKE (human observation required):\n"
            f"  candidate: {candidate_command}\n"
            "  1. launch that exact-SHA candidate in a repository-backed thread\n"
            "  2. press t; run: echo CODEX_TUI_DRAWER_SMOKE\n"
            "  3. resize the host terminal in both dimensions\n"
            "  4. run a long command; Ctrl-C it; run a second echo\n"
            "  5. press F6; verify codex-tui navigation; t to refocus; T to close\n"
            "  6. quit normally; verify visible cursor, line editing and echo\n"
            "  7. complete the documented abnormal/panic-style restoration check\n"
            "  8. only after all observations pass, run record-pass --pass\n",
            file=sys.stderr,
        )
        print(json.dumps(pending, indent=2, sort_keys=True))
        return 0

    if not args.passed:
        raise SystemExit(
            "record-pass requires explicit --pass after completing the documented real-TTY smoke"
        )

    pending_path = (root / args.pending).resolve()
    try:
        pending = json.loads(pending_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read pending TTY smoke manifest: {error}") from error
    if pending.get("schema") != PENDING_SCHEMA:
        raise SystemExit("pending TTY smoke schema mismatch")
    if pending.get("platform") != "linux":
        raise SystemExit("pending TTY smoke must be Linux")
    if str(pending.get("sourceSha", "")).lower() != source_sha:
        raise SystemExit("pending TTY smoke source SHA no longer matches current main")
    if pending.get("ttyPath") != tty_path:
        raise SystemExit(
            f"record-pass must run on the same controlling TTY: "
            f"prepared={pending.get('ttyPath')!r} current={tty_path!r}"
        )
    if not pending.get("humanObservationRequired"):
        raise SystemExit("pending TTY smoke manifest does not require human observation")

    # Re-read terminal flags after the application smoke. This cannot prove cursor
    # visibility or user-observed line editing, so --pass remains mandatory.
    restored = termios.tcgetattr(sys.stdin.fileno())
    validate_restored_lflag(restored[3])

    output = (root / args.output).resolve()
    notes = (
        "terminal-smoke-v1; post-smoke flags ICANON/ECHO/ISIG verified; "
        + (args.notes.strip() or "documented interactive procedure observed PASS")
    )
    receipt = run(
        [
            sys.executable,
            "scripts/release/create_terminal_receipt.py",
            "--platform",
            "linux",
            "--terminal",
            str(pending["terminal"]),
            "--source-sha",
            source_sha,
            "--output",
            str(output),
            "--notes",
            notes,
            "--pass",
        ],
        cwd=root,
    )
    sys.stdout.write(receipt.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
