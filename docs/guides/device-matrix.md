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
| E01 | Exact source, reviewed remote branch and clean checkout (main for final-candidate) | NOT_RUN | | |
| E02 | Architecture/ABI and Python/Rust/Git/CLI versions | NOT_RUN | | |
| E03 | Real TTY, UTF-8 and terminal dimensions | NOT_RUN | | |
| E04 | Actual executable source and digest | NOT_RUN | | |
| A01 | Device tool and observer self-tests | NOT_RUN | | |
| A02 | Nine controlled suites (not real service qualification) | NOT_RUN | | |
| C01 | Two real repository threads and correct Cwd | NOT_RUN | | |
| C02 | Real TUI task completion and correct result | NOT_RUN | | |
| C03 | Actual approval allowed and refused | NOT_RUN | | |
| C04 | Actual server user-input request | NOT_RUN | | |
| C05 | Active task across Registry/Workspace/search/return in Lite; Board only in historical scope | NOT_RUN | | |
| C06 | Actual task cancellation without replay | NOT_RUN | | |
| C07 | Drafts/notes across restart; historical Saved Views preserved, not edited in Lite | NOT_RUN | | |
| C08 | Navigation, Chinese, scroll and narrow windows | NOT_RUN | | |
| C09 | Named targets when declared (reviewed N/A otherwise) | NOT_RUN | | |
| T01 | Drawer Cwd and echo | NOT_RUN | | |
| T02 | Real width and height resize | NOT_RUN | | |
| T03 | Ctrl+C then usable shell | NOT_RUN | | |
| T04 | Alt+T two-way focus, shell retained, uppercase T close | NOT_RUN | | |
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

For Lite pre-merge development-integration, retain the historical mutation rows G02–G04 and F02–F06 as NOT_RUN/BLOCKED for the retired TUI surface unless an independent reviewer approves the replacement scope. Native actions do not retroactively pass old Stable checks. The following rows record the existing unified design's field acceptance, not a new publication gate:

| ID | Check | Status | Actual outcome / evidence | Reviewer |
|---|---|---|---|---|
| R01 | G1: real source coverage/Partial, parent-child/Fork, two Target/CODEX_HOME scopes, exact request routing without wrong target or missed Attention | NOT_RUN | | |
| R02 | G2: actual requirement and actual defect linked to original Feishu Project/TAPD item, exact session and evidence; original owner acceptance | NOT_RUN | | |
| R03 | G3: two different actual cross-repository scenarios, exact versions, directed recipient acknowledgment and separate joint test result | NOT_RUN | | |
| R04 | G4: native Git/Forge/Codex replacements, no retired interactive entry, private original backup and isolated recovery; all historical unresolved counts reconciled or explicitly retained | NOT_RUN | | |
| R05 | G4: existing paired UX comparison and correct-target/safety acceptance; no unobserved PASS | NOT_RUN | | |

## Independent conclusions
<!-- docs-section: conclusion -->

Device integration: NOT_RUN. Qualification review: NOT_RUN. Actual installation/recovery: NOT_RUN. Administrator/rehearsal/publication authority: NOT_RUN. Record declared scope, blockers, unobserved cases and responsible follow-up. Preserve failures; source or binary changes require affected revalidation. Device acceptance never grants Stable publication.
