#!/usr/bin/env python3
import argparse
import hashlib
import pathlib


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("directory")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    root = pathlib.Path(args.directory)
    output = pathlib.Path(args.output)
    files = sorted(
        path for path in root.iterdir()
        if path.is_file() and path.resolve() != output.resolve()
    )
    if not files:
        raise SystemExit("no files found for checksum manifest")

    lines = [f"{sha256(path)}  {path.name}" for path in files]
    output.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"WROTE {output} ({len(files)} files)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
