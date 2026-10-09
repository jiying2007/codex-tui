<!-- docs-id: team-quickstart -->
<!-- docs-lang: zh-CN -->
# 团队快速入门（个人优先）
<!-- docs-section: overview -->

**语言 / Language:** [English](../team-quickstart.md) · [简体中文](team-quickstart.md)

codex-tui 主要给个人开发者使用；团队复用依赖仓库内的规则和现有 GitLab/GitHub，而非共享数据库或团队服务器。若尚未部署，请按 [首次部署基线](implementation/v1.4-first-deployment-baseline.md) 从干净状态开始。

## 1. 安装和自检
<!-- docs-section: install -->

从可信的精确 SHA GitHub Actions 产物安装匹配平台的原生二进制，并校验 Release Bundle 的 `SHA256SUMS`。**v1.4 Stable 尚未发布**，目前不应把开发预览包装成正式发布。

~~~bash
codex-tui --version
codex-tui doctor compat
codex-tui doctor codex
~~~

`doctor` 输出本地配置/状态路径；每个开发者独立管理 SQLite，不应提交进 Git 仓库。SSH 进入 Ubuntu 时，在 Ubuntu 使用 Linux 版本、Ubuntu 的 `codex` 和 `git`。

## 2. 团队规范的权威位置
<!-- docs-section: authority -->

- `AGENTS.md`：仓库级 Agent 工作约定（若团队需要）。
- `.codex/config.toml`：Codex 项目配置。
- Git/GitLab/GitHub：分支、审阅、CI、Issue/MR/PR 的唯一权威。
- codex-tui：投影视图、提醒、个人笔记及安全确认；不拥有共享任务数据。

## 3. 可选安全启动预设
<!-- docs-section: presets -->

在仓库根目录使用受限制的 `.codex-tui.toml`：

~~~toml
version = 1

[[launch]]
name = "Tests"
argv = ["cargo", "test", "--all-targets"]
cwd = "repo"

[[launch]]
name = "Open workspace"
argv = ["code", "."]
cwd = "thread"
~~~

验证但不执行：`codex-tui doctor presets`。不接受任意 Shell 命令字符串、模板环境变量、链式执行、Hooks 或调度器。启动时需检查具体 cwd/argv 并确认。

## 4. GitLab / GitHub
<!-- docs-section: forge -->

内部 GitLab 项目先在实际仓库登录 `glab`，GitHub 则登录 `gh`，然后：

~~~bash
codex-tui doctor forge
~~~

版本号不能代替能力探测。正式内部部署必须从当前 SHA 的真实仓库采集 [能力证据与准入回执](qualification/provider.md)；单纯 `doctor compat` 报 Ready **不表示**内部 GitLab 已通过。

## 5. 每日工作流
<!-- docs-section: daily -->

`Ctrl+K` 打开面板，`/` 搜索，`j/k` 导航并 `Enter` 进入；`b` 查看 Board，`h/l` 切换列，`t` 打开 Terminal Drawer；按 `F6`（或终端支持时 `Ctrl+]`）将控制权返回 codex-tui。`?` 为键盘说明的权威来源，若按键与终端冲突，以实时帮助和 `doctor terminal` 为准。

先检查 Needs You 的人工确认，再观察 Working/Ready；对 MR/PR 的写操作始终逐项预览并确认。

## 6. 诊断顺序
<!-- docs-section: troubleshooting -->

~~~bash
codex-tui doctor compat
codex-tui doctor store
codex-tui doctor forge
codex-tui doctor terminal
codex-tui doctor bundle --output ./codex-tui-support
~~~

诊断包为元数据证据，默认不会存放原始令牌、Prompt 或评论正文；分享前仍应复核。损坏数据库先停机保留文件与备份，不能靠反复删除 SQLite 伪装恢复成功。参见 [排障手册](guides/troubleshooting.md)。

## 明确排除项
<!-- docs-section: boundaries -->

不建设团队 RBAC 服务、第二套 Registry 数据库、未经实测就替换 `glab` 的原生 GitLab REST/GraphQL、通用 Agent 插件平台或 tmux 级终端管理。团队规模化应先加强当前权威的可观测性与兼容性。

