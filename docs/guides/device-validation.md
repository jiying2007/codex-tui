<!-- docs-id: device-validation -->
<!-- docs-lang: en -->
# Device integration validation

**Language / 语言:** [简体中文](../zh-CN/guides/device-validation.md)

<!-- docs-section: overview -->

This runbook covers Windows SSH → an Ubuntu machine → codex-tui → local Codex App Server and the declared GitHub/GitLab providers. It is not MCU firmware qualification. These are instructions, not completed device evidence.

Development integration and final release acceptance are distinct. Before final acceptance, arrange accounts and operators, finalize release notes through a protected PR, and select the resulting exact SHA. Do not date the release or upgrade Codex after collecting evidence and silently keep the old PASS. Record binary bytes, tool versions, configuration hashes and the declared device/provider scope. See [release qualification](release-qualification.md).

Use dedicated test repositories for writes. Never reset a user's worktree, delete their Codex home/database, disable approvals, switch to fake data, or repeatedly retry an uncertain write to obtain PASS. No helper records human PASS, grants publication authority or runs a deployment. Stop on a failed required gate; keep its original output and use a fresh evidence directory for each new attempt.

## Two SSH windows and one explicit candidate
<!-- docs-section: identity -->

Open SSH windows A (interactive witness) and B (observation). Use your actual account and host; confirm the first host-key fingerprint through a trusted channel. Do not disable host-key verification. First test without a multiplexer; test tmux/screen separately when used daily. The helpers now live in the repository: no downloaded ZIP or separately installed kit is required.

```powershell
# Windows PowerShell: execute in two separate windows.
$Target = Read-Host "SSH user@host"
ssh -V
ssh -tt $Target
```

## Create a private, source-bound session
<!-- docs-section: workspace -->

In Ubuntu window A, enter the actual codex-tui source clone and start a separate `bash`. Execute the following in that shell. Enter the full SHA explicitly approved for this run; do not automatically follow a changing HEAD. The guard refuses stale local/remote sources instead of pulling, resetting or force-checking out. Record the printed directory. In window B, source only this trusted private `session.env`, using its actual absolute path.

```bash
set -euo pipefail
umask 077
export PYTHONDONTWRITEBYTECODE=1
export ROOT="$(pwd -P)"
export REPO=jiying2007/codex-tui
read -r -p 'Reviewed candidate SHA (40 lowercase hex): ' SOURCE_SHA
export SOURCE_SHA
[[ "$SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]]
test "$(git branch --show-current)" = main
test -z "$(git status --porcelain)"
git fetch origin main
test "$(git rev-parse HEAD)" = "$SOURCE_SHA"
test "$(git rev-parse origin/main)" = "$SOURCE_SHA"
test "$(git ls-remote origin refs/heads/main | cut -f1)" = "$SOURCE_SHA"
export EVIDENCE="$(mktemp -d "$HOME/codex-tui-device-${SOURCE_SHA:0:8}.XXXXXX")"
export BINARY="$EVIDENCE/bin/codex-tui"
mkdir "$EVIDENCE/bin" "$EVIDENCE/preflight" "$EVIDENCE/real" \
      "$EVIDENCE/provider" "$EVIDENCE/store" "$EVIDENCE/soak" "$EVIDENCE/faults"
printf '%s\n' "$SOURCE_SHA" > "$EVIDENCE/source-sha.txt"
git rev-parse HEAD^{tree} > "$EVIDENCE/source-tree.txt"
for v in ROOT REPO SOURCE_SHA EVIDENCE BINARY; do
  printf 'export %s=%q\n' "$v" "${!v}"
done > "$EVIDENCE/session.env"
printf 'Evidence: %s\n' "$EVIDENCE"
```

## Environment, build and executable identity
<!-- docs-section: build -->

Use the same Ubuntu user, HOME, Codex home and configuration as normal operation. Confirm UTF-8, a real controlling terminal and an appropriate TERM; do not invent terminal capability by setting TERM. The supported packaged x86-64 GNU Linux ABI baseline is glibc 2.31; ARM/musl requires separate qualification. Scripts need Python 3.8+, and the declared Rust minimum is 1.88. Do not upgrade all tools during acceptance.

Check `gh` for GitHub and authenticated `glab` for the internal GitLab profile separately. Never dump the entire environment, tokens or credentials. Build into a private pinned copy instead of overwriting a PATH installation. A locally built binary need not hash identically to a hosted package.

