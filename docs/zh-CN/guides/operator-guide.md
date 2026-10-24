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

默认连接本机的 `codex app-server --listen stdio://`。命名目标由本地 TOML 配置，单次可使用 `codex-tui --target NAME`。非回环地址的明文 `ws://` 即使未配置令牌也拒绝连接；远程使用 `wss://` 或 SSH 隧道映射到回环地址。`localhost`、IPv4/IPv6 回环仍可用于本地开发。真实后端失败不会悄悄切到假数据，`--fake` 只用于测试。命名 App Server 的细节见 [英文设计](../../implementation/v1.3-remote-app-server-targets.md)。

`doctor` 命令会对外部路径、分支、主机名、诊断字段和错误信息进行终端输出端净化；ANSI 控制符与 Unicode 双向覆盖符显示为可见替换字符，原始 Codex/Git/Forge 数据不变。

`headless` 纯文本输出仅对各个不可信字段进行转义，保留原有制表符分列格式；`--json` 仍序列化原始快照，不做显示层文本替换。

`doctor compat` 会净化纯文本版本号与诊断信息中的危险终端控制字符，`doctor compat --json` 保持原始证据和已有数据结构。

CLI 参数错误、支持包路径和 `doctor` 启动失败提示也会在最终终端显示处净化危险控制字符；原始文件路径与帮助内容保持不变。

## 工作和提醒导航
<!-- docs-section: daily -->

Thread 页面用 `PageUp`/`PageDown` 按显示行滚动消息，长段落和代码块也可完整浏览。`Home` 返回已加载历史的开头，在开头再次按 `PageUp` 请求更早的一页；`End` 恢复跟随最新内容。手动滚动会暂停跟随，新消息到达、加载更早历史或调整终端大小时会保留阅读位置。待处理请求保持可见，其余显示行仍可浏览历史。

刷新后的最近历史页若不再包含正在阅读的条目，Thread 会保留该较早历史的本地快照，同时继续更新实时回合状态。“暂停阅读”标记表示当前正在查看快照；`End` 返回最新历史并释放快照，离开 Thread 时也会释放。

有待处理请求且未在编辑文本时，在 Thread 页面按 `d` 查看命令、工作目录和权限详情。详情页用 `PageUp`/`PageDown` 或 `↑`/`↓` 滚动，`Home`/`End` 移到两端，`Esc` 只返回，不会提交或取消请求。请求解决或被替换后，其详情页失效；浏览仅使用本地内容，不向 App Server 发送请求。未识别的权限字段仍明确标记为隐藏，接受操作保持禁用。

打开会话或其队列时，会在当前 App Server 连接上获取订阅；服务端的已加载清单和历史读取本身不建立订阅。最后一个历史、队列或待处理请求的持有者离开后才释放订阅，活跃工作与可操作请求继续保留既有持有关系。订阅被拒绝或结果未知时会明确报告，不会静默重放写操作。固定协议基线已通过标签源码审阅和合成解析回放核对至 Codex 0.164.0，真实账号验收仍需单独完成。新增的上游已读状态字段及通知不会同步本地未读/确认状态；图像生成条目的内联结果和文件引用均保留通用条目显示，客户端不读取对应图像文件。详见[保留的兼容性评估](../../../tests/fixtures/protocol/stable-0.164.0/assessment.json)。

Mission Control 按待处理、置顶、最近活动排序，同优先级以会话 ID 确定顺序。仍保留原有“最近会话窗口＋置顶/待处理例外”，不会自动展开全部历史。Snooze 暂停待处理提升，但保留置顶；已读确认不会掩盖真实审批、输入请求或错误。首次加载选中显示首项，之后刷新按会话 ID 保留可见的选中项。窄终端在运行状态列直接显示待处理原因，普通会话仍显示运行状态。

快速消息会从 Mission Control、Board、Review、Workspace 或 Managed Worktrees 进入所选会话的可见输入框。`Esc` 退出输入并保留草稿，再按一次 `Esc` 回到原页面；切换其他会话后清除这次临时返回路径。

Registry 提醒也包含已经观测到的 Goal 阻塞、用量或预算限制、流水线失败、请求修改及工作树冲突。Board、List 和 Review Queue 每张卡片使用一行，按终端宽度截断长标题或工作区名称，使导航选中项保持可见。

