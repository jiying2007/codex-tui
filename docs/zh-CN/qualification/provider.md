<!-- docs-id: provider -->
<!-- docs-lang: zh-CN -->
# Forge Provider 资格与内部 GitLab 准入
<!-- docs-section: overview -->

**语言 / Language:** [English](../../qualification/provider.md) · [简体中文](provider.md)

Forge 能力以真实观测结果为准，不能根据 GitLab/GitHub 的软件版本号推断。公共 Stable 可以在不使用 GitLab 的条件下运行，但**内部 GitLab 使用档**需单独提交实际仓库认证与能力证据。以下命令只是采集流程，不会自动授予 Stable 发布权限。

## 准备源码、构建与命令环境
<!-- docs-section: requirements -->

在 **codex-tui 源码仓库**中生成确切源码 SHA 并构建，注意这不同于目标业务仓库的 Git HEAD：

~~~bash
cd /path/to/codex-tui
SOURCE_DIR="$(pwd)"
SOURCE_SHA="$(git rev-parse HEAD)"
CODEX_TUI_GIT_SHA="$SOURCE_SHA" cargo build --release --locked
BINARY="$SOURCE_DIR/target/release/codex-tui"
~~~

在目标 GitLab 仓库使用已认证的 `glab`，目标 GitHub 仓库使用 `gh`。`BINARY` 必须来自上述源码，目标仓库的 `git rev-parse HEAD` **不是** codex-tui 的源码身份。脚本运行输出的诊断信息不复制令牌、Prompt、评论正文或原始错误。

## 真实内部 GitLab 能力采集
<!-- docs-section: gitlab -->

先进入**真实、有代表性的内部 GitLab 仓库**：

~~~bash
cd /path/to/internal-gitlab-repository
python3 "$SOURCE_DIR/scripts/release/capture_forge_capability.py" \
  --binary "$BINARY" \
  --output "$SOURCE_DIR/release/evidence/provider/gitlab.json" \
  --expected-provider gitlab \
  --expected-source-sha "$SOURCE_SHA" \
  --require-authenticated \
  --required-capability issues=available \
  --required-capability merge-requests=available \
  --required-capability pipelines=available
~~~

在资格夹具中可以记录能够观测到的客户端/服务端版本、版型、功能和项目对象数量，但**功能状态本身才是准入依据**。Issue Board 等延后能力只有项目确实依赖时才应增加门槛。采集过程最长 45 秒，失败时拒绝输出原始 stdout/stderr，不能把模拟 CI Fixture 当作真实验收。

## GitHub 只读能力采集
<!-- docs-section: github -->

进入真实 GitHub 仓库后使用同一构建产物：

~~~bash
cd /path/to/github-repository
python3 "$SOURCE_DIR/scripts/release/capture_forge_capability.py" \
  --binary "$BINARY" \
  --output "$SOURCE_DIR/release/evidence/provider/github.json" \
  --expected-provider github \
  --expected-source-sha "$SOURCE_SHA" \
  --require-authenticated \
  --required-capability issues=available \
  --required-capability merge-requests=available \
  --required-capability pipelines=available
~~~

这是**只读能力**的资格采集，不授权 PR 评论、审批、合并等写操作；写操作仍需单独预览、确认和来源 HEAD 校验。

## 证据结构与隐私
<!-- docs-section: fixture -->

输出 `codex-tui/forge-capability-fixture/v1`，绑定产品/源码 SHA、操作系统、Provider、认证与能力状态、观测时间和故障类别；不得保存环境变量、认证令牌、内部原始 Remote URL、Prompt/转录、评论正文和原始 stderr。

不匹配时 `capture_forge_capability.py` 仍可能生成 `qualified=false` 的回执，并以非零退出；这仅说明收集到了**失败证据**，不是资格通过。不要将带有内部主机或组织敏感元数据的 Fixture 随意公开。

## 内部首次部署准入（额外门禁）
<!-- docs-section: internal -->

从仍保存的**codex-tui 源码 SHA**对先前真实采集的 GitLab Fixture 运行：

~~~bash
python3 "$SOURCE_DIR/scripts/release/validate_internal_gitlab.py" \
  --fixture "$SOURCE_DIR/release/evidence/provider/gitlab.json" \
  --source-sha "$SOURCE_SHA" \
  --output "$SOURCE_DIR/release/evidence/provider/internal-gitlab-admission.json"
~~~

此步骤拒绝错误 SHA、非 Linux、未登录的 `glab`、任何必须的 Issue/MR/Pipeline 不可用或未明确声明、观测时间超过 7 天、向未来偏差超出 5 分钟及缺少隐私约束。回执绑定输入原始字节的 SHA-256，不覆盖已有结果。

仍须独立取得当前 Codex 真实账号、Linux/Windows→SSH Ubuntu 控制终端、GitHub 管理员保护配置、精确候选证据与人工发布授权；该准入**不自动触发正式发布**。

