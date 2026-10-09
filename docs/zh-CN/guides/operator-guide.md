<!-- docs-id: operator -->
<!-- docs-lang: zh-CN -->
# 操作指南：Mission Control、Forge 与 Terminal
<!-- docs-section: overview -->

**Language / 语言:** [English](../../guides/operator-guide.md) · [简体中文](operator-guide.md)

本文说明 v1.4 当前开发工作台的操作方式，不宣称已经发布 Stable 或完成真实环境资格。键位以程序内 `?` 为准，CLI 以 `--help` 为准。建议先阅读 [团队快速入门](../team-quickstart.md)。

## 首次会话与目标
<!-- docs-section: first -->

~~~bash
codex-tui --version
codex-tui doctor compat
codex-tui doctor codex
codex-tui
~~~

默认连接本机的 `codex app-server --listen stdio://`。命名目标由本地 TOML 配置，单次可使用 `codex-tui --target NAME`。真实后端失败不会悄悄切到假数据，`--fake` 只用于测试。命名 App Server 的细节见 [英文设计](../../implementation/v1.3-remote-app-server-targets.md)。

## 工作和提醒导航
<!-- docs-section: daily -->

Mission Control 按仓库/项目聚合会话，显示 Needs You / Working / Ready / Inactive。优先处理 Needs You；`j/k` 选中、Enter 打开、`/` 筛选、`Ctrl+K` 命令面板、`?` 查看实时键位；`b` 进入 Board 后用 `h/l` 切换列和 `j/k` 选择卡片。Board/Attention 是派生视图，不是第二套团队任务数据库。

搜索与 Saved View 仅管理个人工作流；未知上游历史不会被虚构。要区分 Windows 路径残影与 Ubuntu 本机会话，应检查仓库 cwd 和 Codex Home。

## 仓库和交付状态
<!-- docs-section: forge -->

在有 Git Remote 的目标仓库执行 `codex-tui doctor git` 和 `codex-tui doctor forge`。内部 GitLab 使用已认证 `glab`，GitHub 使用 `gh`。GitLab/GitHub 某一只读接口失败时，对应能力会降级，其他成功的数据可保留，但 Forge 会明确提示不完整；三个核心接口全部失败时不得报告“最新空结果”。Headless Forge 若缺少核心能力应以降级退出码返回。

MR/PR 创建、评论、批准、合并遵循先预览→明确确认→再执行；Approval/Merge 会重新校验目标修订版本，结果不确定时禁止盲目重试。Review 状态权威在 GitLab/GitHub。见 [真实资格采集](../qualification/provider.md)。

删除受管 Worktree 时，仓库或 cwd 未知的活跃 Codex 会话按潜在冲突保守拒绝；破坏性操作排队过久将使操作范围证据过期，需要重新审阅和确认。Git 命令非零退出会依据实际 Git 状态协调，不假设没有副作用。这不能对独立 Codex 进程建立原子锁，必须继续进行真实并发操作验证。

## 本地语言与可访问性
<!-- docs-section: language -->

配置 `[ui].language = "en"` 或 `"zh-CN"`，或使用 `"auto"` 跟随进程简体语言环境。`presentation = "quiet"` 合并后台刷新；`"screen-reader"` 降低后台刷新频率但保持直接输入即时响应。`[notifications].mode` 默认 `"off"`。这些都只是本地偏好，不会创建团队同步权威。

## Windows SSH Ubuntu 与 Terminal Drawer
<!-- docs-section: ssh -->

通过 Windows SSH 客户端控制 Ubuntu 时，codex-tui **在 Ubuntu 运行**；终端设备、文件路径、Codex、配置、Shell 子进程都应以 Ubuntu 为准。`t` 打开 Drawer，`F6` 或 `Ctrl+]` 在终端支持时返回聚焦。若 Windows/SSH 抢占按键，先查 `?` 与 `doctor terminal`；Resize、Focus、Ctrl+C、退出与终端恢复必须在**真实控制 TTY** 验证。托管 PTY 回归不是该证明。

## 证据与数据安全
<!-- docs-section: safety -->

SQLite 只保存个人元数据和规划覆盖层；Codex 会话、Git、Forge 始终是外部权威。数据损坏时不得用删除数据库代替诊断，先保留 Store/WAL/SHM 并设计离线恢复。`doctor bundle --output ./codex-tui-support` 需复核隐私；Bug 记录必须带精确源码 SHA。TUI 操作不授权 Stable 发布。

