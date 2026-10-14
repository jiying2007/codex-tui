<!-- docs-id: device-matrix -->
<!-- docs-lang: en -->
# Device acceptance record — NOT RUN
<!-- docs-section: overview -->

**Language / 语言:** [简体中文](../zh-CN/guides/device-matrix.md)

Copy privately, never commit credentials or raw tasks. Select development-integration or final-candidate. Record source SHA, binary digest, device/SSH/Codex/provider versions, configuration hashes, timezone, operator and independent reviewer. States: NOT_RUN, PASS, FAIL, BLOCKED or independently approved N/A. No automatic completion.

[Runbook](device-validation.md)

## Execution matrix
<!-- docs-section: matrix -->

| ID | Check | Status | Actual outcome / evidence | Reviewer |
|---|---|---|---|---|
| E01 | Source, remote main and clean worktree | NOT_RUN | | |
| E02 | Architecture/ABI and Python/Rust/Git/CLI versions | NOT_RUN | | |
| E03 | Real TTY, UTF-8 and terminal dimensions | NOT_RUN | | |
| E04 | Actual executable source and digest | NOT_RUN | | |
| A01 | Device tool and observer self-tests | NOT_RUN | | |
| A02 | Nine controlled suites (not real service qualification) | NOT_RUN | | |
| C01 | Two real repository threads and correct Cwd | NOT_RUN | | |
| C02 | Real TUI task completion and correct result | NOT_RUN | | |
| C03 | Actual approval allowed and refused | NOT_RUN | | |
| C04 | Actual server user-input request | NOT_RUN | | |
| C05 | Active task across Board/search/return | NOT_RUN | | |
| C06 | Actual task cancellation without replay | NOT_RUN | | |
| C07 | Drafts/Saved Views/notes across restart | NOT_RUN | | |
| C08 | Navigation, Chinese, scroll and narrow windows | NOT_RUN | | |
| C09 | Named targets when declared (reviewed N/A otherwise) | NOT_RUN | | |
| T01 | Drawer Cwd and echo | NOT_RUN | | |
| T02 | Real width and height resize | NOT_RUN | | |
| T03 | Ctrl+C then usable shell | NOT_RUN | | |
| T04 | F6, refocus and uppercase T close | NOT_RUN | | |
| T05 | Normal parent-terminal restoration | NOT_RUN | | |
| T06 | Catchable-signal abnormal exit restoration | NOT_RUN | | |
| T07 | SSH disconnect/reconnect and separate TTY evidence | NOT_RUN | | |
| T08 | Required panic/error scope (do not fabricate) | NOT_RUN | | |
| G01 | Git status, diff and branch | NOT_RUN | | |
| G02 | Managed Worktree creation and identity | NOT_RUN | | |
| G03 | Active/dirty delete refusal and cancellation | NOT_RUN | | |
| G04 | Allowed deletion agrees with Git and filesystem | NOT_RUN | | |
| G05 | Editor path boundaries | NOT_RUN | | |
| G06 | Independent Codex and same-repository concurrency | NOT_RUN | | |
| F01 | Internal GitLab read capabilities and fresh fixture | NOT_RUN | | |
| F02 | Test comment cancel/confirm exactly once | NOT_RUN | | |
| F03 | Approval revision binding and changed-HEAD refusal | NOT_RUN | | |
| F04 | Allowed dedicated test MR merge | NOT_RUN | | |
| F05 | Permission/failure/unknown-outcome without replay | NOT_RUN | | |
| F06 | GitHub counterpart in declared scope | NOT_RUN | | |
| S01 | Healthy Store, schema and single writer | NOT_RUN | | |
| S02 | Consistent backup includes committed state | NOT_RUN | | |
| S03 | New isolated Store recovery and data comparison | NOT_RUN | | |
| L01 | Agreed actual duration and task coverage | NOT_RUN | | |
| L02 | Root and family trends reviewed separately | NOT_RUN | | |
| L03 | Resource/activity stabilization after tasks | NOT_RUN | | |
| L04 | Actual recovery, residual processes and state integrity | NOT_RUN | | |
| L05 | Observed time/steps/token cost where available | NOT_RUN | | |
| Q01 | Final real compatibility/terminal/GitLab evidence | NOT_RUN | | |
| Q02 | Final-candidate Linux chain and rebuilt digest check | NOT_RUN | | |
| Q03 | Private archive verification and public-summary redaction | NOT_RUN | | |

## Independent conclusions
<!-- docs-section: conclusion -->

Device integration: NOT_RUN. Qualification review: NOT_RUN. Actual installation/recovery: NOT_RUN. Administrator/rehearsal/publication authority: NOT_RUN. Record declared scope, blockers, unobserved cases and responsible follow-up. Preserve failures; source or binary changes require affected revalidation. Device acceptance never grants Stable publication.
