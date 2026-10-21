<!-- docs-id: operator -->
<!-- docs-lang: en -->
# Operator handbook: Mission Control, Forge and terminal
<!-- docs-section: overview -->

**Language / 语言:** [English](operator-guide.md) · [简体中文](../zh-CN/guides/operator-guide.md)

This guide covers the **current** v1.4 developer workbench without pretending that v1.4 Stable or real environment qualification is complete. Use in-product `?` for the authoritative keymap and `--help` for CLI behavior. Read [team quickstart](../team-quickstart.md) first.

## First session and target
<!-- docs-section: first -->

~~~bash
codex-tui --version
codex-tui doctor compat
codex-tui doctor codex
codex-tui
~~~

Default target is local `codex app-server --listen stdio://`. Named targets are configured in local TOML; one invocation can use `codex-tui --target NAME`. Non-loopback plaintext `ws://` targets are rejected even without a configured token; use `wss://` or loopback through an SSH tunnel. Loopback IPv4, IPv6 and `localhost` remain permitted for local development. A dead or incompatible backend is **not** silently replaced by fake data; distinguish `--fake` fixture mode from real sessions. Read [Remote Target design](../implementation/v1.3-remote-app-server-targets.md).

`doctor` prints a single-line, terminal-safe presentation of external paths, branches, host names, diagnostics and error strings; ANSI controls and Unicode bidi overrides become visible replacement characters. Original Codex/Git/Forge data is not rewritten.

Human-readable `headless` text output escapes *individual* untrusted fields while retaining tab-delimited rows. `--json` remains a machine-readable serialization of the original snapshot, not display-normalized content.

`doctor compat` escapes unsafe control characters in human-readable tool versions and diagnostic facts. `doctor compat --json` preserves the original evidence and schema.

CLI usage error, support-bundle path and `doctor` startup failure messages also sanitize unsafe terminal-control characters at their final presentation boundary. The normal help text and original file paths remain unchanged.

## Navigate work and attention
<!-- docs-section: daily -->

In Thread view, `PageUp`/`PageDown` scroll the displayed lines of a message, including long paragraphs and code blocks. `Home` moves to the start of loaded history; another `PageUp` there requests an older page. `End` resumes following the newest content. Manual scrolling pauses follow and retains the reading position when messages arrive, older pages load or the terminal is resized. A pending request remains visible while the remaining rows can still browse history.

If a refreshed recent page no longer contains the item you are reading, Thread retains a local snapshot of that older history while live turn status continues to update. The paused-reading label identifies this snapshot; `End` returns to the latest history and releases it, and leaving Thread also releases it.

With a pending request and no text editor active, press `d` in Thread view to read its command, working directory and permission details. In this local detail view, `PageUp`/`PageDown` and `↑`/`↓` scroll, `Home`/`End` move to either end, and `Esc` returns without submitting or cancelling the request. Resolving or replacing the request invalidates its detail view; browsing sends no App Server request. Unrecognized permission fields remain marked as hidden, with acceptance disabled.

Opening a thread or its queue acquires an App Server subscription on the active connection. Server-loaded inventory and history reads alone do not establish that subscription. The connection releases it when the last history, queue or pending-request owner leaves; active work and actionable requests retain their existing ownership. A rejected or uncertain subscription is reported instead of silently replaying writes. The protocol contract has been reviewed against Codex 0.162.1 and 0.163.0; actual account qualification remains separate.

Mission Control groups threads by repository/project and presents Needs You / Working / Ready / Inactive. Check Needs You first; select with `j/k`, open with Enter, filter via `/`, use `Ctrl+K` for commands, and `?` for the live keymap. `b` opens the Board; `h/l` changes columns, `j/k` changes selected WorkCard. Board and Attention are **projections**, never a second team task authority.

