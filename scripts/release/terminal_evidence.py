"""Retain terminal observations without replacing earlier evidence."""
from __future__ import annotations

import pathlib

from _compat import require_python38

require_python38()


def write_new_terminal_evidence(path: pathlib.Path, text: str) -> None:
    # Do not resolve the leaf: exclusive creation must reject dangling links too.
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("x", encoding="utf-8", newline="\n") as handle:
            handle.write(text)
    except FileExistsError:
        raise SystemExit(
            "refusing to overwrite terminal evidence; choose a fresh output path"
        ) from None
    except OSError:
        raise SystemExit("cannot retain terminal evidence (details redacted)") from None
