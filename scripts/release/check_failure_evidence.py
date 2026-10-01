#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify every retained Failure Matrix evidence identifier still exists."
    )
    parser.add_argument("--matrix", required=True)
    parser.add_argument("--test-list", required=True)
    args = parser.parse_args()

    matrix = json.loads(pathlib.Path(args.matrix).read_text(encoding="utf-8"))
    if matrix.get("schema") != "codex-tui/failure-matrix/v1":
        raise SystemExit("unexpected Failure Matrix schema")

    cases = matrix.get("cases")
    if not isinstance(cases, list) or not cases:
        raise SystemExit("Failure Matrix has no cases")

    test_names = set()
    for raw in pathlib.Path(args.test_list).read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line.endswith(": test"):
            test_names.add(line[: -len(": test")])

    if not test_names:
        raise SystemExit("cargo test list contained no test identifiers")

    case_ids = set()
    missing = []
    for case in cases:
        case_id = str(case.get("id", "")).strip()
        if not case_id:
            raise SystemExit("Failure Matrix case has no id")
        if case_id in case_ids:
            raise SystemExit(f"duplicate Failure Matrix case id: {case_id}")
        case_ids.add(case_id)

        evidence = case.get("evidence")
        if not isinstance(evidence, list) or not evidence:
            raise SystemExit(f"Failure Matrix case {case_id} has no evidence")
        for identifier in evidence:
            identifier = str(identifier).strip()
            if not identifier:
                raise SystemExit(f"Failure Matrix case {case_id} has empty evidence")
            if identifier not in test_names:
                missing.append((case_id, identifier))

    if missing:
        details = "\n".join(f"  {case}: {identifier}" for case, identifier in missing)
        raise SystemExit("retained Failure Matrix evidence is missing from cargo test -- --list:\n" + details)

    print(
        f"VERIFIED {len(cases)} Failure Matrix cases against "
        f"{len(test_names)} executable test identifiers"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
