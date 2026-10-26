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
codex-tui doctor bundle --offline --output ./codex-tui-support
~~~

`doctor compat` does not query remote Forge and can be Ready even when `glab` is absent. Probe Forge inside the relevant repository. `doctor bundle` contains redacted metadata; review it before sharing.

`doctor bundle --offline` collects build identity and local SQLite metadata without starting external executables or connecting to App Server/Forge targets. External components are explicitly unavailable/not-probed, readiness remains Blocked, and `realCodexQualification=false`. This is local support/redaction evidence, not live-server or device qualification. Automatic development and release bundle collection uses this mode and checks its markers; all existing qualification gates remain. Without `--offline`, bundle collection retains live probes and may start Codex App Server. Review either bundle before sharing.

## Read-only headless commands
<!-- docs-section: headless -->

~~~bash
codex-tui headless threads --json
codex-tui headless threads --target local-alt --json
codex-tui headless work --json
codex-tui headless status --json
codex-tui headless attention --json
codex-tui headless board --json
codex-tui headless forge --json
codex-tui headless worktrees --json
codex-tui status
codex-tui status --target local-alt --json
codex-tui thread list
codex-tui attention list
codex-tui board list
codex-tui forge status
codex-tui worktree list
~~~

`headless threads --fixture-10k` gives synthetic scale metadata. There is no headless mutation API. Do not infer Forge authentication from headless fixture output.

`threads`, `work`, `status`, `attention` and `board`, including their top-level aliases, resolve the configured `[app_server].active` target unless `--target NAME` overrides it. Each invocation reads one target. An unknown target, invalid configuration or failed connection reports a degraded result and exits `3`; it never silently falls back to local or fixture data. A missing configuration uses in-memory defaults without creating a config file. Missing or duplicate target arguments, `--target` with `--fake`/`--fixture-10k`, and `--target` on `forge`/`worktrees` are syntax errors (`2`).

| Output | Scope |
| --- | --- |
| Codex-backed `source.targetId`, `source.transport`, `source.scope` | Selected target and transport; `single-target` for real reads, `fixture` for synthetic input |
| `work`/`status`/`attention`/`board` `localStoreScope` | `shared-across-targets` for local metadata, `not-loaded` if source resolution failed before loading, or `fixture` |
| Forge `scope` | `current-directory` (or `fixture`) |
| Worktrees `scope` | `shared-local-cache` (or `fixture`) |

The local store still keys aliases, pins and planning metadata by bare thread IDs. Two targets reusing an ID can therefore share that metadata; target selection does not isolate it. `threads` reports the source inventory without those overlays. Invalid target resolution occurs before store loading. Successful local-store reads retain the existing SQLite initialization/WAL behavior; “read-only” describes the lack of upstream mutations, not a promise of zero local filesystem writes. Plain-text output also identifies the source and scope and presents failures explicitly.

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

