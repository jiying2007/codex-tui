#!/usr/bin/env python3
"""Validate active bilingual Markdown with Python 3.8 standard library only."""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit

ROOT = pathlib.Path(__file__).resolve().parents[2]
MANIFEST = "docs/i18n/manifest.json"
SCHEMA = "codex-tui/bilingual-docs/v1"
ID = re.compile(r"(?m)^<!-- docs-id: ([a-z0-9-]+) -->$")
LANG = re.compile(r"(?m)^<!-- docs-lang: (en|zh-CN) -->$")
SECTION = re.compile(r"(?m)^<!-- docs-section: ([a-z0-9-]+) -->$")
LINK = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
CJK = re.compile(r"[\u4e00-\u9fff]")


def read(root, relpath, overrides):
    if relpath in overrides:
        return overrides[relpath]
    return (root / relpath).read_text(encoding="utf-8")


def local_links(markdown):
    fenced = False
    for line in markdown.splitlines():
        stripped = line.lstrip()
        if stripped.startswith(("~~~", "```")):
            fenced = not fenced
            continue
        if fenced:
            continue
        for match in LINK.finditer(line):
            raw = match.group(1).strip().strip("<>")
            if raw.startswith(("http:", "https:", "mailto:", "tel:", "app:")):
                continue
            raw = raw.split(' "', 1)[0]
            parsed = urlsplit(raw)
            if not parsed.scheme and not parsed.netloc and parsed.path:
                yield unquote(parsed.path)


def inspect(root=ROOT, overrides=None):
    root = pathlib.Path(root).resolve()
    overrides = overrides or {}
    errors = []
    try:
        manifest = json.loads(read(root, MANIFEST, overrides))
    except (OSError, UnicodeError, ValueError) as error:
        return ["manifest unreadable: " + type(error).__name__]
    if manifest.get("schema") != SCHEMA:
        errors.append("manifest schema drift")
    pairs = manifest.get("pairs")
    if not isinstance(pairs, list) or not pairs:
        return errors + ["active pairs must be nonempty"]
    ids, paths, chinese = set(), set(), set()
    for entry in pairs:
        if not isinstance(entry, dict):
            errors.append("malformed pair record")
            continue
        docid = entry.get("id")
        if not isinstance(docid, str) or not re.fullmatch("[a-z0-9-]+", docid):
            errors.append("invalid docs-id")
            continue
        if docid in ids:
            errors.append("duplicate docs-id: " + docid)
        ids.add(docid)
        topics = entry.get("sections")
        if not isinstance(topics, list) or "overview" not in topics or (
            len(topics) != len(set(map(str, topics)))
        ) or any(not isinstance(t, str) or
                 re.fullmatch("[a-z0-9-]+", t) is None for t in topics):
            errors.append(docid + ": invalid semantic sections")
            continue
        english, translated = entry.get("en"), entry.get("zh-CN")
        if not all(isinstance(p, str) and p.endswith(".md")
                   for p in (english, translated)):
            errors.append(docid + ": EN/zh-CN path invalid")
            continue
        if english == translated:
            errors.append(docid + ": shared language source forbidden")
        chinese.add(translated)
        for path in (english, translated):
            if path in paths:
                errors.append(docid + ": duplicate active path " + path)
            paths.add(path)

        for locale, name, peer in (("en", english, translated),
                                   ("zh-CN", translated, english)):
            absolute = (root / name).resolve()
            try:
                absolute.relative_to(root)
            except ValueError:
                errors.append(name + ": path escapes repository")
                continue
            try:
                content = read(root, name, overrides)
            except (OSError, UnicodeError):
                errors.append(name + ": missing or invalid UTF-8")
                continue
            if "\r" in content:
                errors.append(name + ": CRLF not supported, use UTF-8 LF")
            if len(content.strip()) < 200 or not re.search(r"(?m)^# [^\n]+$", content):
                errors.append(name + ": empty or missing title")
            if ID.findall(content) != [docid]:
                errors.append(name + ": docs-id mismatch")
            if LANG.findall(content) != [locale]:
                errors.append(name + ": locale mismatch")
            if locale == "zh-CN" and not CJK.search(content):
                errors.append(name + ": no Chinese prose")
            sections = SECTION.findall(content)
            if len(sections) != len(set(sections)) or set(sections) != set(topics):
                errors.append(name + ": semantic sections drift")
            for item in SECTION.finditer(content):
                following = SECTION.search(content, item.end())
                body = content[item.end():following.start() if following else len(content)]
                if len("".join(body.split())) < 35:
                    errors.append(name + ": empty section " + item.group(1))

            found = []
            for target in local_links(content):
                found.append(target)
                dest = (absolute.parent / target).resolve()
                try:
                    dest.relative_to(root)
                except ValueError:
                    errors.append(name + ": local link escapes repository: " + target)
                    continue
                if not dest.is_file():
                    errors.append(name + ": broken local link: " + target)
            expected = os.path.relpath(root / peer, absolute.parent).replace(os.sep, "/")
            if expected not in found:
                errors.append(name + ": missing reciprocal language link: " + expected)

    observed = set()
    for file in root.glob("*.zh-CN.md"):
        observed.add(file.relative_to(root).as_posix())
    for file in (root / "docs").glob("*.zh-CN.md"):
        observed.add(file.relative_to(root).as_posix())
    for directory in (root / "docs/zh-CN", root / "docs/i18n"):
        if directory.is_dir():
            for file in directory.rglob("*.md"):
                if directory.name == "i18n" and not file.name.endswith(".zh-CN.md"):
                    continue
                observed.add(file.relative_to(root).as_posix())
    for file in sorted(observed - chinese):
        errors.append("unpaired current Chinese document: " + file)
    return errors



