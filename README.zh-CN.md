<!-- docs-id: home -->
<!-- docs-lang: zh-CN -->
# codex-tui — 多仓库 Codex 终端工作台
<!-- docs-section: overview -->

**语言 / Language:** [English](README.md) · [简体中文](README.zh-CN.md)

codex-tui 是面向个人开发者、可供团队复用的**本地优先 Mission Control**。它将不同仓库、Codex App Server、Git 工作树以及 GitLab/GitHub 交付状态投影到一个终端界面；它不替代官方 Codex CLI、Git 或 Forge，也不提供第二套 Agent/任务服务。

**当前状态：** GitHub 曾发布 v1.0.0，但维护者确认 v1.0 和 v1.4 都尚未实际部署。`v1.4.0` 是开发范围完成、等待首次生产验收的候选基线，**不是已发布的 v1.4 Stable**。`stableReady=false`、`publicationAllowed=false`。任何 CI PASS 都不能代替真实 Linux 控制终端、账号和内部 GitLab 资格。

从 [中文文档导航](docs/README.zh-CN.md)、[团队快速入门](docs/zh-CN/team-quickstart.md) 或 [安装说明](docs/zh-CN/release/install-upgrade.md) 开始；完整英文研究、历史设计和证据保存在原路径。

## 开发阶段与发布状态
<!-- docs-section: status -->

v1.4 已冻结新增核心功能，仅接受缺陷、安全、互操作、证据和维护治理修复。v1.2/v1.3 的计划/完成记录是**历史资料**，不再作为 v1.4 的升级兼容约束。首次安装直接创建当前 SQLite v4 状态；不导入未部署版本的 `state-v1.json`，也不迁移旧 SQLite v1–v3。

