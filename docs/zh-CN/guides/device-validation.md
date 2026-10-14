<!-- docs-id: device-validation -->
<!-- docs-lang: zh-CN -->
# 设备端完整集成验证

**Language / 语言:** [English](../../guides/device-validation.md)

<!-- docs-section: overview -->

本手册适用于 Windows SSH → Ubuntu 目标机 → codex-tui → 本机 Codex App Server 及声明使用的 GitHub/GitLab，不是 MCU 固件验收。本文是待执行操作，不是已完成的设备证据。

开发集成与正式发布验收分开。正式验收前，先安排账号和操作者，再通过受保护 PR 完成发布说明定稿，选定最终精确 SHA。不得采集证据后修改发布日期或升级 Codex，又继续沿用旧 PASS。记录程序实际字节、工具版本、配置哈希及设备／Provider 范围。另见[发布资格](release-qualification.md)。

写操作仅用专用测试仓库。不得重置用户改动、删除 Codex Home／数据库、关闭审批、改用假数据或反复重试未知写结果来取得通过。辅助工具不签发真实人工 PASS，不授予发布权，不执行部署。必要门禁失败即停止，保留原始结果；每次新尝试使用新证据目录。

## 两条 SSH 连接与一个明确候选
<!-- docs-section: identity -->

打开 SSH 窗口 A（交互见证）和 B（观察）。使用实际账号与主机，首次连接经可信渠道核对主机指纹，不关闭校验。首轮先不经过终端复用器；日常用 tmux／screen 时另验一组。本手册的工具已经在仓库内，无需下载 ZIP 或额外安装工具包。

```powershell
# Windows PowerShell: execute in two separate windows.
$Target = Read-Host "SSH user@host"
ssh -V
ssh -tt $Target
```

## 建立私有、绑定源码的会话
<!-- docs-section: workspace -->

在 Ubuntu 窗口 A 进入实际 codex-tui 克隆，先单独执行 `bash`，再执行下列代码。输入本轮已审阅的完整 SHA，不能自动跟随变化的 HEAD。检查拒绝本地或远端不一致，不会代替你 pull、reset 或强制切换。记下打印的目录；窗口 B 只能用真实绝对路径 source 这份可信私有 `session.env`。

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

## 环境、构建与程序身份
<!-- docs-section: build -->

使用日常运行的同一 Ubuntu 用户、HOME、Codex Home 和配置。确认 UTF-8、真实控制终端与合适 TERM；不能靠设置 TERM 虚构终端能力。现有 x86-64 GNU Linux 包的 ABI 基线为 glibc 2.31；ARM／musl 需独立验收。脚本要求 Python 3.8+，声明的 Rust 最低版本为 1.88。验收中不盲目升级全部工具。

GitHub 使用 `gh`，内部 GitLab 档另检查已认证 `glab`。不得全量打印环境、令牌或凭据。将程序固定到私有副本，不覆盖 PATH 中现有安装；本机构建哈希无需与托管包相同。

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

## 设备上的受控回归
<!-- docs-section: controlled -->

执行器只调用现有九套 Linux 夹具，每套一次；任一命令或收据失败即停止，严格核对源码及二进制身份，不赋予实服或真实 SSH 资格。当前源码显式声明 108 场景，包含 300 秒短历史负载；数量变化须审阅，不接受旧收据目录。身份探测使用隔离环境，不调用真实模型账号。每套 900 秒外层保护不放宽夹具内部超时，也不保证任意后代进程都已清理。

失败／中断保留私有日志和失败摘要，不靠重试失败套件变成通过。诊断后新建完整运行；Rust 与工具自测使用原有发现流程，不新增 CI 工作流。

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

## 真实 Codex、多仓与任务闭环
<!-- docs-section: services -->

先在同一用户下用原生 Codex 完成小型只读任务。认证流程以设备所装 `codex --help` 与 `codex login --help` 为准，不公开设备码或认证文件。记录版本、有效配置哈希及实际 Provider，不保存秘密。codex-tui 默认启动本地 stdio App Server，不另起无关守护进程或悄悄切换 target 来掩盖失败。

创建两个含不同 README 标记的专用本地仓库。确认 Cwd 与 repository-backed 会话正确；从 TUI 发起实际任务并检查成功／中断／失败终态，不能只看连接就绪。真正走通审批允许和拒绝、服务端用户输入请求、活跃任务期间导航、取消、草稿／Saved View 持久化和恢复。必须的交互未触发记 NOT_RUN／BLOCKED，不默认通过；模型目录或普通聊天问句不是审批／用户输入协议覆盖。

Git／Worktree 在可丢弃测试对象上验证 diff／分支／路径、受管工作树创建、脏树／活跃删除拒绝及取消，合法删除后 Git 注册与磁盘目录必须同时消失。另验外部编辑器边界和独立 Codex 并发。OutcomeUnknown 必须先核实 Git／Forge 真实状态，不能盲目重放。另见[操作指南](operator-guide.md)。

```bash
"$BINARY" doctor codex
"$BINARY" doctor store
"$BINARY" doctor git
"$BINARY" doctor terminal
cd "$ROOT"
"$BINARY"
```

## 真实控制终端见证
<!-- docs-section: terminal -->

窗口 A 先 prepare，再执行已经固定的程序。terminal_smoke 的 stdin／stdout／stderr 必须连接同一控制 TTY，不接管道，不重定向。prepare 可能打印 cargo 命令；仍用已记录的 BINARY，避免意外重建改变验收对象。

