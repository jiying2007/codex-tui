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

Default target is local `codex app-server --listen stdio://`. Named targets are configured in local TOML; one invocation can use `codex-tui --target NAME`. A dead or incompatible backend is **not** silently replaced by fake data; distinguish `--fake` fixture mode from real sessions. Read [Remote Target design](../implementation/v1.3-remote-app-server-targets.md).

## Navigate work and attention
<!-- docs-section: daily -->

Mission Control groups threads by repository/project and presents Needs You / Working / Ready / Inactive. Check Needs You first; select with `j/k`, open with Enter, filter via `/`, use `Ctrl+K` for commands, and `?` for the live keymap. `b` opens the Board; `h/l` changes columns, `j/k` changes selected WorkCard. Board and Attention are **projections**, never a second team task authority.

Search and Saved Views are local organization; unseen upstream history is not fabricated. Full-history search uses a short-lived isolated Codex App Server connection so it cannot monopolize the live approval/session actor; only one search runs at once, newer requests cancel older searches, and an overall 12-second deadline fails visibly. Partial per-thread occurrences never claim complete history. Short local-index queries treat `%`, `_` and backslash as literal characters rather than SQLite wildcard patterns. Use a concrete repository/cwd to distinguish foreign Windows paths from Ubuntu-local Codex threads.

## Repository and delivery context
<!-- docs-section: forge -->

Switch to a repository with a configured Git remote and run `codex-tui doctor git` and `codex-tui doctor forge`. Internal GitLab uses authenticated `glab`; GitHub uses `gh`. A partial GitLab/GitHub read failure keeps healthy capabilities visible but marks Forge data incomplete; failure of all core read endpoints is unavailable, not a fresh empty result. Headless Forge returns a degraded exit code when any core capability is missing.

Forge overview lists are bounded recent first pages (20 items per capability). Raw API page saturation is preserved before GitHub's Issues endpoint filters out pull requests, so even a displayed zero Issues does not prove absence when the source page was full. A current-branch CR or Pipeline not found in that snapshot is **not proof it does not exist**; Board provenance and Review now surface that uncertainty. Exact authoritative results require checking the provider. Forge writes (create/comment/approve/merge MR/PR) are **plan-first, confirm-first**. Approve/merge recheck the target revision; unknown external results are **not** blindly retried. Review page contents remain owned by GitLab/GitHub. Forge Merge requires explicitly known provider mergeability and verified approvals; missing/unknown GitHub mergeability, GitLab approval count or detailed status are blockers. Create/Approve preflight checks use bounded complete pagination, not an assumed first page. Providers retain final authority. See [provider evidence](../qualification/provider.md).

For managed Worktree deletion, active Codex threads with unknown repository or cwd are treated as possible conflicts. A destructive operation queued for too long expires its UI activity proof and must be reviewed and confirmed again; a nonzero Git exit is reconciled against observed Git state rather than assumed side-effect-free. A Worktree removal is verified only when **both** Git registration and the filesystem entry are absent; if they disagree, the receipt stays `OutcomeUnknown` without an automatic retry. This guard cannot atomically reserve a Worktree against independent Codex processes. Do not use it as a substitute for real concurrent-process qualification.

## Local language and accessibility
<!-- docs-section: language -->

Set `[ui].language = "en"` or `"zh-CN"`, or leave `"auto"` for locale detection. `presentation = "quiet"` coalesces background redraws; `"screen-reader"` uses longer background intervals while keeping direct input responsive. `[notifications].mode` defaults `"off"`. These are local preferences, not team policy or synchronization.

## Windows SSH Ubuntu and Terminal Drawer
<!-- docs-section: ssh -->

When using a Windows SSH client to operate an Ubuntu host, the TUI runs on **Ubuntu**; the target tty, filesystem, Codex CLI, config and spawned shell must all be resolved there. `t` opens the embedded Drawer; `F6` or `Ctrl+]` returns focus when supported by the emulator. If keys are intercepted by Windows/SSH, use `?` and `doctor terminal`; validate resize, focus, Ctrl+C, exit and terminal restoration on a **real controlling TTY**. Hosted PTY regression is not that proof.

## Evidence and state safety
<!-- docs-section: safety -->

SQLite persists personal metadata/planning overlays. Derived raw user/assistant message text is **not** indexed by default. To opt into searchable local transcript caching, set `[search] persist_local_transcripts = true` in config.toml; up to 10,000 items, at most 4,096 characters each for both message text and title, retained for at most 30 days from first local indexing when maintenance runs. Re-reading the same cached item does not renew its retention deadline. Turning the option off triggers logical removal of local transcript/FTS rows at the next startup, not forensic flash erasure. Codex remains the transcript authority. Never delete/overwrite a corrupt DB as a substitute for diagnosis; preserve store/WAL/SHM and create an offline recovery plan. Use `doctor bundle --output ./codex-tui-support` after checking privacy, and keep exact source SHA in bug reports. Do not publish Stable from the TUI.

