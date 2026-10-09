<!-- docs-id: support -->
<!-- docs-lang: en -->
# Support, diagnostics and known boundaries
<!-- docs-section: overview -->

**Language / 语言:** [English](SUPPORT.md) · [简体中文](SUPPORT.zh-CN.md)

codex-tui is maintained as a local-first development tool. Support is issue-based and best-effort; public issues are not suitable for credentials or undisclosed vulnerabilities. See [SECURITY.md](SECURITY.md) for private reporting.

## First checks
<!-- docs-section: triage -->

~~~bash
codex-tui --version
codex-tui doctor compat
codex-tui doctor codex
codex-tui doctor store
codex-tui doctor terminal
~~~

Run `codex-tui doctor forge` **from the affected repository**, after setting up a working `gh` or `glab` login as appropriate. `doctor compat` can say Ready without GitLab because Forge clients are optional globally; use [provider qualification](docs/qualification/provider.md) for internal deployments.

## Creating a useful issue
<!-- docs-section: issue -->

Use the bilingual [bug form](.github/ISSUE_TEMPLATE/bug_report.yml). Provide codex-tui version and exact SHA, OS/terminal (including Windows→SSH Ubuntu, TERM), Codex CLI version/target type, whether the case is real or `--fake`, expected vs actual behavior, minimal sanitized commands, and the names/IDs of CI runs if relevant. Use `codex-tui doctor bundle --output ./codex-tui-support` only after verifying privacy; do not upload sensitive files. The maintainer cannot infer real-provider PASS from hosted fixtures.

## Known limitations and escalation
<!-- docs-section: limits -->

v1.4 Stable is unpublished, and neither v1.0 nor v1.4 has been deployed. Internal GitLab capability, authenticated Codex, real controlling-TTY focus/resize/Ctrl-C, administrator release settings and explicit publication approval remain external. A degraded/blocked status is not silently repaired by deleting SQLite. Before any potentially destructive action retain the state directory and review [troubleshooting](docs/guides/troubleshooting.md).