选择有效的本地仓库会话。按 t，核对 pwd，执行 `echo CODEX_TUI_DRAWER_SMOKE`；改变窗口宽和高；执行 `sleep 30` 并在期间按 Ctrl+C，再执行 `echo CODEX_TUI_AFTER_CTRL_C`。F6 返回 TUI 焦点，t 重聚焦 Drawer；F6 后大写 T 关闭。检查导航、中文、滚动、窄窗口及正常退出，核对光标、回显、Backspace 和行编辑。Ctrl+] 只是终端支持时的备选，键位以实时 `?` 为准。

完成规定异常退出范围后才可记录 PASS。prepare 与 record 必须同一 TTY，SSH 重连须另建见证会话。不承诺存在安全 --panic 接口，信号测试不能冒充 panic 测试。

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

## 内部 GitLab 与声明的 Forge 写操作
<!-- docs-section: forge -->

进入已授权、有代表性的测试项目，对实际主机完成 glab 登录，再在该项目执行 doctor forge。codex-tui 的 SOURCE_SHA 不是业务仓库 HEAD。以下现有链保留源码／认证／能力／时效／隐私要求，包括七天时效和五分钟未来偏差。退出 3 的诊断是失败证据，不是资格通过；不修改时间戳，不覆盖旧结果。

只读能力采集不证明写操作。专用对象上另行见证评论取消／确认（零／一次写入）、审批界面完整修订、远端 HEAD 变化后拒绝、允许的测试 MR 合并、无权限及未知结果协调。遵循平台审批策略，不绕过作者限制。声明需要 GitHub 时，再用 gh 复验对应范围。另见[Provider 资格](../qualification/provider.md)。

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

## 一致性备份与隔离恢复
<!-- docs-section: state -->

记录 doctor store 的实际路径，先验个人元数据保存和单写者行为。辅助程序以只读方式打开 SQLite，使用在线备份 API、验证完整性，并将同目录暂存副本以不覆盖已有最终文件的方式发布；它不是新增产品恢复／迁移命令。备份、完整性与摘要计算有期限检查，但这里不对系统 I/O 或 fsync 承诺通用硬期限。

不得只复制正在写入的数据库主文件，也不得损坏真实库。WAL 协调可能使用共享内存文件；只读数据库连接不等于文件系统零副作用。只把已验证快照放入全新隔离数据／配置目录演练，不能覆盖在线库。保留失败镜像，遵循[安装与恢复说明](../release/install-upgrade.md)。这是首次部署，不虚构 v1.0–v1.3 生产升级。

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

## 真实使用、进程采样与故障
<!-- docs-section: observation -->

运行前约定时长与任务范围，例如覆盖一个工作日后再有限试用 3～5 天；这是建议安排，不是新增发布阈值或已经完成的观察。用正常工作，不为复现 32 会话合成负载制造额外付费调用。

使用前面的 PID 代码块启动固定 TUI。窗口 B 加载本轮私有 session.env 后执行下列只读采样。采样器核对在线进程镜像与程序文件，跟踪 PID／启动时间；sourceSha 明确仍为操作者断言（sourceShaIndependentlyVerified=false），先前 doctor 记录承担独立源码核验，两者一起保留。

读取不到的资源值记未知，不记零。主进程提前退出、镜像变化、人工中断或不完整采样返回 3，不可表述为完成观察。计数仅覆盖实际观测的同 UID 后代，包括已观测后被重新托管的子进程；未采到的短命进程和独立共享守护进程不在范围内。家族 RSS 求和不是去重物理内存。分别复核根进程和家族趋势，工具不判资源或实服 PASS。

故障试验只用空闲专用会话。先人工对照本轮 PID、进程启动时间和 /proc/PID/exe，再明确选择对该进程发 SIGTERM；不得 pkill 全部 Codex。SIGTERM 不等于 SIGKILL 或真实 panic。分别验 SSH 重连、专属 App Server 故障和 Forge 拒绝。若需 stty sane／恢复光标，先将自动恢复记录为失败，再应急处理。

```bash
python3 "$ROOT/scripts/release/sample_device_process.py" \
  --pid "$(cat "$EVIDENCE/real/tui.pid")" --binary "$BINARY" \
  --source-sha "$SOURCE_SHA" --seconds 28800 --interval 5 \
  --output "$EVIDENCE/soak/processes.jsonl"
```

## 真实见证后采集最终证据
<!-- docs-section: qualification -->

只有所有必需终端观察真实通过后，才能在窗口 A、同一控制 TTY 执行以下第一段。SSH 已重连则重新 prepare 和见证，不能在另一 TTY 签旧 pending。--pass 故意保持人工执行，不属于无人值守辅助程序；必需项未观察则阻塞。

最终采集仍用新输出路径。只有正式候选才执行不带 --dispatch 的 linux_qualify；该脚本会重建，结束后核对程序是否与见证副本相同。字节变化须重做受影响证据，不能只信生成 JSON 的 valid。此时发布说明必须已经合入。管理员保护／不可变设置、独立签核、精确候选 Stable publish=false 演练和另行授权发布仍不由本设备执行器代办。

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

## 复核、归档与停止
<!-- docs-section: closure -->

把[人工矩阵](device-matrix.md)复制到私有证据目录；所有项目从 NOT_RUN 开始，记录实际结果、证据路径和复核者。分别判受控夹具、设备集成、发布资格和安装／回退，不能合并为一个笼统 PASS。转移私有归档前核对摘要，对外分享前审查元数据和日志；令牌、原始提示词及内部仓库信息不能进入公共仓库。

源码、程序或配置变化使受影响证据需重做，不能改写原记录。失败保留，新尝试新目录；采样／备份退出 0 不代表人工资格。既定自动范围通过后，真实凭据、TTY／长期使用见证和管理员／所有者决定须等真实输入；不为增加提交反复改动已验收源码。#211 是外部交接，不是授权旁路。

