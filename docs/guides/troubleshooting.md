<!-- docs-id: troubleshooting -->
<!-- docs-lang: en -->
# Troubleshooting: Codex, Forge, SSH and SQLite
<!-- docs-section: overview -->

**Language / 语言:** [English](troubleshooting.md) · [简体中文](../zh-CN/guides/troubleshooting.md)

Diagnose the failed authority first; avoid deleting local state, changing upstream credentials or inventing successful recovery. Keep the exact source SHA, target type and relevant sanitized error category. For disclosure-sensitive cases see [SECURITY](../../SECURITY.md).

## App Server will not connect or load threads
<!-- docs-section: codex -->

Run `codex-tui doctor codex` (optionally `--target NAME`). Check the real `codex --version` and its login/account independently. Named remote WebSocket/Unix targets have bounded connect/handshake timeouts; a timeout is not proof an action never executed. Verify process/transport, authentication, TLS proxy and App Server capability errors. Older optional methods may degrade; no real-to-fake fallback exists. Avoid pasting bearer tokens into an issue.

Stdio fragments remain buffered across timer/command cancellation; a backend EOF delivers the final disconnected snapshot once. Repeated/cyclic cursors, stalled pagination and total budgets produce explicit incomplete inventory, never complete mutation authority. Search jumps locate the exact item within a bounded global-history scan and retain a continuation cursor of the same scope; deleted targets or exhausted budgets report failure instead of silently jumping elsewhere.

RPC notifications retain at most 1024 messages and 16 MiB of accounted payload. Pending interactive requests are limited to 64, 256 KiB each and 4 MiB in aggregate. Pagination allows up to 1024 pages, 200,000 identities, 32 MiB of accounted identity storage and 120 seconds across the scan. These are payload/accounting limits, not certified process RSS. Overload terminates/quarantines the session visibly; it does not discard arbitrary approval messages or replay write RPCs. An ordinary 50k-row scan remains supported within these limits.

A cancelled or failed outbound write quarantines the connection: partial delivery is unknown, so later requests cannot append to an unfinished frame and writes are never automatically replayed. Every outbound write also has a five-second deadline, including notifications. Wire read/decode failure or EOF during an RPC cannot be misreported as a healthy connection. A valid optional-method RPC rejection and cancellation while merely waiting for a response do not poison an otherwise intact wire.

Connection status covers the entire RPC session, not just its wire: either a retained-message admission failure or a transport failure makes it unusable and disconnected. A quarantined Registry bootstrap skips optional loaded-list probes and preserves the original fault. A valid optional-method rejection still reports a connected but degraded session.

Each stdio connection owns its stderr drain in a task set containing at most one task. Dropping the connection cancels the drain after failed initialization, completed isolated search, or cancellation; a descendant retaining the write end cannot keep an unowned reader alive. Cancellation completes when Tokio next schedules the task, rather than synchronously in the destructor. This does not terminate unrelated descendants or retain their stderr content.

## GitLab/GitHub unavailable or missing data
<!-- docs-section: forge -->

In the repository at issue: inspect `git remote -v` **privately**, run `codex-tui doctor git` then `codex-tui doctor forge`. Sign in `glab` or `gh` to the correct host outside the tool. GitLab Issues/MRs/Pipelines probe independently; a single unavailable capability is not entire project failure. `doctor compat` Ready does not prove internal GitLab. Use [provider evidence](../qualification/provider.md) for a required internal rollout.

Malformed GitLab approval-state JSON or missing `rules`, `approvals_required` or `approved` fields are unknown evidence and block client-side merging. An explicit valid empty rules array is distinct from a missing array. These checks do not replace GitLab server protections.

Forge stdout/stderr readers share the command future and its existing five-second deadline. Timeout or caller cancellation drops both local pipe readers, including when an unrelated descendant retains its write ends; no detached reader tasks survive that scope. This does not authorize killing unrelated process groups or retrying writes with unknown outcomes.

## Windows paths or repository-backed threads look wrong
<!-- docs-section: cwd -->

On Windows→SSH Ubuntu the TUI reads **Ubuntu-side** Codex Home, shell cwd and Git repository identity. A Windows cwd embedded in a stored Codex thread is foreign provenance, not an Ubuntu checkout. Confirm `doctor codex` reported paths and `doctor git` in the actual checkout. Do not concatenate a Windows absolute path onto an Ubuntu project path or silently invent a local thread.

