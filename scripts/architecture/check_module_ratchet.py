#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import sys

DEFAULT_PLAN = pathlib.Path("release/v1.2-plan.json")


def main() -> int:
    plan_path = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_PLAN
    plan = json.loads(plan_path.read_text(encoding="utf-8"))
    ratchet = plan.get("moduleRatchet")
    if not isinstance(ratchet, dict) or not ratchet:
        raise SystemExit("v1.2 moduleRatchet is missing")

    failures = []
    rows = []
    for name, ceiling in sorted(ratchet.items()):
        path = pathlib.Path(name)
        if not path.is_file():
            failures.append(f"{name}: tracked ratchet module is missing")
            continue
        if not isinstance(ceiling, int) or ceiling <= 0:
            failures.append(f"{name}: invalid ceiling {ceiling!r}")
            continue
        lines = len(path.read_text(encoding="utf-8").splitlines())
        rows.append((name, lines, ceiling))
        if lines > ceiling:
            failures.append(f"{name}: {lines} LOC exceeds no-growth ceiling {ceiling}")

    for name, lines, ceiling in rows:
        print(f"{name}: {lines}/{ceiling} LOC")

    if failures:
        raise SystemExit("module ratchet failed:\n" + "\n".join(failures))
    print("PASS v1.2 module no-growth ratchet")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
