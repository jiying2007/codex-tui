#!/usr/bin/env python3
import argparse
import pathlib
import re


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--changelog", default="CHANGELOG.md")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    text = pathlib.Path(args.changelog).read_text(encoding="utf-8")
    pattern = re.compile(
        rf"^## \[{re.escape(args.version)}\].*?$\n(?P<body>.*?)(?=^## \[|\Z)",
        re.MULTILINE | re.DOTALL,
    )
    match = pattern.search(text)
    if not match:
        raise SystemExit(f"CHANGELOG section not found for {args.version}")

    body = match.group("body").strip()
    if not body:
        raise SystemExit(f"CHANGELOG section for {args.version} is empty")

    output = pathlib.Path(args.output)
    output.write_text(
        f"# codex-tui {args.version}\n\n{body}\n",
        encoding="utf-8",
        newline="\n",
    )
    print(f"WROTE {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