Git path identity preserves trailing spaces, tabs and embedded newlines: only the single response terminator is removed. Paths and status identities that are not valid UTF-8, cannot be resolved, or no longer match the probed cwd are rejected rather than rewritten. The editor checks the real cwd against the observed worktree root again before resolving the selected relative file. This is not an atomic defense against concurrent same-user filesystem replacement.

Git identity stdout is limited to 64 KiB, status stdout to 2 MiB, and stderr to 64 KiB per command, under the existing three-second deadline. Status parsing permits at most 8192 nonempty records including headers and rename-source records. Over-limit or incomplete metadata/status produces an error and cannot be treated as a clean repository or a complete review. Diff presentation retains its existing bounded, explicitly marked prefix behavior. These are per-operation budgets, not a certified process-RSS ceiling.

Same-cwd thread projections and planning snapshots share an immutable file-change inventory. They do not deep-copy every file entry for each thread. An explicitly loaded Review owns its presentation copy; independent probes and distinct worktrees are not globally interned. The 2 MiB/8192 limits intentionally degrade very large dirty inventories and require qualification on representative workloads.

## Drawer focus, Ctrl+C, resize and rendering
<!-- docs-section: terminal -->

Try the in-app `?` keymap: `t` opens the Drawer and `F6` (or `Ctrl+]`) returns focus if supported. Remote terminal/SSH clients may intercept key combinations. Run `codex-tui doctor terminal` and check TERM, TTY, resizing and direct child exit. Real controlling-TTY focus/resize/Ctrl+C/restore smoke is required for Stable and is not replaced by hosted PTY tests.

PTY children remain owned by a kill-and-reap guard through initialization and the detached actor lifetime. Failure to obtain the reader/writer, publish Ready or start the reader cannot abandon that ownership. Fault-injection tests exercise each startup stage using local children, not real SSH acceptance.

Catchable Unix process signals (SIGTERM, SIGHUP and SIGINT) enter the ordinary interactive shutdown path. Raw terminal mode, alternate screen and cursor are restored before the final bounded state save; a save failure remains an error. Raw Ctrl+C input and OS SIGINT are separate cases. SIGKILL and a disconnected physical terminal cannot be made restorable by an application. These handlers are not installed for headless subcommands.

Scoped Forge readers keep their bounded 8KiB buffers on the heap rather than multiplying inline buffers through nested async state. Compact-future checks and actual native Windows debug CLI tests cover startup stack overflow; boxing only the interactive entry future was not sufficient. Help, version and invalid usage remain free of TTY, backend and state writes, without larger thread stacks.

## Board updates under a busy Registry
<!-- docs-section: board -->

The planning worker keeps one metadata snapshot in flight. If only a small thread-source delta arrived during computation, it reprojects at most 64 affected cards (including membership and shared-worktree collision effects) from **current** inputs before installing the result. It never simply accepts an old generation. Operator edits, changed local/Forge/Git/goal/backend authority or a larger delta still require a fresh full snapshot. This prevents one noisy thread from indefinitely discarding an otherwise usable result; it is not an absolute latency guarantee under arbitrary global churn. Retained 5k continuous-event fixtures and performance diagnostics are synthetic evidence, not human productivity or multi-hour real-use certification.

## SQLite state damaged or forward schema rejected
<!-- docs-section: store -->

Run `codex-tui doctor store` for an explicit SQLite integrity scan. The interactive application also runs integrity screening once on startup; ordinary state/index reads and writes no longer repeat a full quick_check for every connection. Stop all running codex-tui processes, preserve database/WAL/SHM and any recovery backups, and inspect before restoring. Never copy a live SQLite DB as a backup or allow two processes to rewrite the same state concurrently. The TUI retains a cross-process SQLite owner lease while running, and offline restore rejects a concurrent owner. A pre-existing symlink/non-regular SQLite database or writer-lock file is rejected before SQLite opens it; this hardens private-path handling but does not grant atomic protection against a malicious same-user process swapping entries concurrently. First v1.4 installation opens only current SQLite v4, intentionally refusing undeployed v1-v3 images without data loss.

## Share a minimal safe diagnostic
<!-- docs-section: artifacts -->

`codex-tui doctor bundle --output ./codex-tui-support` retains redacted metadata and checksums. Check its contents yourself before sharing; remove credentials, internal host names, personal paths, prompts, raw logs and comment bodies. Include minimal command and exit status, the exact Git commit and CI Run IDs when available. Treat unverified real-environment evidence as **not yet verified**.