Search and Saved Views are local organization; unseen upstream history is not fabricated. Full-history search uses a short-lived isolated Codex App Server connection so it cannot monopolize the live approval/session actor; only one search runs at once, newer requests cancel older searches, and an overall 12-second deadline fails visibly. Partial per-thread occurrences never claim complete history. When an awaited RPC yields to an inbound approval request, the live actor drains that queued request before accepting competing commands or scheduled probes; an interrupted RPC remains outcome-unknown and is not blindly replayed. Short local-index queries treat `%`, `_` and backslash as literal characters rather than SQLite wildcard patterns. Use a concrete repository/cwd to distinguish foreign Windows paths from Ubuntu-local Codex threads. Clearing an upstream project or thread title temporarily projects the safe cwd/ID before a coalesced authoritative refresh; unarchiving requests the same bounded refresh without inventing missing threads. App Server Git origins are normalized to credential-free host/repository identity; malformed origins fall back to cwd, never into search, Headless or display. A weekly advisory compares the pinned protocol fixture with the official Codex stable release; newer upstream metadata requires review and never counts as authenticated L3 qualification.

Local transcript search matches cached user/assistant **message bodies** for both short LIKE queries and longer FTS queries. Thread titles remain searchable through the global metadata search. Missing FTS support uses the same body-only field scope; local results remain an incomplete cache, not full-history authority.

The Review `e` external-editor action now requires a current Git worktree identity matching the selected thread and review. Git status paths resolve from the worktree root (not a nested thread cwd); absolute paths, parent traversal, missing targets and symlinks resolving outside the worktree are rejected before launching an editor. Refresh Review and Git context if the identity is stale. No source path or Git state is modified.

Review external browser links require an unambiguous HTTP(S) host without embedded credentials, control characters, whitespace or backslash normalization. Invalid provider links fail before browser launch; valid GitLab/GitHub URLs are passed through unchanged.

## Repository and delivery context
<!-- docs-section: forge -->

Switch to a repository with a configured Git remote and run `codex-tui doctor git` and `codex-tui doctor forge`. Internal GitLab uses authenticated `glab`; GitHub uses `gh`. A partial GitLab/GitHub read failure keeps healthy capabilities visible but marks Forge data incomplete; failure of all core read endpoints is unavailable, not a fresh empty result. Headless Forge returns a degraded exit code when any core capability is missing.

Forge overview lists are bounded recent first pages (20 items per capability). Raw API page saturation is preserved before GitHub's Issues endpoint filters out pull requests, so even a displayed zero Issues does not prove absence when the source page was full. A current-branch CR or Pipeline not found in that snapshot is **not proof it does not exist**; Board provenance and Review now surface that uncertainty. Exact authoritative results require checking the provider. Forge writes (create/comment/approve/merge MR/PR) are **plan-first, confirm-first**. Approve/merge recheck the target revision; unknown external results are **not** blindly retried. Review page contents remain owned by GitLab/GitHub. Forge Merge requires explicitly known provider mergeability and verified approvals; missing/unknown GitHub mergeability, GitLab approval count or detailed status are blockers. Create/Approve preflight checks use bounded complete pagination, not an assumed first page. Providers retain final authority. GitHub fork PRs and Actions workflows are projected with a qualified source such as `owner/repo:branch`, never mistaken for an unqualified local Git branch. A workflow run with missing `head_repository` is labeled `unverified-source:branch`; its failure must not become a local-branch Pipeline Failed alert. Branch-bound approval, comment and merge require the PR head repository ID and path to match the selected local repository; missing head provenance refuses the operation rather than guessing. GitLab cross-project MRs likewise display `project/<id>:branch` instead of the local short branch. GitLab branch-bound writes require verified `source_project_id` and `target_project_id` matching the selected project; missing or fork source identity fails closed. See [provider evidence](../qualification/provider.md).

For managed Worktree deletion, active Codex threads with unknown repository or cwd are treated as possible conflicts. A destructive operation queued for too long expires its UI activity proof and must be reviewed and confirmed again; a nonzero Git exit is reconciled against observed Git state rather than assumed side-effect-free. A Worktree removal is verified only when **both** Git registration and the filesystem entry are absent; if they disagree, the receipt stays `OutcomeUnknown` without an automatic retry. This guard cannot atomically reserve a Worktree against independent Codex processes. Do not use it as a substitute for real concurrent-process qualification.

Managed Worktree creation binds the default or explicitly entered `HEAD` to the complete commit ID already observed for the selected thread's worktree, including linked and detached worktrees. The confirmation shows the observed commit; refresh Git context first if you want a newer observation. Moving `HEAD` after preview cannot change that plan's start commit, and verification requires the created worktree to match it. Other explicitly entered refs remain as typed and are resolved by Git at execution; their preview does not claim a fixed commit.

