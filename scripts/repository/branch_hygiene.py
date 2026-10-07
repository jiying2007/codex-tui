#!/usr/bin/env python3
"""Dry-run first, exact-head branch cleanup. Never delete release or unmerged work."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

SCHEMA = "codex-tui/branch-hygiene/v2"
EPHEMERAL_PREFIXES = (
    "feat/",
    "fix/",
    "hardening/",
    "perf/",
    "refactor/",
    "rebase/",
    "chore/",
    "v1.2/",
    "v1.3/",
    "v1.4/",
)

def ephemeral_branch(name: str) -> bool:
    return name.startswith(EPHEMERAL_PREFIXES)


def checked(*args: str) -> str:
    return subprocess.run(args, check=True, capture_output=True, text=True, timeout=120).stdout.strip()

def api(repo: str, path: str):
    return json.loads(checked("gh", "api", "repos/" + repo + "/" + path))

def pages(repo: str, path: str):
    return [item for page in json.loads(checked("gh", "api", "--paginate", "--slurp", "repos/" + repo + "/" + path)) for item in page]

def build_plan(
    repo: str,
    main: str,
    branches: list,
    prs: list,
    ancestor,
    equivalent=None,
    allow_ancestor_only: bool = False,
) -> dict:
    entries = []
    for branch in branches:
        name, sha = branch["name"], branch["commit"]["sha"]
        relevant = [pr for pr in prs if pr["head"]["ref"] == name and (pr["head"].get("repo") or {}).get("full_name") == repo]
        merged = next((pr for pr in relevant if pr.get("merged_at") and pr["head"]["sha"] == sha), None)
        proof = None
        if name in ("main", "master", "develop") or name.startswith(("release/", "archive/", "checkpoint/")) or branch.get("protected"):
            reason = "protected-or-retained-reference"
        elif any(pr["state"] == "open" for pr in relevant):
            reason = "active-pull-request"
        elif not re.fullmatch(r"[0-9a-f]{40}", sha):
            reason = "invalid-head"
        else:
            reachable = ancestor(sha, main)
            if merged and reachable:
                reason = "exact-merged-head-reachable-from-main"
            elif allow_ancestor_only and reachable and ephemeral_branch(name):
                reason = "ancestor-only-no-unique-commits"
            elif not merged:
                reason = "no-exact-merged-pull-request"
            else:
                proof = equivalent(sha, merged.get("merge_commit_sha"), main) if equivalent else None
                reason = "exact-merged-squash-tree-delta" if proof else "head-not-reachable-from-main"
        delete_reasons = (
            "exact-merged-head-reachable-from-main",
            "exact-merged-squash-tree-delta",
            "ancestor-only-no-unique-commits",
        )
        entry = {"branch": name, "sha": sha, "decision": "delete" if reason in delete_reasons else "keep", "reason": reason, "mergedPr": merged["number"] if merged else None}
        if proof:
            entry["equivalenceProof"] = proof
        entries.append(entry)
    return {
        "schema": SCHEMA,
        "repository": repo,
        "mainSha": main,
        "policy": {
            "ancestorOnlyEnabled": allow_ancestor_only,
            "ancestorOnlyPrefixes": list(EPHEMERAL_PREFIXES),
        },
        "entries": entries,
    }

def is_ancestor(head: str, main: str) -> bool:
    code = subprocess.run(["git", "merge-base", "--is-ancestor", head, main], capture_output=True, timeout=30).returncode
    if code not in (0, 1):
        raise RuntimeError("cannot verify commit ancestry; fetch full history first")
    return code == 0

def squash_equivalence(head: str, merge: str, main: str):
    """Prove identical file/mode/blob deltas, not whitespace-insensitive patch IDs.

    The exact PR head must already match. A one-parent merge commit reachable
    from main may be a squash; require its complete raw tree delta to equal the
    branch delta from the common ancestor. Changed/deleted/renamed/binary files
    and mode changes are all covered, and empty diffs never authorize deletion.
    """
    if not merge or not re.fullmatch(r"[0-9a-f]{40}", merge) or not is_ancestor(merge, main):
        return None
    parents = checked("git", "rev-list", "--parents", "-n", "1", merge).split()[1:]
    if len(parents) != 1:
        return None
    base = checked("git", "merge-base", head, parents[0])
    def delta(before, after):
        return subprocess.run(["git", "diff", "--raw", "--no-abbrev", "--no-renames", "-z", before, after], check=True, capture_output=True, timeout=30).stdout
    branch_delta = delta(base, head)
    if not branch_delta or branch_delta != delta(parents[0], merge):
        return None
    return {"method": "identical-raw-tree-delta-v1", "mergeCommit": merge, "branchBase": base,
            "mergeParent": parents[0], "deltaSha256": hashlib.sha256(branch_delta).hexdigest()}

def scoped_branches(branches: list, name: str, expected_sha: str) -> list:
    if not name and not expected_sha:
        return branches
    if not name or not re.fullmatch(r"[0-9a-f]{40}", expected_sha or ""):
        raise ValueError("branch scope requires an exact expected HEAD")
    selected = [branch for branch in branches if branch["name"] == name]
    if any(branch["commit"]["sha"] != expected_sha for branch in selected):
        raise ValueError("scoped branch advanced; refusing cleanup")
    return selected  # Already absent is a safe, verified no-op.

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--only-branch")
    parser.add_argument("--expected-head")
    parser.add_argument("--apply", action="store_true")
    parser.add_argument(
        "--allow-ancestor-only",
        action="store_true",
        help="full backlog only: allow ephemeral branch deletion when exact HEAD is already an ancestor of main",
    )
    parser.add_argument("--plan-sha256")
    parser.add_argument("--confirm-repository")
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo):
        parser.error("expected owner/name repository")
    if args.allow_ancestor_only and (args.only_branch or args.expected_head):
        parser.error("--allow-ancestor-only is forbidden for scoped merged-branch cleanup")
    head = api(args.repo, "git/ref/heads/main")["object"]["sha"]
    branches = scoped_branches(pages(args.repo, "branches?per_page=100"), args.only_branch, args.expected_head)
    prs = pages(args.repo, "pulls?state=all&per_page=100")
    plan = build_plan(
        args.repo,
        head,
        branches,
        prs,
        is_ancestor,
        squash_equivalence,
        allow_ancestor_only=args.allow_ancestor_only,
    )
    encoded = (json.dumps(plan, indent=2, sort_keys=True) + "\n").encode()
    digest = hashlib.sha256(encoded).hexdigest()
    if not args.apply:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(encoded)
        print("DRY RUN %s sha256=%s candidates=%d" % (args.output, digest, sum(e["decision"] == "delete" for e in plan["entries"])))
        return 0
    if args.confirm_repository != args.repo or args.plan_sha256 != digest:
        parser.error("fresh plan changed or exact repository/digest confirmation missing; rerun dry-run")
    if not args.output.is_file() or hashlib.sha256(args.output.read_bytes()).hexdigest() != digest:
        parser.error("retained plan bytes do not match the freshly verified plan")
    origin = checked("git", "remote", "get-url", "--push", "origin")
    if origin not in ("https://github.com/" + args.repo, "https://github.com/" + args.repo + ".git", "git@github.com:" + args.repo + ".git"):
        parser.error("push origin does not match confirmed repository")
    if api(args.repo, "git/ref/heads/main")["object"]["sha"] != head:
        parser.error("main moved during verification")
    entries = [e for e in plan["entries"] if e["decision"] == "delete"]
    if entries:
        command = ["git", "push", "--atomic"]
        command += ["--force-with-lease=refs/heads/%s:%s" % (e["branch"], e["sha"]) for e in entries]
        command += ["origin"] + [":refs/heads/" + e["branch"] for e in entries]
        checked(*command)
    remaining = {b["name"] for b in pages(args.repo, "branches?per_page=100")}
    if any(e["branch"] in remaining for e in entries):
        raise RuntimeError("cleanup verification failed")
    receipt = dict(plan, applied=True, deletedCount=len(entries), remainingBranches=sorted(remaining))
    args.output.with_suffix(".receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("VERIFIED deleted=%d; retained=%d" % (len(entries), len(remaining)))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
