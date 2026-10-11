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

搜索与 Saved View 仅管理个人工作流；未知上游历史不会被虚构。全文搜索使用短生命周期独立 Codex App Server 连接，不占用主审批/会话 Actor；只允许一个搜索，新请求取消旧请求，总体 12 秒超时会明确提示。单 Thread 匹配项分页未耗尽时不得标记全部完成。若等待中的 RPC 收到服务端审批请求，主 Actor 会优先分派已排队的审批事件，再处理其他命令或周期探测；被中断的 RPC 结果仍为未知，不自动重放。本地索引的短查询会将 `%`、`_` 和反斜杠按字面字符搜索，而不是 SQLite 通配符。要区分 Windows 路径残影与 Ubuntu 本机会话，应检查仓库 cwd 和 Codex Home。上游清除项目归属或标题后，先用安全的 cwd/会话 ID 临时显示并触发合并后的权威重读；解归档也通过既有有界刷新恢复，不凭通知虚构会话。App Server 的 Git Origin 仅保留剔除凭证、查询和片段后的 host/repository 身份，无法安全解析时退回 cwd，不将原始 URL 带入搜索、Headless 或界面。

## 仓库和交付状态
<!-- docs-section: forge -->

在有 Git Remote 的目标仓库执行 `codex-tui doctor git` 和 `codex-tui doctor forge`。内部 GitLab 使用已认证 `glab`，GitHub 使用 `gh`。GitLab/GitHub 某一只读接口失败时，对应能力会降级，其他成功的数据可保留，但 Forge 会明确提示不完整；三个核心接口全部失败时不得报告“最新空结果”。Headless Forge 若缺少核心能力应以降级退出码返回。

Forge 概览仅获取各能力最近 20 条记录。GitHub Issues API 会混入 PR，系统在过滤 PR 前保留原始分页是否触顶的证据；因此即使展示 0 条 Issue，只要原始页已满，也不得断言不存在。当前分支的 CR/Pipeline 未命中**不代表不存在**；Board 数据来源与 Review 会提示该不确定性，精确结果仍应向平台核实。MR/PR 创建、评论、批准、合并遵循先预览→明确确认→再执行；Approval/Merge 会重新校验目标修订版本，结果不确定时禁止盲目重试。Review 状态权威在 GitLab/GitHub。Forge Merge 要求明确可合并状态和经验证的审批信息；GitHub 合并性未知、GitLab 审批数量缺失或详细状态未知时必须阻止执行。创建/审批的前置检查使用有界完整分页，不把第一页当作全部数据，服务端仍为最终权威。GitHub Fork PR 与 Actions 工作流均使用 `owner/repo:branch` 等完整源仓库标识展示，不得仅凭同名分支绑定到本地 Git 分支。工作流若缺少 `head_repository`，标记为 `unverified-source:branch`，不能将其失败误归属本地分支。与本地分支绑定的审批、评论和合并需要 PR 源仓库 ID 与路径均匹配；缺少来源信息时保守拒绝。GitLab 跨项目 MR 也会以 `project/<id>:branch` 标记来源，不能冒充本地分支；与本地分支绑定的写操作要求 `source_project_id` 和 `target_project_id` 与当前项目一致，缺失或 Fork 来源均保守拒绝。见 [真实资格采集](../qualification/provider.md)。

删除受管 Worktree 时，仓库或 cwd 未知的活跃 Codex 会话按潜在冲突保守拒绝；破坏性操作排队过久将使操作范围证据过期，需要重新审阅和确认。Git 命令非零退出会依据实际 Git 状态协调，不假设没有副作用。删除 Worktree 只有在 Git 注册信息与磁盘目录**均已消失**时才视作已验证成功；两者不一致时回执保持 `OutcomeUnknown`，禁止自动重试。这不能对独立 Codex 进程建立原子锁，必须继续进行真实并发操作验证。

## 本地语言与可访问性
<!-- docs-section: language -->

配置 `[ui].language = "en"` 或 `"zh-CN"`，或使用 `"auto"` 跟随进程简体语言环境。`presentation = "quiet"` 合并后台刷新；`"screen-reader"` 降低后台刷新频率但保持直接输入即时响应。`[notifications].mode` 默认 `"off"`。这些都只是本地偏好，不会创建团队同步权威。

## Windows SSH Ubuntu 与 Terminal Drawer
<!-- docs-section: ssh -->

通过 Windows SSH 客户端控制 Ubuntu 时，codex-tui **在 Ubuntu 运行**；终端设备、文件路径、Codex、配置、Shell 子进程都应以 Ubuntu 为准。`t` 打开 Drawer，`F6` 或 `Ctrl+]` 在终端支持时返回聚焦。若 Windows/SSH 抢占按键，先查 `?` 与 `doctor terminal`；Resize、Focus、Ctrl+C、退出与终端恢复必须在**真实控制 TTY** 验证。托管 PTY 回归不是该证明。界面会对不可信的会话消息、Git diff 正文、仓库路径及平台状态中的控制字符、Unicode 双向覆盖符，以及软连字符、零宽空格和词连接符等视觉不可见格式字符做安全替换；仅改变显示内容，不改动 Codex/Git 权威原始数据。Terminal Drawer 是独立的 PTY 终端模拟器，不属于安全过滤后的预览。

## 证据与数据安全
<!-- docs-section: safety -->

SQLite 保存个人元数据与规划覆盖层；默认**不持久化**用户和助手消息原文。若需本地全文检索，在 config.toml 中配置 `[search] persist_local_transcripts = true`；最多 10,000 条，消息原文和标题分别最多存储 4,096 字符；按首次本地索引时间计算最多保留 30 天（在维护运行时清理）。重复打开同一已缓存消息不会延长其保留期限。关闭此选项后下次启动逻辑删除本地文本/FTS 行，但不保证闪存物理擦除。Codex 仍是对话权威。数据损坏时不得用删除数据库代替诊断，先保留 Store/WAL/SHM 并设计离线恢复。`doctor bundle --output ./codex-tui-support` 需复核隐私；Bug 记录必须带精确源码 SHA。TUI 操作不授权 Stable 发布。