```bash
test "$(uname -s)" = Linux
test -t 0 && test -t 1 && test -t 2
tty
printf 'TERM=%s\n' "$TERM"
locale charmap
stty size
for tool in git python3 cargo rustc codex; do command -v "$tool"; done
{
  date -u +%FT%TZ
  uname -a
  cat /etc/os-release
  getconf GNU_LIBC_VERSION
  git --version
  python3 --version
  rustc -Vv
  cargo -Vv
  codex --version
} > "$EVIDENCE/preflight/versions.txt" 2>&1
python3 -c 'import sys; raise SystemExit(0 if sys.version_info >= (3,8) else 1)'
cd "$ROOT"
CODEX_TUI_GIT_SHA="$SOURCE_SHA" cargo build --release --locked \
  > "$EVIDENCE/preflight/build.log" 2>&1
install -m 700 target/release/codex-tui "$BINARY"
sha256sum "$BINARY" > "$EVIDENCE/binary-before.sha256"
"$BINARY" --version
"$BINARY" --help > "$EVIDENCE/preflight/codex-tui-help.txt"
if "$BINARY" doctor compat --json > "$EVIDENCE/preflight/compat.json" \
  2> "$EVIDENCE/preflight/compat.stderr"; then rc=0; else rc=$?; fi
printf '%s\n' "$rc" > "$EVIDENCE/preflight/compat.exit"
python3 - "$EVIDENCE/preflight/compat.json" "$SOURCE_SHA" <<'CHECK'
import json, sys
with open(sys.argv[1], encoding='utf-8') as f: report = json.load(f)
if not isinstance(report, dict) or report.get('sourceSha') != sys.argv[2]:
    raise SystemExit('STOP: executable source mismatch')
print('readiness:', report.get('readiness'))
CHECK
test "$rc" -eq 0
```

## Device-local controlled regression
<!-- docs-section: controlled -->

The driver invokes the nine existing Linux fixture suites once, stops at the first failed command or receipt, validates exact source/binary identity and emits no real-service or real-SSH authority. The current source declares 108 scenarios including a 300-second short-history workload. Count changes require review; a directory full of older receipts is refused. Identity probing is isolated, not a call to your live model account. The 900-second outer per-suite watchdog does not relax any fixture timeout and is not a universal descendant cleanup guarantee.

Failed/incomplete runs retain private logs and a failed summary; they do not become PASS by retrying the failed suite. Run a new complete attempt after diagnosis. Rust and tool self-tests use existing discovery, not a new CI workflow.

```bash
cd "$ROOT"
python3 scripts/release/run_device_regressions.py \
  "$ROOT" "$BINARY" "$SOURCE_SHA" "$EVIDENCE/controlled-native"
python3 -m unittest discover -s scripts/release -p 'test_*.py' \
  > "$EVIDENCE/preflight/release-tests.log" 2>&1
python3 -O -m unittest discover -s scripts/release -p 'test_*.py' \
  > "$EVIDENCE/preflight/release-tests-optimized.log" 2>&1
CODEX_TUI_GIT_SHA="$SOURCE_SHA" cargo test --locked --all-targets --all-features \
  > "$EVIDENCE/preflight/cargo-test.log" 2>&1
```

## Real Codex, multiple repositories and task completion
<!-- docs-section: services -->

Before the TUI, complete a small read-only task with native Codex under this same user. Use the installed `codex --help` and `codex login --help` for the configured authentication flow; never publish a device code or auth file. Record versions, effective configuration hashes and the real provider without secrets. Default codex-tui starts its local stdio App Server; do not start an unrelated daemon or silently select another target to hide failure.

Create two dedicated local repositories containing distinct README markers. Verify correct Cwd and repository-backed threads; send real work from the TUI and check the actual final success/interruption/failure, not only connection readiness. Exercise an actual approval both allowed and refused, an actual server user-input request, navigation while a task is active, cancellation, draft/Saved View persistence and reopening. Not observing a required interaction is NOT_RUN/BLOCKED, never PASS. Public model metadata or a chat question alone is not approval/user-input protocol coverage.

For Git/Worktree, inspect real diff/branch/path, create a dedicated managed worktree, verify dirty/active deletion refusal and cancellation, and confirm both Git registration and directory disappearance after an allowed deletion. Test external-editor boundaries and independent Codex concurrency only on disposable test objects. An OutcomeUnknown write must be reconciled with Git/Forge before deciding whether any new action is safe. See the [operator guide](operator-guide.md).

```bash
"$BINARY" doctor codex
"$BINARY" doctor store
"$BINARY" doctor git
"$BINARY" doctor terminal
cd "$ROOT"
"$BINARY"
```

## Real controlling-TTY witness
<!-- docs-section: terminal -->

In window A, prepare and run the exact pinned executable below. Do not pipe or redirect terminal_smoke: stdin/stdout/stderr must all be on the same controlling TTY. The prepare helper may print a cargo command; use the already recorded BINARY so an unintended rebuild does not change the test object.