Removal requires no tracked changes, untracked files **or ignored files**, including ignored local configuration and build directories. Save or remove those assets yourself before making a new removal plan. The preflight includes ignored files and refuses a failed check, output over 256 KiB or a five-second probe timeout. This adds no force mode or automatic cleanup, and retains managed/adopted identity and active-scope checks. Independent filesystem changes after the check still require real concurrent-process qualification.

## Local language and accessibility
<!-- docs-section: language -->

Set `[ui].language = "en"` or `"zh-CN"`, or leave `"auto"` for locale detection. `presentation = "quiet"` coalesces background redraws; `"screen-reader"` uses longer background intervals while keeping direct input responsive. `[notifications].mode` defaults `"off"`. Desktop notification subjects are escaped and truncated for display only; Linux OS notifications pass dynamic text after the `notify-send --` option terminator. Enabled notifications use a bounded dispatcher plus a 128-event pending retry queue, drained by the normal UI loop; saturation reports the number of events not queued and directs the operator to Needs You rather than claiming lossless delivery. Original work item titles and notification tracking remain unchanged. These are local preferences, not team policy or synchronization.

Managed Worktree mutation/recovery prefers complete NUL-delimited Git worktree inventory, preserving newline and trailing-space directory names. Older Git 2.25 (as found in the Linux qualification image) lacks the worktree `-z` flag: a strict, complete plaintext porcelain fallback keeps trailing spaces and rejects malformed/ambiguous paths rather than inferring absence. Newline-bearing paths require NUL-capable Git for safe representation. Invalid encoding or Git stdout over 256 KiB fails closed; large inventories need explicit qualification rather than automatic false-success.

Terminal Drawer output draining is cooperative: at most 16 queued PTY events are parsed per main-loop iteration, leaving burst output in the existing 64-slot FIFO rather than starving keyboard, rendering and reconciliation during a noisy child process. A delayed PTY Ready event cannot revert an earlier requested terminal resize or desynchronize the VT parser dimensions. The PTY queue remains bounded; this hosted regression is not a replacement for real controlling-TTY qualification.

## Windows SSH Ubuntu and Terminal Drawer
<!-- docs-section: ssh -->

When using a Windows SSH client to operate an Ubuntu host, the TUI runs on **Ubuntu**; the target tty, filesystem, Codex CLI, config and spawned shell must all be resolved there. `t` opens the embedded Drawer; `F6` or `Ctrl+]` returns focus when supported by the emulator. If keys are intercepted by Windows/SSH, use `?` and `doctor terminal`; validate resize, focus, Ctrl+C, exit and terminal restoration on a **real controlling TTY**. Hosted PTY regression is not that proof. The UI neutralizes control characters, Unicode direction overrides and visually ignorable formatting (soft hyphen, zero-width space and word joiner) in untrusted conversation items, Git diff text, repository paths and provider labels. The selected-thread detail and offline footer apply the same display-only protection to thread IDs, model labels, Git branches/errors and App Server source. Managed Worktrees plan/confirmation and Board change-request status apply the same protection to external Git/Forge values without bypassing explicit confirmation. Forge mutation, Launch Preset and local batch confirmation overlays, typed local editors and Command Palette search also neutralize untrusted text; all original mutation payloads and external launch argv are preserved. Mission Control and Board footer input, custom Saved View titles/groups and Scratch IDs use the same rendering protection, without altering queries or saved metadata. Only the display projection is modified; canonical Codex/Git data remains unchanged. The Terminal Drawer is a separate PTY emulator, not a sanitized preview.

## Evidence and state safety
<!-- docs-section: safety -->

SQLite persists personal metadata/planning overlays. Derived raw user/assistant message text is **not** indexed by default. To opt into searchable local transcript caching, set `[search] persist_local_transcripts = true` in config.toml; up to 10,000 items, at most 4,096 characters each for both message text and title, retained for at most 30 days from first local indexing when maintenance runs. Re-reading the same cached item does not renew its retention deadline. Turning the option off triggers logical removal of local transcript/FTS rows at the next startup, not forensic flash erasure. Codex remains the transcript authority. Never delete/overwrite a corrupt DB as a substitute for diagnosis; preserve store/WAL/SHM and create an offline recovery plan. Use `doctor bundle --output ./codex-tui-support` after checking privacy, and keep exact source SHA in bug reports. Do not publish Stable from the TUI.

