#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime as dt
import json
import pathlib
import sys

from _compat import write_text_lf

SCHEMA = "codex-tui/terminal-restoration/v1"


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Create an explicit retained real-TTY restoration receipt."
    )
    parser.add_argument("--platform", choices=["linux", "macos", "windows"], required=True)
    parser.add_argument("--terminal", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--notes", default="")
    parser.add_argument(
        "--pass",
        dest="passed",
        action="store_true",
        help="Explicitly attest that the documented restoration smoke passed.",
    )
    args = parser.parse_args()

    if not args.passed:
        raise SystemExit(
            "receipt creation requires explicit --pass after completing the real-TTY smoke"
        )
    terminal = args.terminal.strip()
    if not terminal:
        raise SystemExit("--terminal must not be empty")

    observed_at = dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")
    receipt = {
        "schema": SCHEMA,
        "platform": args.platform,
        "status": "pass",
        "terminal": terminal,
        "observedAt": observed_at,
        "notes": args.notes.strip() or None,
    }

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    write_text_lf(output, json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
