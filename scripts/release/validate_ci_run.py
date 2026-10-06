#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-json", required=True)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()

    run = json.loads(pathlib.Path(args.run_json).read_text(encoding="utf-8"))
    checks = {
        "workflow name": run.get("name") == "ci",
        "workflow path": run.get("path") == ".github/workflows/ci.yml",
        "workflow event": run.get("event") == "push",
        "status": run.get("status") == "completed",
        "conclusion": run.get("conclusion") == "success",
        "head SHA": str(run.get("head_sha", "")).lower() == args.commit.lower(),
        "head branch": run.get("head_branch") == "main",
    }
    failed = [name for name, ok in checks.items() if not ok]
    if failed:
        raise SystemExit("canonical CI run mismatch: " + ", ".join(failed))
    print(
        f'VALID canonical CI run {run.get("id")} for {run.get("head_sha")} '
        f'({run.get("conclusion")})'
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