def changed_pair_issues(manifest, changed):
    """Require both active language files in a single PR/main change."""
    violations = []
    if not isinstance(manifest, dict):
        return ["cannot inspect bilingual pair changes: invalid manifest"]
    entries = manifest.get("pairs")
    if not isinstance(entries, list):
        return ["cannot inspect bilingual pair changes: missing pairs"]
    changed = set(changed)
    for pair in entries:
        if not isinstance(pair, dict):
            violations.append("cannot inspect bilingual pair changes: invalid pair")
            continue
        en, zh = pair.get("en"), pair.get("zh-CN")
        if not isinstance(en, str) or not isinstance(zh, str):
            violations.append("cannot inspect bilingual pair changes: invalid paths")
            continue
        if (en in changed) != (zh in changed):
            violations.append(
                "only one language of active pair changed: " + str(pair.get("id", "unknown"))
            )
    return violations


def changed_paths(root, base_ref):
    """Read a shallow, pinned checkout's first-parent diff without a network API."""
    try:
        result = subprocess.run(
            ["git", "diff", "--name-only", "-z", "--diff-filter=ACMRTD",
             base_ref, "HEAD", "--"],
            cwd=str(root), stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            timeout=15, check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        raise ValueError("cannot inspect changed Git paths")
    if result.returncode != 0:
        raise ValueError("cannot inspect changed Git paths: missing checkout parent")
    try:
        return set(value.decode("utf-8") for value in result.stdout.split(b"\0") if value)
    except UnicodeDecodeError:
        raise ValueError("changed Git paths are not valid UTF-8")


def main():
    parser = argparse.ArgumentParser(description="Validate EN/zh-CN active docs")
    parser.add_argument("--root", default=str(ROOT))
    parser.add_argument("--changed-base", default=None,
                        help="check that each modified active pair has both languages")
    args = parser.parse_args()
    root = pathlib.Path(args.root)
    failures = inspect(root)
    if args.changed_base:
        try:
            pairs = json.loads((root / MANIFEST).read_text(encoding="utf-8"))
            files = changed_paths(root, args.changed_base)
            failures.extend(changed_pair_issues(pairs, files))
        except (OSError, UnicodeError, ValueError):
            failures.append("cannot validate paired Git changes")
    if failures:
        for error in failures:
            print("DOC FAIL: " + error, file=sys.stderr)
        return 1
    print("PASS bilingual docs: paired chapters, locales, navigation and local links")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