受保护 `main` 通过的托管 CI、发布预检与产物打包只能证明**自动化工程基线**。真实环境资格与发布授权仍由 [#211](https://github.com/jiying2007/codex-tui/issues/211) 跟踪，详见 [首次部署决策](docs/zh-CN/implementation/v1.4-first-deployment-baseline.md)。

## 核心价值与每日工作
<!-- docs-section: product -->

**一分钟入门：** 打开任务中心，按 `Space` 定位需要处理的会话，按 `/` 搜索，`Enter` 打开；对常用任务按 `=`、数字 `1–9` 绑定，之后可一键切换。按 `?` 查看完整帮助；用 `↑/↓` 或 `j/k` 逐项阅读、`PgUp/PgDn` 翻阅、`Home/End` 到首末项、`Esc` 关闭。小终端优先显示这些高频操作。当前不提供鼠标点击/滚轮导航，因此不再捕获鼠标事件；旧配置 `[ui].mouse` 仍可解析，但该值暂不生效。

Mission Control 优先回答：**正在做什么、哪条会话需要我、如何回到准确的会话、当前代码有何变化？**

- Mission Control 优先显示待处理会话，其次是置顶与最近活动；刷新保留选中会话，最近窗口仍有界，窗口外的置顶与待处理项仍可达。窄终端直接显示待处理原因。
- Board 的 List/Review Queue Saved View 按工作区连续分组；区分 Needs You / Working / Ready / Inactive，并支持筛选、置顶和别名。
- Board/WorkCard 来源于 Codex、Git 和 Forge 的**派生视图**；个人笔记、提醒和规划覆盖层才写入本地 SQLite。
- Git Context 展示分支、脏文件和共享工作树冲突；GitLab/GitHub Forge 展示 Issue、MR/PR、流水线和 Review。外部变更必须计划→确认→执行→回执。
- Thread View、Thread Queue 和 Terminal Drawer 仅作上游兼容维护，不扩大为独立会话/队列/终端管理平台。

`b` 打开 Board，`Tab` 切换 Needs You / Needs Review；`r` 进入所选会话的 Review，`w` 查看 Workspace。Headless 的 Codex 投影视图使用配置中的活动目标或 `--target NAME`，每次只读取一个目标，并明确标记跨目标共享的本地元数据范围。详见[操作指南](docs/zh-CN/guides/operator-guide.md)与 [CLI 参考](docs/zh-CN/guides/cli-reference.md)。

快捷键以应用内 `?` 帮助为准：`Ctrl+K` 命令面板，`/` 搜索，`j/k` 选择、`Enter` 打开，`b` Board，`t` Drawer，`F6` 或 `Ctrl+]` 返回聚焦（取决于实际终端支持）。详见 [操作指南](docs/zh-CN/guides/operator-guide.md)。

**任务提醒需主动启用：** 在 `codex-tui doctor` 打印的本地配置中设置 `[notifications].mode = "terminal"`，在 Windows Terminal 的 SSH Profile 中可设置 `"bellStyle": "all"` 并从 Ubuntu 用 `printf '\a'` 测试。提醒只在 TUI 运行且连通时产生，是终端响铃/闪烁，不是 Windows Toast 或离线推送。不想有提示音时保持默认 `off`。

## 架构和权威边界
<!-- docs-section: architecture -->

~~~text
Codex App Server ── 会话 / Agent / Queue 权威
        │
        ▼
  只读 Registry / Attention ───── Git / Worktree 权威
        │
        ├── Mission Control
        ├── 派生 Board / WorkCard ───── GitLab / GitHub Forge 权威
        └── 本地 SQLite（仅个人 UI / 规划元数据）
        │
        ▼
    Ratatui 终端界面
~~~

本工具不创建云端服务、共享任务数据库、RBAC、通用 Agent 平台或独立的 Forge/Terminal 后端。具体边界参见 [ADR-012 中译](docs/zh-CN/adr/012-upstream-convergence.md)。

## 安装、构建与运行
<!-- docs-section: run -->

需要 **Rust 1.88+**、Git，以及用于真实会话的 `codex` 命令。Forge 集成按目标仓库选择已认证的 `glab`（GitLab）或 `gh`（GitHub）。`glab/gh` 对不使用对应 Forge 的场景不是全局必选项。

~~~bash
git clone https://github.com/jiying2007/codex-tui.git
cd codex-tui
cargo build --release --locked
./target/release/codex-tui --version
./target/release/codex-tui --help
./target/release/codex-tui doctor compat
./target/release/codex-tui doctor codex
./target/release/codex-tui
~~~

Windows 原生环境可以使用对应 Windows 产物；通过 **Windows SSH 登录 Ubuntu** 时，应在 Ubuntu 内运行 Linux 二进制，应用看到的是服务器侧的终端、Codex 和本地仓库。不要把 Windows 本地的路径或会话文件当成 Ubuntu 本地权威。先用 `doctor codex`、`doctor terminal` 排查。

`--fake`、`--fixture-10k` 为测试/诊断用途，不能代表真实账号或 GitLab 的资格通过。完整命令见 [CLI 参考](docs/zh-CN/guides/cli-reference.md)。

## 语言、配置、通知
<!-- docs-section: language -->

在 `codex-tui doctor` 输出的本地配置路径设置：

~~~toml
[ui]
language = "zh-CN"     # auto | en | zh-CN
mouse = false # 未实现鼠标导航前不捕获鼠标事件
presentation = "normal" # normal | quiet | screen-reader

[notifications]
mode = "off"           # off | terminal | os

[app_server]
active = "local"
~~~

`auto` 依次读取 `LC_ALL`、`LC_MESSAGES`、`LANG`。`zh_CN/zh_SG/zh-Hans` 等简体语言环境选中文；繁体语言环境及未知环境默认英文，不假定简体等价。技术标识和上游错误原样保留以便定位。Quiet 和 Screen-reader 只调整后台刷新节奏；输入/键盘/窗口改变仍即时响应。

如需“完成／审批／阻塞／失败”时提醒，主动把 `[notifications].mode` 改为 `"terminal"`。**Windows SSH Ubuntu** 时，通知由 Ubuntu 进程通过当前 SSH 终端发送 BEL；即使配置 `"os"`，SSH 下也安全回退到 BEL，**不会在 Windows 自动弹出 Toast**。Windows Terminal 的 SSH 配置文件可以设置 `"bellStyle": "all"`（任务栏闪烁／响铃），并在 SSH 窗口运行 `printf '\a'` 验证。最多 75 毫秒的同批事件合并为一条提醒，优先显示审批、输入和失败，其余数量写在提醒中；精确状态仍看 Mission Control。程序退出或 SSH 断开后不继续监听。新收到的完成／审批／失败等事件会在终端空间足够时显示约 20 秒单行最近提醒；提示开头先显示 `! 跳转`，窄 SSH 窗口也能看到快捷键；按 `!` 可直接打开关联的 Codex 会话，按 `Esc` 返回原页面。到期事件即使尚未被 UI 定期清除，也不能再通过快捷键打开。Forge/Scratch 更新若无关联会话，仍要到 Board 查看；不会保存通知历史或保证 Windows 桌面弹窗。快捷槽可在 Registry／Board 选中任务按 `=` 再按 `1–9` 绑定；在 Registry、Board、Thread、Review、Workspace、Managed Worktrees 中按数字即直达该会话，编辑和确认框不会被抢键。

远程 Codex 可在本地 TOML 配置命名的 stdio/WebSocket/Unix Socket 目标，令牌只通过 `auth_token_env` 指定环境变量名；不要提交真实密钥。详见英文 [Remote App Server 技术说明](docs/implementation/v1.3-remote-app-server-targets.md)。

## 明确不做的功能
<!-- docs-section: boundaries -->

不建设第二套 Agent 编排、跨 Agent 消息、会话权威、任务/Queue 后端、通用插件平台、tmux 替代品、团队共享数据库、通用跨 Agent API、Web 服务或财务统计平台。上游 Codex 能力增强后，优先精简本地重复层，而不是跟随堆功能。已有兼容界面只进行缺陷和安全维护。

## 验收、故障排查与发布资格
<!-- docs-section: qualification -->

调试顺序：`codex-tui doctor compat`、`doctor codex`、`doctor git`、在目标仓库执行 `doctor forge`、`doctor store`、`doctor terminal`。提供诊断证据时使用 `doctor bundle --output ./codex-tui-support`，并自行复核内容；默认排除环境变量、令牌、提示词和原始错误。详见 [排障指南](docs/zh-CN/guides/troubleshooting.md)。

内部 GitLab 首次部署必须用真正认证过的目标仓库抓取能力 Fixture，再运行 [内部准入检查](docs/zh-CN/qualification/provider.md)。Stable 发布还要求确切 SHA 的 Linux 兼容/TTY/性能证据、严格主线保护、不可变 Releases、审阅发布说明和明确人工授权。自动化演练始终 `publish=false`。详见 [发布说明](docs/zh-CN/guides/release-qualification.md)。

## 许可证与参与贡献
<!-- docs-section: license -->

项目采用 **Apache-2.0**，详见 [LICENSE](LICENSE)。贡献前请阅读 [中文贡献指南](CONTRIBUTING.zh-CN.md)、[安全披露](SECURITY.zh-CN.md) 与 [支持说明](SUPPORT.zh-CN.md)。代码和文档的修订必须经过受保护 PR 与所需门禁；不应直接修改或删除历史 Release/Tag/证据。

