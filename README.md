<!-- docs-id: home -->
<!-- docs-lang: en -->
# codex-tui — Multi-Repository Codex Terminal Workbench
<!-- docs-section: overview -->

**Language / 语言:** [English](README.md) · [简体中文](README.zh-CN.md)

codex-tui is a **local-first Mission Control for individual developers, reusable by teams**. It projects Codex sessions, Git worktrees and GitLab/GitHub delivery context into one terminal. It does not replace the official Codex CLI, Git or Forge, and is not another Agent or task service.

**Current status:** `v1.4.0` is the first-deployment candidate, **not a published v1.4 Stable release**. Historical v1.0.0 was published but the maintainer confirms neither v1.0 nor v1.4 has been deployed. `stableReady=false`, `publicationAllowed=false`. Hosted CI cannot qualify a real account, controlling terminal or internal GitLab.

Start with [installation](docs/release/install-upgrade.md), [team quickstart](docs/team-quickstart.md), or the [documentation index](docs/README.md). Historical plans, research and evidence remain at their original paths; this page describes the current workflow rather than a milestone archive.

## Development and release status
<!-- docs-section: status -->

v1.4 freezes new core functionality; defect, security, interoperability, evidence and maintenance fixes remain allowed. v1.2/v1.3 plans are historical, not deployed-version migration obligations. First installation creates SQLite v4 directly; it neither imports `state-v1.json` nor migrates undeployed SQLite v1–v3. See [first-deployment decision](docs/implementation/v1.4-first-deployment-baseline.md).

