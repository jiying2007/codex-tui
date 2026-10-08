#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import sys

DEFAULT_PLAN = pathlib.Path("release/v1.4-plan.json")

def inspect_plan(plan_path: pathlib.Path, root: pathlib.Path = pathlib.Path(".")):
    full_plan = root / plan_path
    plan = json.loads(full_plan.read_text(encoding="utf-8"))
    ratchet = plan.get("moduleRatchet")
    if not isinstance(ratchet, dict) or not ratchet:
        raise ValueError(f"{plan_path} moduleRatchet is missing")

    failures = []
    rows = []
    for name, ceiling in sorted(ratchet.items()):
        path = root / name
        if not path.is_file():
            failures.append(f"{name}: tracked ratchet module is missing")
            continue
        if not isinstance(ceiling, int) or isinstance(ceiling, bool) or ceiling <= 0:
            failures.append(f"{name}: invalid ceiling {ceiling!r}")
            continue
        lines = len(path.read_text(encoding="utf-8").splitlines())
        rows.append((name, lines, ceiling))
        if lines > ceiling:
            failures.append(f"{name}: {lines} LOC exceeds no-growth ceiling {ceiling}")

    policy = plan.get("ratchetPolicy")
    complete = isinstance(policy, dict) and policy.get("completeSourceCoverage") is True
    if complete:
        source_root = root / "src"
        if not source_root.is_dir():
            failures.append("src: complete source coverage requested but source root is missing")
        else:
            actual = {
                path.relative_to(root).as_posix()
                for path in source_root.rglob("*.rs")
                if path.is_file()
            }
            tracked = {name for name in ratchet if name.startswith("src/")}
            for name in sorted(actual - tracked):
                failures.append(f"{name}: Rust source module is not governed by moduleRatchet")
            for name in sorted(tracked - actual):
                # The ordinary tracked-file check reports the detailed missing-file failure.
                if not (root / name).is_file():
                    continue
                failures.append(f"{name}: ratchet source coverage identity mismatch")

    return rows, failures, complete


def main() -> int:
    plan_path = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_PLAN
    try:
        rows, failures, complete = inspect_plan(plan_path)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        raise SystemExit(str(error)) from error

    for name, lines, ceiling in rows:
        print(f"{name}: {lines}/{ceiling} LOC")

    if failures:
        raise SystemExit("module ratchet failed:\n" + "\n".join(failures))
    coverage = " complete-src-coverage" if complete else ""
    print(f"PASS {plan_path} module no-growth ratchet{coverage}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