选中任务或看板卡片按 `=`、再按 `1–9` 绑定快捷槽。Registry、Board、Thread、Review、Workspace、Managed Worktrees 中按 `1–9` 可直接打开绑定会话或本地 Scratch；当前会话重复选中不会重载。目标不可用会明确提示，不会误开其他会话。文本编辑、审批确认和 Terminal Drawer 聚焦时，按键仍由这些操作处理；快捷槽不启动或调度任务。\n\n`j/k` 选中、Enter 打开、`/` 筛选、`Ctrl+K` 命令面板、`?` 查看实时键位。`b` 打开 Board，`Tab` 切换 Saved View，包括按工作区分组的 Needs You / Needs Review 列表。List/Review Queue 将同一工作区连续排列，组内按所选顺序排序；Board 列仍按工作流阶段排列。Board 内用 `h/l` 切换列、`j/k` 选择卡片；Mission Control 的 `r` 进入所选会话的 Review，`w` 进入 Workspace。这些入口直达既有交付上下文；Board/Attention 始终是派生视图。

Headless 的 `threads`、`work`、`status`、`attention`、`board` 使用配置中的活动 Codex 目标或 `--target NAME`，一次只获取一个目标。来源字段标明目标与传输，规划视图明确标记本地存储跨目标共享。别名/置顶/卡片仍以不含目标的会话 ID 为键，跨目标的相同 ID 可能共享本地元数据。Forge 读取当前目录，Worktrees 读取共享本地缓存。详见 [CLI 范围与失败语义](cli-reference.md#只读无头命令)。

搜索与 Saved View 仅管理个人工作流；未知上游历史不会被虚构。全文搜索使用短生命周期独立 Codex App Server 连接，不占用主审批/会话 Actor；只允许一个搜索，新请求取消旧请求，总体 12 秒超时会明确提示。单 Thread 匹配项分页未耗尽时不得标记全部完成。若等待中的 RPC 收到服务端审批请求，主 Actor 会优先分派已排队的审批事件，再处理其他命令或周期探测；被中断的 RPC 结果仍为未知，不自动重放。本地索引的短查询会将 `%`、`_` 和反斜杠按字面字符搜索，而不是 SQLite 通配符。要区分 Windows 路径残影与 Ubuntu 本机会话，应检查仓库 cwd 和 Codex Home。上游清除项目归属或标题后，先用安全的 cwd/会话 ID 临时显示并触发合并后的权威重读；解归档也通过既有有界刷新恢复，不凭通知虚构会话。App Server 的 Git Origin 仅保留剔除凭证、查询和片段后的 host/repository 身份，无法安全解析时退回 cwd，不将原始 URL 带入搜索、Headless 或界面。每周仅查询公开 Codex 稳定版元数据并对新版提示复核，不能替代真实账号 L3 验收。

本地对话搜索在短查询的 LIKE 路径和长查询的 FTS 路径中，均仅匹配已缓存的用户/助手**消息正文**。会话标题继续由全局元数据搜索负责。缺少 FTS 时采用相同的正文搜索范围；本地结果始终只是部分缓存，不代表完整历史。

Review 页面使用 `e` 在外部编辑器打开变更文件时，必须具备与当前会话及 Review 匹配的 Git 工作树身份。Git 变更路径以工作树根目录为基准，而非会话子目录；绝对路径、`..` 越界路径、不存在的目标和指向工作树外的符号链接都会在启动编辑器前被拒绝。身份过期时需刷新 Review/Git；不会改写原始路径或 Git 状态。

Review 外部浏览器链接必须是主机明确的 HTTP(S) URL，不允许嵌入用户凭证、控制字符、空白符或反斜杠导致的歧义解析。无效平台链接会在启动浏览器前拒绝；合法的 GitLab/GitHub URL 原样传递。

## 仓库和交付状态
<!-- docs-section: forge -->

在有 Git Remote 的目标仓库执行 `codex-tui doctor git` 和 `codex-tui doctor forge`。内部 GitLab 使用已认证 `glab`，GitHub 使用 `gh`。GitLab/GitHub 某一只读接口失败时，对应能力会降级，其他成功的数据可保留，但 Forge 会明确提示不完整；三个核心接口全部失败时不得报告“最新空结果”。Headless Forge 若缺少核心能力应以降级退出码返回。

Review 详情绑定当前仓库、分支、变更请求 ID 和已观测的 head 版本。分支或 head 变更会使不匹配的缓存详情和迟到响应失效；打开 Review 时，按已有 Forge 刷新间隔重新获取已过期或失败的详情。

HTTP(S) Git 远端会在平台地址中保留非默认端口，例如 `gitlab.example:8443`；请在 `glab` 中认证该地址。SSH 传输端口（如 `:2222`）不作为 API 端口；SSH 与 API 端点不同时，在平台 CLI 配置中维护映射。

Forge 概览仅获取各能力最近 20 条记录。GitHub Issues API 会混入 PR，系统在过滤 PR 前保留原始分页是否触顶的证据；因此即使展示 0 条 Issue，只要原始页已满，也不得断言不存在。当前分支的 CR/Pipeline 未命中**不代表不存在**；Board 数据来源与 Review 会提示该不确定性，精确结果仍应向平台核实。MR/PR 创建、评论、批准、合并遵循先预览→明确确认→再执行；Approval/Merge 会重新校验目标修订版本，结果不确定时禁止盲目重试。Review 状态权威在 GitLab/GitHub。Forge Merge 要求明确可合并状态和经验证的审批信息；GitHub 合并性未知、GitLab 审批数量缺失或详细状态未知时必须阻止执行。创建/审批的前置检查使用有界完整分页，不把第一页当作全部数据，服务端仍为最终权威。GitHub Fork PR 与 Actions 工作流均使用 `owner/repo:branch` 等完整源仓库标识展示，不得仅凭同名分支绑定到本地 Git 分支。工作流若缺少 `head_repository`，标记为 `unverified-source:branch`，不能将其失败误归属本地分支。与本地分支绑定的审批、评论和合并需要 PR 源仓库 ID 与路径均匹配；缺少来源信息时保守拒绝。GitLab 跨项目 MR 也会以 `project/<id>:branch` 标记来源，不能冒充本地分支；与本地分支绑定的写操作要求 `source_project_id` 和 `target_project_id` 与当前项目一致，缺失或 Fork 来源均保守拒绝。见 [真实资格采集](../qualification/provider.md)。

删除受管 Worktree 时，仓库或 cwd 未知的活跃 Codex 会话按潜在冲突保守拒绝；破坏性操作排队过久将使操作范围证据过期，需要重新审阅和确认。Git 命令非零退出会依据实际 Git 状态协调，不假设没有副作用。删除 Worktree 只有在 Git 注册信息与磁盘目录**均已消失**时才视作已验证成功；两者不一致时回执保持 `OutcomeUnknown`，禁止自动重试。这不能对独立 Codex 进程建立原子锁，必须继续进行真实并发操作验证。

创建受管 Worktree 时，默认或显式输入的 `HEAD` 固定为当前会话所在工作树已观测到的完整提交 ID，适用于主工作树、关联工作树和 detached 工作树。确认页显示该已观测提交；需要更新观测时请先刷新 Git。预览后 `HEAD` 的移动不会改变此计划的起点，创建结果必须与固定提交一致。其他显式输入的 ref 仍按输入保留，由 Git 在执行时解析；其预览不声称已固定提交。

删除要求不存在 tracked 改动、untracked 文件或 **ignored 文件**，包含被忽略的本地配置及构建目录。请先自行保存或移除这些资产，再创建新的删除计划。检查包含 ignored 文件；检查失败、输出超过 256 KiB 或探测超过 5 秒时均拒绝删除。不增加 force 或自动清理模式，继续保留受管/接管身份及活跃操作范围约束。检查后的独立文件变更仍需真实并发环境验证。

## 本地语言与可访问性
<!-- docs-section: language -->

配置 `[ui].language = "en"` 或 `"zh-CN"`，或使用 `"auto"` 跟随进程简体语言环境。`presentation = "quiet"` 合并后台刷新；`"screen-reader"` 降低后台刷新频率但保持直接输入即时响应。`[notifications].mode` 默认 `"off"`。同一 75 毫秒窗口的多条完成／待处理提醒会合并为一次响铃或通知，保留最高优先级事件与其余提醒数量；原始 Attention/事件边沿不丢失。Windows SSH Ubuntu 时，即使选择 `"os"`，也经 SSH 回退为客户端终端响铃，不会误以为服务器的桌面通知是 Windows Toast；Windows Terminal SSH 配置可设 `"bellStyle": "all"` 实现声音和任务栏闪烁，在 Ubuntu 用 `printf '\\a'` 验证。TUI 必须持续运行并连接，断线、终端模拟器或 tmux 可能影响送达；未增加 Windows 远端通知桥接器。桌面通知仅在输出时净化和限制标题的显示宽度；Linux 系统通知在 `notify-send --` 选项终止符之后传递动态文本，原始工作项名称与通知追踪状态保持不变。启用通知后使用有界分派队列和 128 条待投递重试队列，由正常 UI 循环非阻塞排空；超过容量会明确提示未入队数量并引导查看 Needs You，而不是声称不会丢提醒。这些都只是本地偏好，不会创建团队同步权威。

受管 Worktree 变更与异常协调优先使用完整 NUL 分隔 Git 清单，以准确保留换行和尾随空格。Linux 资格镜像中的旧 Git 2.25 不支持 Worktree `-z`：此时使用严格完整的纯文本 porcelain 解析，保留尾随空格；遇到格式歧义时拒绝，不得据此推断工作树不存在。包含换行的工作树路径需要支持 NUL 的新版 Git。非法编码及标准输出超过 256 KiB 时保守拒绝；超大清单另做资格验证。

Terminal Drawer 的 PTY 输出改为协作式排空：每次主循环最多处理 16 个事件，其余内容仍留在原有的 64 槽 FIFO 中，避免子进程持续刷屏时饿死键盘、渲染和状态协调。即使 PTY 启动 Ready 事件迟于窗口 Resize 到达，也不允许它把已请求的终端尺寸回退至启动初值或与 VT 解析器尺寸脱节。队列持续有界；该托管测试不能代替真实控制 TTY 验收。

## Windows SSH Ubuntu 与 Terminal Drawer
<!-- docs-section: ssh -->

通过 Windows SSH 客户端控制 Ubuntu 时，codex-tui **在 Ubuntu 运行**；终端设备、文件路径、Codex、配置、Shell 子进程都应以 Ubuntu 为准。`t` 打开 Drawer，`F6` 或 `Ctrl+]` 在终端支持时返回聚焦。若 Windows/SSH 抢占按键，先查 `?` 与 `doctor terminal`；Resize、Focus、Ctrl+C、退出与终端恢复必须在**真实控制 TTY** 验证。托管 PTY 回归不是该证明。界面会对不可信的会话消息、Git diff 正文、仓库路径及平台状态中的控制字符、Unicode 双向覆盖符，以及软连字符、零宽空格和词连接符等视觉不可见格式字符做安全替换；选中会话详情和离线状态栏也会对会话 ID、模型名、Git 分支/错误和 App Server 来源字符串执行同样的仅显示端净化；受管 Worktree 的计划/确认界面及 Board 的变更请求状态，同样净化来自 Git/Forge 的不可信字符串；仅处理显示文本，不跳过显式操作确认。Forge 变更、启动预设和本地批量操作的确认窗口，以及本地文本编辑和命令面板查询也过滤不可信显示文本；变更请求与外部进程 argv 的原始值保持不变。Mission Control/Board 页脚输入、自定义 Saved View 标题/分组及 Scratch ID 同样进行显示层防护；原始搜索、状态和本地元数据不改变。仅改变显示内容，不改动 Codex/Git 权威原始数据。Terminal Drawer 是独立的 PTY 终端模拟器，不属于安全过滤后的预览。

## 证据与数据安全
<!-- docs-section: safety -->

SQLite 保存个人元数据与规划覆盖层；默认**不持久化**用户和助手消息原文。若需本地全文检索，在 config.toml 中配置 `[search] persist_local_transcripts = true`；最多 10,000 条，消息原文和标题分别最多存储 4,096 字符；按首次本地索引时间计算最多保留 30 天（在维护运行时清理）。重复打开同一已缓存消息不会延长其保留期限。关闭此选项后下次启动逻辑删除本地文本/FTS 行，但不保证闪存物理擦除。Codex 仍是对话权威。数据损坏时不得用删除数据库代替诊断，先保留 Store/WAL/SHM 并设计离线恢复。`doctor bundle --output ./codex-tui-support` 需复核隐私；Bug 记录必须带精确源码 SHA。TUI 操作不授权 Stable 发布。