Protected-main CI and preview artifacts establish an automated engineering baseline only. Actual environment acceptance and publication authorization remain in [#211](https://github.com/jiying2007/codex-tui/issues/211).

## Core value and daily workflow
<!-- docs-section: product -->

**Find what needs you → open the right context → act explicitly → check the result → return to the next item.**

| Task | Starting point |
|---|---|
| Find the next actionable session | `Space`; `/` searches; `j/k` or arrows select; `Enter` opens |
| Return to frequent work | Select a task, `=` then `1–9` binds it; the digit opens it; `Esc` returns one level |
| Inspect delivery context | `b` Board, `Tab` Saved Views, `r` Review, `w` Workspace |
| Find commands and help | `Ctrl+K` command palette; `?` help; arrows, `PgUp/PgDn`, `Home/End` navigate help |
| Open a recent observed event | `!` when its visible, unexpired alert has a related Codex thread |
| Recover a failed first connection | Plain `F5` in empty Mission Control after the failure hint, with overlays closed |

Mission Control prioritizes actionable attention, pins and recent activity, retaining selected identity across refreshes. Its recent window is bounded; pinned/actionable exceptions remain reachable. Board List/Review Queue views group by workspace. Notes, aliases, pins, saved views and personal planning overlays are local; WorkCards remain derived from Codex/Git/Forge. Forge writes require plan, explicit confirmation, execution and a truthful receipt.

**Idle is not success.** A working/waiting session becoming ready reports that it is idle and its result needs inspection, not that a task passed. An explicit Goal-complete observation is separate and still not delivery acceptance. System errors, usage/budget limits, approvals, input requests and supported delivery events are observed without inventing unseen upstream results.

**Silent does not mean invisible.** Default notification mode `off` disables external delivery, not the in-TUI recent event and related-thread jump. The single recent event expires after 20 seconds, is not persisted, and does not steal editor/modal/terminal focus. It is not a notification history or a delivery guarantee.

The `F5` path retries only the same first-connection target, one attempt at a time. After any Registry has been installed it stays disabled, including after a later disconnect; no prompt, approval, queue or Forge write is replayed. Invalid configuration still requires correction before launch. See [startup retry](docs/implementation/startup-retry.md).

Use the in-product help and failure hint for contextual keys. Mouse click/wheel navigation is not implemented; mouse capture is disabled and the old `[ui].mouse` key is parsed but ignored. Full behavior and limits are in the [operator guide](docs/guides/operator-guide.md) and [CLI reference](docs/guides/cli-reference.md).

## Architecture and authority boundaries
<!-- docs-section: architecture -->

~~~text
Codex App Server: conversation / Agent / queue authority
        |
        +-- Registry / Attention / Mission Control
        +-- derived Board / WorkCard
        +-- Git / worktree and GitLab / GitHub projections
        +-- SQLite: personal UI / planning metadata only
        |
        +-- Ratatui terminal interface
~~~

No cloud service, shared task database, RBAC or independent Forge/terminal backend is introduced. Thread View, Thread Queue and Terminal Drawer are maintenance-only overlap surfaces. See [upstream convergence](docs/adr/012-upstream-convergence.md).

## Build and run
<!-- docs-section: run -->

Building needs **Rust 1.88+** and Git. Real sessions need an authorized Codex installation; the corresponding Forge needs authenticated `glab` or `gh`. Forge clients are not mandatory when that Forge is unused. Verify trusted packages before installation; see [installation and archive verification](docs/release/install-upgrade.md).

~~~bash
git clone https://github.com/jiying2007/codex-tui.git
cd codex-tui
cargo build --release --locked
./target/release/codex-tui --version
./target/release/codex-tui --help
~~~

For metadata-only diagnostics without external processes or target connections:

~~~bash
./target/release/codex-tui doctor bundle --offline --output ./codex-tui-support
~~~

Offline diagnostics can create local SQLite state and the output directory; they are not a zero-write mode or real compatibility qualification. Only when real-backend use is authorized, run `doctor compat`, `doctor codex`, then the interactive program. Fake/scale fixtures do not qualify real accounts.

Over **Windows SSH Ubuntu**, use the Linux executable on Ubuntu. Its filesystem, Codex, configuration and child processes belong to Ubuntu, not the Windows desktop. `t` opens Terminal Drawer; `F6` or `Ctrl+]` releases focus where supported. Default external GUI opening over SSH is refused before spawn without an explicit `CODEX_TUI_EDITOR`/`CODEX_TUI_BROWSER` adapter. An adapter still runs on the TUI host; no client desktop bridge or confirmed application-open receipt is provided.

## Language, configuration and notifications
<!-- docs-section: language -->

Edit the local configuration path reported by `codex-tui doctor` when that diagnostic is permitted:

~~~toml
[ui]
language = "auto"      # auto | en | zh-CN
mouse = false          # parsed but no pointer navigation
presentation = "normal" # normal | quiet | screen-reader

[notifications]
mode = "off"           # external delivery: off | terminal | os; visual alerts remain

[app_server]
active = "local"
~~~

Locale detection checks `LC_ALL`, `LC_MESSAGES`, then `LANG`; Simplified Chinese locales such as `zh_CN`, `zh_SG` and `zh-Hans` choose Chinese. Traditional Chinese and unknown locales fall back to English. Technical identifiers and upstream errors retain their diagnostic meaning. Quiet/screen-reader modes coalesce background redraws, not direct input; their names do not certify full screen-reader accessibility or reduced resource consumption.

Choose `terminal` to opt into BEL delivery. SSH `os` delivery falls back to BEL, **not a Windows Toast**. A Windows Terminal SSH profile can set `"bellStyle": "all"`; test the actual path with `printf '\a'` on Ubuntu. Enabled external delivery coalesces bursts within 75 ms and retains bounded queues with explicit overflow reporting. Sound or OS delivery failure does not suppress the selected visual event. No listening continues after exit/SSH loss, and events first observed as historical state are not replayed as new alerts.

Named stdio/WebSocket/Unix Socket targets use local TOML; tokens are referenced by `auth_token_env`, not committed. See [remote target design](docs/implementation/v1.3-remote-app-server-targets.md). Headless Codex views read one active/named target; local metadata remains shared and identical bare thread IDs across targets can share overlays. See the CLI reference before treating this as cross-target isolation.

## Explicit non-goals
<!-- docs-section: boundaries -->

No second Agent orchestrator, Agent messaging, conversation/queue authority, generic plugin platform, tmux replacement, team database, generic cross-Agent API, Web service or finance product. Prefer deleting, reducing or projecting upstream overlap, not duplicating it. Existing overlap surfaces receive bugfix, compatibility and security maintenance only.

## Acceptance, troubleshooting and release qualification
<!-- docs-section: qualification -->

Start with the visible failure and [troubleshooting guide](docs/guides/troubleshooting.md). With permission to probe the corresponding system, diagnose compatibility, Codex, Git, Forge from a representative repository, local store and terminal separately. Support bundles omit tokens, prompts, transcripts and raw probe errors, but still require privacy review; keep real tasks and internal evidence private.

The [UX qualification plan](docs/guides/ux-qualification.md) compares real task time, wrong-target operations, onboarding and alert return paths against the existing toolchain. Its targets are **not measured savings**. Hosted synthetic timings and five-minute structural soaks are not SSH latency, hours of field use, token savings or a full accessibility assessment.

Internal GitLab needs an authenticated [provider fixture](docs/qualification/provider.md). Stable requires exact-source Linux compatibility/TTY/performance evidence, strict main protection, immutable Releases, reviewed release notes, independent checks and explicit authorization. Automated preview self-tests use `publish=false`; they do not replace the separate stable dry run. See [release qualification](docs/guides/release-qualification.md).

## Documentation and contribution
<!-- docs-section: license -->

**Apache-2.0**, see [LICENSE](LICENSE). Read [CONTRIBUTING](CONTRIBUTING.md), [SECURITY](SECURITY.md) and [SUPPORT](SUPPORT.md). Source and documentation changes use protected PRs and required gates. Do not rewrite or delete historical releases, tags or evidence. The [documentation index](docs/README.md) links detailed current guides and retained historical designs.
