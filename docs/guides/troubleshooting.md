<!-- docs-id: troubleshooting -->
<!-- docs-lang: en -->
# Troubleshooting: Codex, Forge, SSH and SQLite
<!-- docs-section: overview -->

**Language / 语言:** [English](troubleshooting.md) · [简体中文](../zh-CN/guides/troubleshooting.md)

Diagnose the failed authority first; avoid deleting local state, changing upstream credentials or inventing successful recovery. Keep the exact source SHA, target type and relevant sanitized error category. For disclosure-sensitive cases see [SECURITY](../../SECURITY.md).

## App Server will not connect or load threads
<!-- docs-section: codex -->

Run `codex-tui doctor codex` (optionally `--target NAME`). Check the real `codex --version` and its login/account independently. Named remote WebSocket/Unix targets have bounded connect/handshake timeouts; a timeout is not proof an action never executed. Verify process/transport, authentication, TLS proxy and App Server capability errors. Older optional methods may degrade; no real-to-fake fallback exists. Avoid pasting bearer tokens into an issue.

## GitLab/GitHub unavailable or missing data
<!-- docs-section: forge -->

In the repository at issue: inspect `git remote -v` **privately**, run `codex-tui doctor git` then `codex-tui doctor forge`. Sign in `glab` or `gh` to the correct host outside the tool. GitLab Issues/MRs/Pipelines probe independently; a single unavailable capability is not entire project failure. `doctor compat` Ready does not prove internal GitLab. Use [provider evidence](../qualification/provider.md) for a required internal rollout.

## Windows paths or repository-backed threads look wrong
<!-- docs-section: cwd -->

On Windows→SSH Ubuntu the TUI reads **Ubuntu-side** Codex Home, shell cwd and Git repository identity. A Windows cwd embedded in a stored Codex thread is foreign provenance, not an Ubuntu checkout. Confirm `doctor codex` reported paths and `doctor git` in the actual checkout. Do not concatenate a Windows absolute path onto an Ubuntu project path or silently invent a local thread.

## Drawer focus, Ctrl+C, resize and rendering
<!-- docs-section: terminal -->

Try the in-app `?` keymap: `t` opens the Drawer and `F6` (or `Ctrl+]`) returns focus if supported. Remote terminal/SSH clients may intercept key combinations. Run `codex-tui doctor terminal` and check TERM, TTY, resizing and direct child exit. Real controlling-TTY focus/resize/Ctrl+C/restore smoke is required for Stable and is not replaced by hosted PTY tests.

## SQLite state damaged or forward schema rejected
<!-- docs-section: store -->

Run `codex-tui doctor store` for an explicit SQLite integrity scan. The interactive application also runs integrity screening once on startup; ordinary state/index reads and writes no longer repeat a full quick_check for every connection. Stop all running codex-tui processes, preserve database/WAL/SHM and any recovery backups, and inspect before restoring. Never copy a live SQLite DB as a backup or allow two processes to rewrite the same state concurrently. The TUI retains a cross-process SQLite owner lease while running, and offline restore rejects a concurrent owner. First v1.4 installation opens only current SQLite v4, intentionally refusing undeployed v1-v3 images without data loss.

## Share a minimal safe diagnostic
<!-- docs-section: artifacts -->

`codex-tui doctor bundle --output ./codex-tui-support` retains redacted metadata and checksums. Check its contents yourself before sharing; remove credentials, internal host names, personal paths, prompts, raw logs and comment bodies. Include minimal command and exit status, the exact Git commit and CI Run IDs when available. Treat unverified real-environment evidence as **not yet verified**.

