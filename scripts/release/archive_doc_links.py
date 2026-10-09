#!/usr/bin/env python3
"""Keep bundled Markdown navigable without copying historical repository trees.

Current bilingual guides remain offline. Links to tracked-but-unbundled
historical material become immutable GitHub links bound to the source SHA.
Both rewriting and verification are independent of a network connection.
"""
from __future__ import annotations

import os
import pathlib
import re
from urllib.parse import quote, unquote, urlsplit

from _compat import write_text_lf

GITHUB_SOURCE = "https://github.com/jiying2007/codex-tui/blob/"
HEX40 = re.compile(r"^[0-9a-fA-F]{40}$")
LINK = re.compile(r"(!?\[[^\]\n]*\]\()([^\n)]+)(\))")
EXTERNAL_SCHEMES = frozenset(("http", "https", "mailto", "tel", "app"))
ALIASES = {
    "INSTALL-UPGRADE.md": "docs/release/install-upgrade.md",
    "TEAM-QUICKSTART.md": "docs/team-quickstart.md",
    "INSTALL-UPGRADE.zh-CN.md": "docs/zh-CN/release/install-upgrade.md",
    "TEAM-QUICKSTART.zh-CN.md": "docs/zh-CN/team-quickstart.md",
}


def markdown_lines(content):
    """Yield all lines and flag whether each belongs to a fenced code block."""
    fenced = None
    for line in content.splitlines(keepends=True):
        token = line.lstrip()
        if fenced is None and token.startswith((chr(96) * 3, "~" * 3)):
            fenced = token[:3]
            yield line, True
            continue
        if fenced is not None:
            yield line, True
            if token.startswith(fenced):
                fenced = None
            continue
        yield line, False


def parse_link(value):
    raw = value.strip().strip("<>")
    # Unlike URLs, local documentation links must have no query strings.
    # Keep anchors (including empty-path anchors) intact.
    parsed = urlsplit(raw)
    if parsed.scheme:
        if parsed.scheme.lower() in EXTERNAL_SCHEMES:
            return None, None
        raise ValueError("unsupported Markdown link scheme (redacted)")
    if parsed.netloc or raw.startswith("//") or parsed.query:
        raise ValueError("unsafe or ambiguous Markdown link (redacted)")
    if not parsed.path:
        return None, None
    if parsed.path.startswith("/") or "\\" in parsed.path:
        raise ValueError("absolute/backslash Markdown link forbidden")
    return unquote(parsed.path), ("#" + parsed.fragment if parsed.fragment else "")


def inside(root, relpath, label):
    destination = (root / relpath).resolve()
    try:
        return destination, destination.relative_to(root.resolve()).as_posix()
    except ValueError:
        raise ValueError(label + " escapes documentation root")


def rewrite_packaged_links(stage, checkout, source_sha):
    """Rewrite only absent bundled targets; never fabricate a source link."""
    stage = pathlib.Path(stage).resolve()
    checkout = pathlib.Path(checkout).resolve()
    if not HEX40.fullmatch(source_sha):
        raise ValueError("source SHA must be exactly 40 hex characters")
    for document in sorted(stage.rglob("*.md")):
        bundled = document.relative_to(stage).as_posix()
        origin = ALIASES.get(bundled, bundled)
        result = []
        for line, fenced in markdown_lines(document.read_text(encoding="utf-8")):
            if fenced:
                result.append(line)
                continue

            def convert(match):
                target, fragment = parse_link(match.group(2))
                if target is None:
                    return match.group(0)
                original, original_rel = inside(
                    checkout, pathlib.Path(origin).parent / target, "source link"
                )
                if not original.is_file():
                    raise ValueError("Markdown references missing source file")
                bundled_target, _ = inside(stage, original_rel, "archive link")
                if bundled_target.is_file():
                    new_target = os.path.relpath(
                        bundled_target, document.parent
                    ).replace(os.sep, "/")
                else:
                    new_target = GITHUB_SOURCE + source_sha.lower() + "/" + quote(
                        original_rel, safe="/._-~"
                    )
                return match.group(1) + new_target + fragment + match.group(3)

            result.append(LINK.sub(convert, line))
        updated = "".join(result)
        if updated != document.read_text(encoding="utf-8"):
            write_text_lf(document, updated)


def verify_packaged_links(stage):
    """Fail closed when a v1.4+ release archive contains dead local links."""
    stage = pathlib.Path(stage).resolve()
    for document in sorted(stage.rglob("*.md")):
        for line, fenced in markdown_lines(document.read_text(encoding="utf-8")):
            if fenced:
                continue
            for match in LINK.finditer(line):
                target, _ = parse_link(match.group(2))
                if target is None:
                    continue
                file, _ = inside(stage, document.relative_to(stage).parent / target,
                                 "archive link")
                if not file.is_file():
                    raise ValueError("release archive has a broken local Markdown link")