Select a valid local repository-backed thread. Press t, verify pwd and `echo CODEX_TUI_DRAWER_SMOKE`; resize both dimensions; run `sleep 30`, press Ctrl+C during it, then run `echo CODEX_TUI_AFTER_CTRL_C`. F6 returns focus to TUI, t refocuses the Drawer; F6 then uppercase T closes it. Check navigation, Chinese text, scrolling, narrow windows and normal exit. Verify cursor, echo, Backspace and line editing. Ctrl+] is only a terminal-dependent alternative. Keys follow live `?` help.

Complete the required abnormal-exit scope separately before recording PASS. The same TTY is required for prepare and record; SSH reconnect creates a new witness session. No safe --panic CLI is promised, and a signal test is not a panic test.

```bash
cd "$ROOT"
python3 scripts/release/terminal_smoke.py prepare \
  --pending "$EVIDENCE/real/terminal-pending.json" \
  --terminal-label "windows-ssh:${TERM}:$(tty)"
(
  printf '%s\n' "$BASHPID" > "$EVIDENCE/real/tui.pid"
  exec "$BINARY"
)
```

## Internal GitLab and declared Forge writes
<!-- docs-section: forge -->

Enter an authorized representative test project, authenticate glab on its actual host, and check doctor forge there. codex-tui SOURCE_SHA is not the business repository HEAD. The existing capture and admission chain below retains source/authentication/capability/age/privacy gates, including seven-day age and five-minute future skew limits. An exit-3 diagnostic fixture is failed evidence, not qualification. Do not edit its timestamp or overwrite a previous result.

Read capability capture does not qualify writes. On dedicated test objects, separately witness comment cancel/confirm (zero/one write), approval of the displayed full revision, refusal after remote HEAD changes, permitted test MR merge, permission refusal and uncertain-outcome reconciliation. Honor platform approval policy; do not bypass author restrictions. Repeat for GitHub using gh only when declared in scope. See [provider qualification](../qualification/provider.md).

```bash
read -r -p 'Internal GitLab host: ' GITLAB_HOST
read -r -p 'Authorized test repository absolute path: ' GITLAB_REPO
cd "$GITLAB_REPO"
git rev-parse --show-toplevel
glab auth status --hostname "$GITLAB_HOST"
# If needed, authenticate interactively in this trusted terminal before continuing.
"$BINARY" doctor forge
python3 "$ROOT/scripts/release/capture_forge_capability.py" \
  --binary "$BINARY" --expected-provider gitlab --expected-source-sha "$SOURCE_SHA" \
  --require-authenticated --required-capability issues=available \
  --required-capability merge-requests=available --required-capability pipelines=available \
  --output "$EVIDENCE/provider/gitlab.json"
python3 "$ROOT/scripts/release/validate_internal_gitlab.py" \
  --fixture "$EVIDENCE/provider/gitlab.json" --source-sha "$SOURCE_SHA" \
  --output "$EVIDENCE/provider/gitlab-admission.json"
```

## Consistent backup and isolated recovery
<!-- docs-section: state -->

Record the actual doctor store path. Test saved personal metadata and the single-writer behavior first. The helper opens SQLite read-only and uses the online backup API, verifies integrity and publishes a staged same-directory copy without overwriting an existing final file. It is not a new product restore/migration command. The backup/decode/digest work has deadline checks; operating-system I/O or fsync cannot be given a universal hard deadline here.

Do not copy a live database main file alone or corrupt the real store. WAL coordination may use shared-memory files; read-only database opening is not a promise of zero filesystem side effects. Restore only the verified snapshot into a fresh isolated trial data/config directory, never over the live database. Preserve failed images and follow [installation and recovery](../release/install-upgrade.md). This is first deployment, not a fabricated v1.0–v1.3 production migration.

```bash
read -r -p 'Actual SQLite path from doctor store: ' STORE_DB
python3 "$ROOT/scripts/release/backup_device_sqlite.py" \
  --database "$STORE_DB" --output "$EVIDENCE/store/backup.sqlite3" \
  > "$EVIDENCE/store/backup-receipt.json"
RECOVERY="$(mktemp -d "$HOME/codex-tui-recovery.XXXXXX")"
mkdir -p "$RECOVERY/data/codex-tui" "$RECOVERY/config"
cp "$EVIDENCE/store/backup.sqlite3" "$RECOVERY/data/codex-tui/state-v2.sqlite3"
(
  export XDG_DATA_HOME="$RECOVERY/data" XDG_CONFIG_HOME="$RECOVERY/config"
  "$BINARY" doctor store > "$EVIDENCE/store/recovery-doctor.txt" 2>&1
)
```

