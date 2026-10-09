<!-- docs-id: cli -->
<!-- docs-lang: en -->
# CLI command reference (v1.4 development line)
<!-- docs-section: overview -->

**Language / 语言:** [English](cli-reference.md) · [简体中文](../zh-CN/guides/cli-reference.md)

The executable `codex-tui --help` and per-command `--help` are authoritative. Examples below are valid documented routes, not evidence of a live account or deployed release. Exit codes: `0` success, `1` unexpected runtime error, `2` invalid syntax, `3` degraded/blocked.

## Interactive workbench
<!-- docs-section: interactive -->

~~~bash
codex-tui
codex-tui --target local-alt
codex-tui --fake
codex-tui --version
codex-tui --help
~~~

The no-arg launch opens Mission Control. Named targets must exist in local config; `--fake` cannot be combined with `--target` and is never a fallback for failed real sessions. Keymap is shown with `?`.

## Doctor and safe diagnostic exports
<!-- docs-section: doctor -->

~~~bash
codex-tui doctor compat --json
codex-tui doctor codex --target local-alt
codex-tui doctor git
codex-tui doctor forge
codex-tui doctor store
codex-tui doctor presets
codex-tui doctor terminal
codex-tui doctor bundle --output ./codex-tui-support
~~~

`doctor compat` does not query remote Forge and can be Ready even when `glab` is absent. Probe Forge inside the relevant repository. `doctor bundle` contains redacted metadata; review it before sharing.

## Read-only headless commands
<!-- docs-section: headless -->

~~~bash
codex-tui headless threads --json
codex-tui headless work --json
codex-tui headless status --json
codex-tui headless attention --json
codex-tui headless board --json
codex-tui headless forge --json
codex-tui headless worktrees --json
codex-tui status
codex-tui thread list
codex-tui attention list
codex-tui board list
codex-tui forge status
codex-tui worktree list
~~~

`headless threads --fixture-10k` gives synthetic scale metadata. There is no headless mutation API. Do not infer Forge authentication from headless fixture output.

## Development-only release and soak measurements
<!-- docs-section: diagnostics -->

~~~bash
codex-tui release verify --channel preview --tag v1.4.0-preview.1 --commit SOURCE_SHA --json
codex-tui release benchmark --iterations 200 --json
codex-tui release render-benchmark --iterations 200 --json
codex-tui release interaction-benchmark --iterations 200 --json
codex-tui release scale --rows 50000 --warmup 5 --iterations 50 --json
codex-tui release failure-matrix --json
codex-tui soak --rows 50000 --cycles 256 --duration-seconds 300 --json
~~~

Use the actual 40-character source SHA instead of `SOURCE_SHA`. These are diagnostic/verification routes; they cannot grant Stable publishing authority. Resource bounds are checked by the CLI.

## Error-handling and exit status
<!-- docs-section: exit -->

`2` means invalid command/argument; `3` means a requested Doctor/headless capability is degraded or blocked; `1` is an unexpected runtime error. Always preserve the relevant state before diagnosis; do not silently retry write outcomes marked unknown. For errors containing sensitive internal URLs, report a sanitized category, not a raw child stderr dump.

## Source of truth and stable publication
<!-- docs-section: authority -->

CLI descriptions do not supersede Codex App Server/Forge/Git authority. Consult [first-deployment policy](../implementation/v1.4-first-deployment-baseline.md) and [release qualification](release-qualification.md) before any deployment. v1.4 is not a published Stable channel.

