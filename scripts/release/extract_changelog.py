#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime as dt
import pathlib
import re

from _compat import write_text_lf


RELEASED_DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
PREPUBLICATION_MARKERS = (
    "### release candidate",
    "not yet declared or published",
    "not yet published",
    "stable-eligible but",
    "stable publication remains fail-closed",
)


def validate_released_section(version: str, heading: str, body: str) -> None:
    prefix = f"## [{version}] - "
    if not heading.startswith(prefix):
        raise SystemExit(
            f"CHANGELOG section for {version} must include a release date before stable publish"
        )

    release_date = heading[len(prefix):].strip()
    if not RELEASED_DATE.fullmatch(release_date):
        raise SystemExit(
            f"CHANGELOG section for {version} is not released: {release_date!r}"
        )
    try:
        parsed = dt.date.fromisoformat(release_date)
    except ValueError as exc:
        raise SystemExit(
            f"CHANGELOG section for {version} has invalid release date: {release_date!r}"
        ) from exc
    if parsed.isoformat() != release_date:
        raise SystemExit(
            f"CHANGELOG section for {version} release date must be YYYY-MM-DD"
        )

    lowered = body.casefold()
    for marker in PREPUBLICATION_MARKERS:
        if marker in lowered:
            raise SystemExit(
                f"CHANGELOG section for {version} still contains pre-publication marker: {marker!r}"
            )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--changelog", default="CHANGELOG.md")
    parser.add_argument("--output", required=True)
    parser.add_argument(
        "--require-released",
        action="store_true",
        help="Require a dated, publication-ready changelog section.",
    )
    args = parser.parse_args()

    text = pathlib.Path(args.changelog).read_text(encoding="utf-8")
    pattern = re.compile(
        rf"^(?P<heading>## \[{re.escape(args.version)}\].*?)$\n(?P<body>.*?)(?=^## \[|\Z)",
        re.MULTILINE | re.DOTALL,
    )
    match = pattern.search(text)
    if not match:
        raise SystemExit(f"CHANGELOG section not found for {args.version}")

    heading = match.group("heading").strip()
    body = match.group("body").strip()
    if not body:
        raise SystemExit(f"CHANGELOG section for {args.version} is empty")
    if args.require_released:
        validate_released_section(args.version, heading, body)

    output = pathlib.Path(args.output)
    write_text_lf(output, f"# codex-tui {args.version}\n\n{body}\n")
    print(f"WROTE {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