## Actual usage, process sampling and faults
<!-- docs-section: observation -->

Agree the duration and workload before running, for example one working day followed by 3–5 days of limited trial. These are recommended arrangements, not new release thresholds or completed observations. Use normal tasks; do not create paid model traffic merely to reproduce the 32-thread synthetic load.

Start the pinned TUI with the PID block above. In window B, load this run's private session.env and invoke the read-only sampler below. It verifies the live executable image against the binary file and tracks PID/start-time identity. Its sourceSha remains an explicit operator assertion (sourceShaIndependentlyVerified=false); the earlier doctor evidence provides independent source verification. Keep both records together.

Missing process metrics are unknown, not zero. Early root exit/image change/interruption or incomplete samples return 3 and must not be presented as a completed observation. Raw counters cover observed same-UID descendants, including observed reparented children; short-lived missed processes and unrelated shared daemons remain outside scope. Family RSS sums are not unique physical memory. Review root and family trends separately; the tool never declares resource or live-service PASS.

For fault tests, use an idle dedicated test session. First compare the PID, process start time and /proc/PID/exe with the recorded session, then consciously choose SIGTERM for only that process. Never pkill all Codex processes. A SIGTERM test is not SIGKILL or a true panic test. Observe SSH disconnect/reconnect, dedicated App Server failure and Forge refusal separately. If manual stty sane/cursor recovery is necessary, record automatic restoration as failed before recovery.

```bash
python3 "$ROOT/scripts/release/sample_device_process.py" \
  --pid "$(cat "$EVIDENCE/real/tui.pid")" --binary "$BINARY" \
  --source-sha "$SOURCE_SHA" --seconds 28800 --interval 5 \
  --output "$EVIDENCE/soak/processes.jsonl"
```

## Witness then collect final evidence
<!-- docs-section: qualification -->

Only after every required terminal observation really passes, in window A on the SAME controlling TTY, run the first block below. If SSH reconnected, prepare and witness anew; never sign the old pending manifest on another TTY. The --pass block is intentionally manual, not part of any unattended helper. Unobserved mandatory cases block it.

Use fresh destinations for final capture. `terminal_smoke.py` and `create_terminal_receipt.py` refuse existing pending/receipt outputs and symbolic links, including dangling links. Keep the previous attempt and start a fresh evidence directory instead of deleting an old record to reuse its name. For final release qualification only, run linux_qualify without --dispatch. It rebuilds, so compare its binary with the witnessed copy afterwards. A changed binary needs renewed affected evidence even if a generated JSON says valid. Final release notes must already be committed. Administrator protection/immutability readback, independent signoff, exact-candidate Stable publish=false rehearsal, and separately authorized publication remain outside this device runner.

```bash
# MANUAL ONLY: after actual witness, on the same controlling TTY.
cd "$ROOT"
sha256sum -c "$EVIDENCE/binary-before.sha256"
python3 scripts/release/terminal_smoke.py record-pass \
  --pending "$EVIDENCE/real/terminal-pending.json" \
  --output "$EVIDENCE/real/terminal-linux.json" --pass
python3 scripts/release/capture_compat.py --binary "$BINARY" \
  --expected-source-sha "$SOURCE_SHA" --output "$EVIDENCE/real/compat-final.json"
"$BINARY" doctor bundle --output "$EVIDENCE/real/support-final"

# FINAL CANDIDATE ONLY; not a deployment or publishing command.
python3 scripts/release/linux_qualify.py --github-repo "$REPO" \
  --terminal-receipt "$EVIDENCE/real/terminal-linux.json" \
  --output-dir "$EVIDENCE/linux-qualification" \
  > "$EVIDENCE/linux-qualification.log" 2>&1
cmp "$BINARY" "$ROOT/target/release/codex-tui"
sha256sum -c "$EVIDENCE/binary-before.sha256"
```

## Review, archive and stop
<!-- docs-section: closure -->

Copy the [manual matrix](device-matrix.md) into the private evidence directory. All entries start NOT_RUN; each needs actual outcome, evidence path and reviewer. Distinguish controlled fixture results, device integration, release qualification and installation/recovery. Check private archive digests before transfer and inspect metadata/logs before sharing; do not upload tokens, raw prompts or internal repository details to the public repository.

Source/binary/configuration changes invalidate affected evidence, not the original records. A failure is preserved and a new attempt uses a fresh directory. Do not interpret exit 0 from a sampler/backup as human qualification. Once the declared automated scope passes, leave real credentials, TTY/long-use witness and administrator/owner decisions blocked until genuinely supplied. Do not keep changing accepted code merely to generate commits. #211 remains the external handoff, not an authorization bypass.

