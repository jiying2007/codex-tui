<!-- docs-id: ux-qualification -->
<!-- docs-lang: en -->
# Operator UX qualification: prove usefulness, ease, and safety
<!-- docs-section: overview -->

**Language / 语言:** [English](ux-qualification.md) · [简体中文](../zh-CN/guides/ux-qualification.md)

This is an **operator-run experiment**, not a software CI gate and not permission for Stable publication. Use the exact current candidate SHA and executable digest. Record results privately under the existing [#211 external qualification](https://github.com/jiying2007/codex-tui/issues/211); do not create a second task authority. No data from real users or internal GitLab should be posted publicly.

## Preparation
<!-- docs-section: preparation -->

- Confirm the installed Linux binary's SHA-256 matches the selected signed-off preview package. Run `codex-tui doctor compat`, `doctor codex`, `doctor terminal`, and, only on authorized internal hosts, `doctor forge`. Save the sanitized diagnostics privately.
- Use at least 4 independent repositories, 50 existing Codex threads and 30 *representative tasks* drawn from waiting-for-input/approval, active, completed, failure, review and non-actionable historical states. Test a subset with more than 10k historical rows separately; do not conflate synthetic scale checks with realistic TTY use.
- Compare two tool chains on the **same host, operator, terminal size, repositories and task inventory**: A = upstream Codex CLI plus existing tmux and `glab`/`gh`; B = codex-tui with the same canonical backends.
- Alternate randomized task blocks ABBA/BAAB, distinguishing cold startup from warm navigation. Time from task prompt presentation to the *correct* task context on screen; record incorrect switches and extra steps.
- Run the novice onboarding test with a separate participant who has not read the long operator handbook. Do not tell them shortcuts in advance beyond the one-minute quickstart.
- Before comparing notifications, opt in locally to terminal BEL; verify `printf '\a'` reaches the Windows SSH terminal. Track observed alert delivery, not assumed OS toast. Run in both 60×24 and 100×30 terminals.

## Private observation schema
<!-- docs-section: recording -->

Record each attempt as one CSV row, using only opaque trial IDs, no internal repository names, task content, access tokens or transcripts:

```text
trial_id,chain,scenario,terminal,session_state,duration_ms,correct_target,accidental_mutation,notification_observed,notes
T001,A,needs-you,60x24,warm,12300,true,false,na,opaque-note
T001,B,needs-you,60x24,warm,8400,true,false,na,opaque-note
```

Use `na` for notifications in non-alert trials. Keep first failures. Preserve the original rows and executable digest. Do not edit raw trials after collecting them.

## Acceptance (proposed, not yet observed)
<!-- docs-section: acceptance -->

| Dimension | Target | How to judge |
|---|---|---|
| Time saved | ≥25% reduction in paired median end-to-end time | Report baseline and candidate medians per scenario and sample count; never infer from synthetic CPU benchmarks |
| Safe targeting | ≥99% correct out of at least 100 attempted task jumps | Record all wrong targets, including cases where the UI returned to the wrong view |
| Mutations | 0 accidental approvals, merges, destructive writes or edits | Any event is a blocker, retain receipt and exact repro |
| Find a task | Common Needs You item located within 5 seconds | User-centered measured distribution, not a frame-render timing |
| First use | Basic task inspection/search/switch completed within 10 minutes | Document steps and help usage without coaching |
| Notifications | Actual SSH BEL/visual response and return-navigation measured | Separate observed from missed; no expectation of offline or Windows Toast delivery |
| Stability | Complete existing 46-item device/TTY/Forge matrix and agreed multi-hour real use | The GitHub-hosted 300-second synthetic soak does not meet this bar |

Report p50/p95 of end-to-end interaction time, correct-target fraction, onboarding success, per-scenario failure count and limitations. If a value is not measured, write `NOT_RUN` rather than `PASS`.

## Decision boundary
<!-- docs-section: decision -->

If canonical Codex + tmux + Forge is as easy and fast as codex-tui, keep only cross-repository attention and delivery projection; do not grow the embedded conversation client, a parallel task scheduler, a duplicate agent runtime or a second terminal manager. Prioritize deletion/simplification over additional features.

Real Windows→SSH Ubuntu, authenticated Codex, internal GitLab, sustained operation, administrator/independent reviewer signoff and subsequent Stable `publish=false` dry-run remain open in [#211](https://github.com/jiying2007/codex-tui/issues/211). This guide does not override `stableReady=false` or `publicationAllowed=false`.
