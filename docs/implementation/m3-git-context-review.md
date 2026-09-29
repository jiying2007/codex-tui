# M3 Git context and review

Date: 2026-09-29
Status: Implemented

M3 adds the personal engineering workspace above Codex without moving Git authority into codex-tui.

## Authority boundary

Git CLI output remains authoritative for:

- repository identity;
- worktree identity;
- HEAD/branch/upstream;
- index and working-tree state;
- changed files;
- repository diff.

codex-tui keeps only an in-memory projection keyed by exact Codex thread ID.

No Git mutation is introduced in M3.

## Git projection

A dedicated Tokio Git actor executes bounded read-only subprocesses.

Normal projection uses:

- `git rev-parse --path-format=absolute --show-toplevel --git-common-dir`;
- `git status --porcelain=v2 -z --branch --untracked-files=all`.

Every command has an explicit three-second timeout and `kill_on_drop`.

The parser preserves:

- branch or detached HEAD;
- upstream;
- ahead/behind;
- staged state;
- unstaged state;
- untracked files;
- renames with original path;
- conflicts.

Non-Git cwd and Git command failures are distinct degraded states.

## Identity

`LocalRepoIdentity` and `WorktreeIdentity` remain separate.

Local repository identity is based on the canonical Git common directory and primary repository root.

Worktree identity adds the canonical worktree top-level and branch.

Real integration tests create a Git repository and a linked worktree on CI and assert:

- both worktrees share one `LocalRepoIdentity`;
- each has a distinct `WorktreeIdentity`;
- branch projection remains correct.

This test runs on Linux, macOS and Windows.

## Asynchronous runtime integration

Git probing never blocks the first UI frame.

`RefreshGitProjections` emits a probe only when:

- a thread has no Git projection; or
- the thread cwd changed.

A pending projection is inserted immediately so periodic Codex registry refreshes do not spawn repeated Git commands.

Git projection is memory-only and is discarded when the thread leaves the registry.

## Shared mutable checkout warning

Collision state is derived, not persisted.

A collision exists when another active `WORKING` or `WAITING` Codex thread projects to the same:

- `LocalRepoIdentity`; and
- canonical worktree path.

Mission Control shows a warning marker and Context/Workspace surfaces explain the collision.

M3 only warns. Managed worktree creation and mutation locks remain M5.

## Workspace view

`w` opens a read-only Workspace view from Registry or Thread.

It presents:

- repository root;
- Git common directory;
- worktree path;
- branch or detached HEAD;
- upstream;
- ahead/behind;
- dirty state;
- changed-file status/path;
- shared-worktree warning.

Workspace performs no shell workflow or Git mutation.

`r` transitions to Review.

## Review

`r` opens Review for the exact selected/current thread.

The Git actor retrieves exactly two authoritative repository diffs:

- staged: `git diff --cached --no-ext-diff --no-color --unified=3 --`;
- unstaged: `git diff --no-ext-diff --no-color --unified=3 --`.

Changed files are sourced from porcelain v2.

Review supports:

- `j/k` changed-file selection;
- PageUp/PageDown diff scrolling;
- `w` presentation word-diff toggle;
- `e` external editor;
- Esc/back to the originating Registry/Thread/Workspace view.

Untracked files remain visible in the changed-file list even though ordinary `git diff` has no content diff for them.

## Presentation diff

The `similar` crate is used only after Git has produced canonical diff text.

When word-diff is enabled, adjacent ordinary deletion/addition lines are annotated using word-level differences.

It never replaces Git's repository diff semantics.

## Bounded Review output

Review stdout is bounded while it is being read, not after full buffering:

- each diff retains at most 2 MiB;
- stderr retains at most 64 KiB;
- pipes continue to drain so the child cannot deadlock on a full pipe;
- the whole subprocess remains under the Git timeout;
- the UI marks truncated output.

Non-UTF8 bytes are retained in the bounded byte capture and converted with lossy display semantics only at the presentation boundary.

## External editor

`e` is an explicit user action.

Editor selection:

1. `CODEX_TUI_EDITOR`;
2. fallback `code`.

The selected changed path is joined to the thread cwd and launched as a separate process with stdio detached from the TUI.

codex-tui does not embed a PTY editor in M3.

## Diagnostics

`codex-tui doctor git` probes the current directory and reports:

- repository presence;
- cwd;
- Git common directory;
- repository root;
- worktree;
- branch/detached state;
- dirty state;
- changed-file count;
- error/degraded state.

## Verification

Cross-platform CI runs:

- `cargo fmt --check`;
- Clippy with warnings denied;
- all-target/all-feature tests.

Coverage includes:

- porcelain-v2 machine parsing;
- detached HEAD;
- rename identity;
- real dirty repository;
- real linked worktree identity;
- Git probe coalescing;
- shared-worktree collision derivation;
- Workspace navigation;
- Review navigation;
- word-diff presentation;
- bounded Review capture;
- existing M0-M2 regression suite.

## Next slice

M4 introduces personal planning and the first justified relational local store:

- SQLite migration from `state-v1.json`;
- WorkCard anchor/link model;
- workflow derivation;
- provenance/freshness;
- Codex Goal projection;
- Saved Views;
- List/Board;
- ScratchWork;
- notes/bookmarks;
- snooze/unread;
- hot slots.
