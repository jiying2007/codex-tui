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

日常操作优先学习：按 `Space` 找下一条待处理任务、`Enter` 打开、`/` 搜索、`1–9` 切换已绑定的任务。按 `?` 展开完整帮助：在窄 SSH 终端中 `↑/↓` 或 `j/k` 逐项滚动、`PgUp/PgDn` 每次跳三项、`Home/End` 到第一/最后一项、`Esc` 关闭，不会在背景执行其他快捷键。

**鼠标限制：** 当前没有点击/滚轮事件处理，应用已停止捕获鼠标报告，以免影响终端原生文字选取和滚动。旧 `[ui].mouse` 仍能读取，但会被忽略；配置 `mouse = true` 并不会启用点击导航。

默认连接本机的 `codex app-server --listen stdio://`。命名目标由本地 TOML 配置，单次可使用 `codex-tui --target NAME`。非回环地址的明文 `ws://` 即使未配置令牌也拒绝连接；远程使用 `wss://` 或 SSH 隧道映射到回环地址。`localhost`、IPv4/IPv6 回环仍可用于本地开发。真实后端失败不会悄悄切到假数据，`--fake` 只用于测试。命名 App Server 的细节见 [英文设计](../../implementation/v1.3-remote-app-server-targets.md)。

**首次连接失败：**排除临时后端问题后，在空的 Mission Control 中按无修饰的单次 `F5`；编辑框、帮助、确认框或聚焦的 Terminal Drawer 会保留按键所有权。只重试原先解析的同一目标，同时最多一个连接尝试，没有自动重试定时器；草稿与本地操作状态保留。一旦连接成功安装，即使后来断开，F5 也不重连已建立过的会话，不重放消息或审批。该入口不重新加载目标配置；配置本身无效时需修正后显式重启。

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

快速消息会从 Mission Control、Review、Workspace 进入所选会话的可见输入框。`Esc` 退出输入并保留草稿，再按一次 `Esc` 回到原页面；切换其他会话后清除这次临时返回路径。

Registry 提醒也包含已经观测到的 Goal 阻塞、用量或预算限制、流水线失败、请求修改及工作树冲突。Registry 每条会话使用一行，按终端宽度截断长标题或工作区名称，使导航选中项保持可见。

观察到空闲／Goal 完成转换、审批／输入请求、系统错误、额度／预算受限、流水线失败或待评审事件后，有足够终端空间时会短暂显示一行**最近提醒**；`[notifications].mode = "off"` 时也保留。提醒行**开头**就标明 `! 跳转`，避免窄终端里长任务标题将快捷键截掉；无关联 Codex 会话的事件开头则显示“未定位”，不伪造可跳转目标。按 `!` 可跳转到关联的活跃 Codex 会话；对上报原始数字键的终端也兼容 `Shift+1`。`Esc` 按一级路径返回；即使 UI 尚未执行定期清理，过期事件也会在动作入口被拒绝。修饰键组合不会误触发数字快捷槽；审批及破坏性确认键只接受无修饰的单次按下，长按重复事件不能连续确认，Ctrl+C 原有中断或取消行为不变；帮助窗口按 `Esc` 或 `?` 关闭，不会在背后切换会话。提醒显示约 20 秒，纯内存、不持久化、不代表终端响铃或系统通知必然送达；没有关联会话的更新须核对权威来源，绝不误开其他会话。编辑框、审批详情、帮助、命令面板、操作确认框和聚焦终端都不会被提醒抢占；有弹层时 `!` 不会在背后切换会话或提前消耗提醒。

**空闲不等于成功：**“已空闲；请核对结果”只表示运行状态从工作／等待转为空闲，不证明轮次成功或任务验收完成。“Goal 已完成”要求观测到 Goal 完成状态转换，仍不等于交付验收。提醒还覆盖已观测到的系统错误、额度受限和预算受限；不会虚构上游未报告的结果。

在已连接且身份明确的 Target/CODEX_HOME 作用域中选中 Codex 会话，按 `=`、再按 `1–9` 绑定快捷槽。历史无作用域快捷槽保留在原库供核对，不允许跳转；须在准确作用域内明确重新绑定。备注、稍后提醒与新书签也按会话作用域保存；历史无作用域备注、稍后提醒和完成标记不影响真实会话。备注及稍后提醒等待 SQLite 回执才清除输入，作用域变化或断线则拒绝提交。断线或作用域变化时拒绝绑定和跳转，不迁移或覆盖历史记录。Registry、Thread、Review、Workspace 中按 `1–9` 可直接打开绑定会话。跳转 Codex 会话后按一次 `Esc` 返回原工作页面（仅保留一级返回路径，不引入独立历史栈）；重复选中当前会话不会重载。快捷槽通过已有会话 ID 索引定位，不再全量扫描会话清单。目标不可用会明确提示，不会误开其他会话。文本编辑、审批确认和 Terminal Drawer 聚焦时，按键仍由这些操作处理；快捷槽不启动或调度任务。

`j/k` 选中、Enter 打开、`/` 筛选、`Ctrl+K` 命令面板、`?` 查看实时键位。Mission Control 的 `r` 进入所选会话的 Review，`w` 进入 Workspace。这些入口直达既有交付上下文；Attention 始终是观测结果。

Headless 的 `threads`、`work`、`status`、`attention`、`board` 使用配置中的活动 Codex 目标或 `--target NAME`，默认只获取一个目标，除非显式执行 `threads --all-targets --query TEXT`。任务中心的 `Ctrl+K` → **搜索全部目标** 使用同一有界只读元数据查询，已获取结果会逐步以 Partial 状态显示。来源字段标明目标与传输，规划视图明确标记本地存储跨目标共享。操作员别名、置顶和确认偏好按配置目标及观测的 Codex Home 隔离，旧无作用域偏好已隔离；旧规划元数据仍可能含裸会话 ID，不是跨目标权威。Forge 读取当前目录，Worktrees 读取共享本地缓存。详见 [CLI 范围与失败语义](cli-reference.md#只读无头命令)。

队列项交由原生 Codex CLI 管理。Lite 候选不再打开或编辑独立会话队列，也不查询其清单；原生会话历史订阅与待处理交互请求仍独立保留。

使用原生 Codex CLI 创建、编辑、暂停、恢复或清除 Goal。Thread 中的 `g`（或命令面板中的“刷新 Goal 观测”）仅只读获取当前会话 Goal，已观测到的额度受限、阻塞和完成仍用于 Attention；codex-tui 不再发出 Goal 写操作。

Lite 候选退役看板交互式批量编辑、Scratch 新建/修改/状态/删除，以及 Saved View 新建/编辑/删除。交互 Board、Scratch 导航入口及旧投影 Worker 启动链均已移除。历史规划记录通过显式只读 Headless、`doctor store` 及备份恢复测试保留；旧离线诊断不代表当前交互行为。真实研发事项回原平台维护，并关联原生会话；保留会话备注、稍后提醒和书签入口。本次退役不删除或改写现有 SQLite 记录。

任务中心搜索、会话详情、书签名称和原生提醒路由均不依赖看板卡片。`/` 匹配会话元数据、当前工作树/Forge 观测及保留的本地备注；限定字段为 `thread:`、`cwd:`、`project:`、`goal:`、`forge:`、`branch:`、`worktree:`、`runtime:`、`note:`。原 `card:`、`work:`、`link:`、`related:`、`stage:`、`tag:` 已退役，真实事项关联使用原平台或 `workref`。历史数据保留，不将旧规划状态冒充原生当前状态。

任务中心与原生会话提醒直接使用当前 Codex Goal、Git 和 Forge 观测，不再等待看板卡片投影。迁移期间只读索引原有稍后提醒及完成确认偏好；这些偏好不授予写入权限。稍后提醒可以推迟普通待核对事项，但不能隐藏任务中心或原生提醒中的真实审批、输入请求和系统错误。

搜索与 Saved View 仅管理个人工作流；未知上游历史不会被虚构。全文搜索使用短生命周期独立 Codex App Server 连接，不占用主审批/会话 Actor；只允许一个搜索，新请求取消旧请求，总体 12 秒超时会明确提示。单 Thread 匹配项分页未耗尽时不得标记全部完成。若等待中的 RPC 收到服务端审批请求，主 Actor 会优先分派已排队的审批事件，再处理其他命令或周期探测；被中断的 RPC 结果仍为未知，不自动重放。本地索引的短查询会将 `%`、`_` 和反斜杠按字面字符搜索，而不是 SQLite 通配符。要区分 Windows 路径残影与 Ubuntu 本机会话，应检查仓库 cwd 和 Codex Home。上游清除项目归属或标题后，先用安全的 cwd/会话 ID 临时显示并触发合并后的权威重读；解归档也通过既有有界刷新恢复，不凭通知虚构会话。App Server 的 Git Origin 仅保留剔除凭证、查询和片段后的 host/repository 身份，无法安全解析时退回 cwd，不将原始 URL 带入搜索、Headless 或界面。每周仅查询公开 Codex 稳定版元数据并对新版提示复核，不能替代真实账号 L3 验收。

本地对话搜索在短查询的 LIKE 路径和长查询的 FTS 路径中，均仅匹配当前已核实且连接的 Target/CODEX_HOME 所属用户/助手**消息正文**。历史无作用域缓存不出现在真实检索结果中，仍沿用原有有界保留与关闭索引清理策略；跨作用域迟到结果直接丢弃。会话标题继续由全局元数据搜索负责。缺少 FTS 时采用相同的正文搜索范围；本地结果始终只是部分缓存，不代表完整历史。

Review 页面使用 `e` 在外部编辑器打开变更文件时，必须具备与当前会话及 Review 匹配的 Git 工作树身份。Git 变更路径以工作树根目录为基准，而非会话子目录；绝对路径、`..` 越界路径、不存在的目标和指向工作树外的符号链接都会在启动编辑器前被拒绝。身份过期时需刷新 Review/Git；不会改写原始路径或 Git 状态。

Review 外部浏览器链接必须是主机明确的 HTTP(S) URL，不允许嵌入用户凭证、控制字符、空白符或反斜杠导致的歧义解析。无效平台链接会在启动浏览器前拒绝；合法的 GitLab/GitHub URL 原样传递。

**SSH 下的外部打开：**编辑器／浏览器命令运行在 TUI 所在主机，不会自动交给 Windows 客户端。检测到 `SSH_TTY` 或 `SSH_CONNECTION` 且未配置显式适配器时，会在创建进程前拒绝默认图形程序启动；可手动使用显示的目标，或将 `CODEX_TUI_EDITOR`／`CODEX_TUI_BROWSER` 配成单个可执行适配器。路径与 URL 校验仍生效。进程启动不证明窗口已打开或最终执行成功；当前没有 Windows 桥接、保证完整长目标的复制窗口或外部程序最终回执。

## 仓库和交付状态
<!-- docs-section: forge -->

在有 Git Remote 的目标仓库执行 `codex-tui doctor git` 和 `codex-tui doctor forge`。内部 GitLab 使用已认证 `glab`，GitHub 使用 `gh`。GitLab/GitHub 某一只读接口失败时，对应能力会降级，其他成功的数据可保留，但 Forge 会明确提示不完整；三个核心接口全部失败时不得报告“最新空结果”。Headless Forge 若缺少核心能力应以降级退出码返回。

Review 详情绑定当前仓库、分支、变更请求 ID 和已观测的 head 版本。分支或 head 变更会使不匹配的缓存详情和迟到响应失效；打开 Review 时，按既有新鲜度 TTL 重新获取已过期或失败的详情。Forge 概览复用 Git 上下文观测事件，仅刷新最近 Registry 窗口、选中及活跃会话，以及已缓存的固定或待处理会话；不再有独立的 15 秒全局 Forge 轮询。其他历史观测保留并按正常时间变旧，重新关注时再更新。

HTTP(S) Git 远端会在平台地址中保留非默认端口，例如 `gitlab.example:8443`；请在 `glab` 中认证该地址。SSH 传输端口（如 `:2222`）不作为 API 端口；SSH 与 API 端点不同时，在平台 CLI 配置中维护映射。

Forge 概览仅获取各能力最近 20 条记录，保留过滤前的原始分页触顶证据；当前分支 CR/Pipeline 未命中不能证明不存在，精确结果向原平台核实。GitHub Fork PR 和工作流、GitLab 跨项目 MR 保留来源身份，缺失来源时不将失败误归属本地分支。Lite 运行时不再创建、评论、批准、合并 MR/PR，也不自动恢复旧 Forge 变更；使用原生平台或 CLI。保留历史回执，包括 Pending/OutcomeUnknown，不删除、不重放。见 [真实资格采集](../qualification/provider.md)。

Lite 运行时退役受管 Worktree 创建、接管、移除和分支删除。使用原生 Git/Codex 工具，破坏性操作前核实真实仓库、工作树与活跃会话范围。退役不删除实际目录、分支、本地受管记录或回执；既有未知结果先向 Git 核实再操作。只读 `worktree list` 是历史本地记录清单，不是实时 Git 权威清单。

## 本地语言与可访问性
<!-- docs-section: language -->

配置 `[ui].language = "en"` 或 `"zh-CN"`，或使用 `"auto"` 跟随进程简体语言环境。`presentation = "quiet"` 合并后台刷新；`"screen-reader"` 降低后台刷新频率但保持直接输入即时响应。`[notifications].mode` 默认 `"off"`，只关闭外部投递，保留界面内最近提醒与准确回跳。`screen-reader` 仅调整重绘节奏，不代表读屏器兼容性已通过验收。启用外部投递后，同一 75 毫秒窗口的多条状态／待处理提醒可合并为一次响铃或通知，保留最高优先级事件与其余提醒数量；精确状态仍需查看 Mission Control，不能据此承诺每条外部提醒必达。Windows SSH Ubuntu 时，即使选择 `"os"`，也经 SSH 回退为客户端终端响铃，不会误以为服务器的桌面通知是 Windows Toast；Windows Terminal SSH 配置可设 `"bellStyle": "all"` 实现声音和任务栏闪烁，在 Ubuntu 用 `printf '\a'` 验证。TUI 必须持续运行并连接，断线、终端模拟器或 tmux 可能影响送达；未增加 Windows 远端通知桥接器。桌面通知仅在输出时净化和限制标题的显示宽度；Linux 系统通知在 `notify-send --` 选项终止符之后传递动态文本，原始工作项名称与通知追踪状态保持不变。启用外部通知后使用有界分派队列和 128 条待投递重试队列，由正常 UI 循环非阻塞排空；超过容量会明确提示未入队数量并引导查看 Needs You，而不是声称不会丢提醒。这些都只是本地偏好，不会创建团队同步权威。

保留的历史受管工作树清单和操作回执仅供审计，不是实时恢复或执行入口。

Terminal Drawer 的 PTY 输出改为协作式排空：每次主循环最多处理 16 个事件，其余内容仍留在原有的 64 槽 FIFO 中，避免子进程持续刷屏时饿死键盘、渲染和状态协调。即使 PTY 启动 Ready 事件迟于窗口 Resize 到达，也不允许它把已请求的终端尺寸回退至启动初值或与 VT 解析器尺寸脱节。队列持续有界；该托管测试不能代替真实控制 TTY 验收。

## Windows SSH Ubuntu 与 Terminal Drawer
<!-- docs-section: ssh -->

通过 Windows SSH 客户端控制 Ubuntu 时，codex-tui **在 Ubuntu 运行**；终端设备、文件路径、Codex、配置、Shell 子进程都应以 Ubuntu 为准。`Alt+T` 在 Mission Control／当前应用页与 Terminal Drawer 之间**双向切换焦点**（无须使用功能键）（保持同一个 Shell，不重建 PTY）；`t` 打开或重新聚焦 Drawer，`T` 关闭 Drawer。终端聚焦时 `Ctrl+]` 仍可返回应用作为备用；弹窗、编辑框和确认界面继续优先处理自己的按键。若 Windows/SSH 抢占按键，先查 `?` 与 `doctor terminal`；Resize、Focus、Ctrl+C、退出与终端恢复必须在**真实控制 TTY** 验证。托管 PTY 回归不是该证明。界面会对不可信的会话消息、Git diff 正文、仓库路径及平台状态中的控制字符、Unicode 双向覆盖符，以及软连字符、零宽空格和词连接符等视觉不可见格式字符做安全替换；选中会话详情和离线状态栏也会对会话 ID、模型名、Git 分支/错误和 App Server 来源字符串执行同样的仅显示端净化；变更请求状态同样净化来自 Forge 的不可信显示字符串。本地文本编辑和命令面板查询也过滤不可信显示文本；变更请求与外部交接目标的原始值保持不变。Mission Control 页脚输入及保留的诊断渲染 同样进行显示层防护；原始搜索、状态和本地元数据不改变。仅改变显示内容，不改动 Codex/Git 权威原始数据。Terminal Drawer 是独立的 PTY 终端模拟器，不属于安全过滤后的预览。

## G2/G3 手工需求、缺陷和跨仓引用
<!-- docs-section: workref -->

`codex-tui workref` 为**按需使用的本地引用命令**。真实飞书项目/TAPD 事项的创建、处理与验收仍由原平台负责，无需企业 API 权限。URL、Target 身份、协议版本与测试结果均由操作者**手工录入、尚未在线核实**；它们不是远程 Codex 接续凭证，也不能用于自动判断任务完成。

要让 Mission Control 精确匹配，`--target` 必须填写目标主机/配置的 `codex-tui doctor store` 输出的**无凭据** `app-server-target-identity`，不能仅填写显示名称或主机名；若要核对另一个已配置目标，可使用 `codex-tui doctor codex --target NAME`。同时从该目标 `codex-tui status --json` 获取真实 `codexHome`，并填写准确的原生 ThreadId。这些是手工引用，不代表远程主机身份认证。

TUI 启动时仅读取现有 SQLite `workref` 快照一次。**宽屏 Mission Control 已选会话详情**显示完整 Target/CODEX_HOME/Thread 匹配的前两项本地引用、契约版本、各类独立证据结果和交接数量；**80 列终端**可进入 Thread 查看关联提示。记录损坏显示 **UNAVAILABLE/不可用**，不得伪装“没有关联”。本功能不影响 Codex 原生权限、Attention 或飞书/TAPD 状态。CLI 写入与 TUI 共享独占锁，修改引用前退出 TUI，修改后重新启动以更新快照；不做进程间实时刷新或自动交接。

要把**当前选中的有效 Codex 会话**关联到已有事项，无需手工输入 Target/Home/Thread：按 `.` → **关联当前会话到已有事项…**，或按 `Ctrl+K` 搜索 **关联当前会话到事项**。编辑框列出最多五个已有本地事项 Key，输入从权威飞书/TAPD 事项复制的准确 Key。TUI 在打开时捕获当前连接的精确 Target、已观测 CODEX_HOME 和原生 ThreadId，并在按 Enter 时再次校验视图与身份未变化。会话别名按仓库名自动生成；发生冲突时追加 `-2`、`-3` 等后缀，角色记录为 `development`。此操作不会创建事项或发送消息。SQLite 提交成功后会立即更新当前 TUI 的内存引用快照；CLI 写入仍需先退出 TUI，因为交互实例持有现有独占写锁。 若有界工作线程队列拒绝接收，输入的事项 Key 会保留，可修正或重试；一旦接收写入，编辑器会冻结，直至收到 SQLite 提交回执。若工作线程停止且未收到回执，明确报告**结果未知**并隐藏旧关联快照，必须先检查 SQLite，再决定是否重试。不会把已接收的写入误报为已取消，也不会盲目重试。

~~~bash
# 从原平台复制真实事项 ID、空间和 HTTPS URL。
codex-tui workref add --key BUG17 --provider tapd --space robot --type bug --id 17 --url "$ORIGINAL_TAPD_URL"
# 没有产品事项的纯技术协作可使用 --provider local-draft --type technical。
codex-tui workref session --key BUG17 --alias soc --target "$SOC_TARGET_ID" --home "$SOC_CODEX_HOME" --thread "$SOC_THREAD_ID" --repo robot-app --role implementation
codex-tui workref session --key BUG17 --alias mcu --target "$MCU_TARGET_ID" --home "$MCU_CODEX_HOME" --thread "$MCU_THREAD_ID" --repo motor-fw --role firmware
codex-tui workref contract --key BUG17 --kind protocol --source robot-app/docs/protocol.md --revision r3 --git-sha "$FULL_40_CHAR_COMMIT"
codex-tui workref evidence --key BUG17 --kind integration --source "$JOINT_TEST_REPORT" --revision combo-3 --result unknown --environment bench
codex-tui workref handoff --key BUG17 --from soc --to mcu --summary "Review protocol revision r3"
codex-tui workref handoff-text --key BUG17 --index 0
# 只按完整 Target + CODEX_HOME + ThreadId 反查本地关联；不自动接续会话。
codex-tui workref find --target "$SOC_TARGET_ID" --home "$SOC_CODEX_HOME" --thread "$SOC_THREAD_ID" --json
# 只有收到接收方实际确认后，才能标记回执：
codex-tui workref ack --key BUG17 --index 0 --receipt "$RECIPIENT_ACK_REFERENCE"
codex-tui workref show --key BUG17
codex-tui workref list --json
~~~

契约类型为 `protocol/api/abi/schema/config/data/model/other`。Git 管理的契约须提供完整 40 位十六进制 Commit SHA（必要时另外记录子模块 gitlink 版本）。证据类型为 `commit/ci/unit/integration/device/acceptance/other`，结果需明确为 `passed/failed/partial/unknown`，并附环境信息。**分仓单测 PASS 不等于联合 PASS，CI PASS 不等于上电设备或产品验收通过；交接“已准备”不等于“已送达”。** 两个会话别名可以区分不同 Target 上相同的 ThreadId；手工 Target/Home 字段不构成在线身份认证。

不要把客户资料、会话原文、设备日志或凭据写入命令行历史和 JSON 导出。CLI 写入复用 TUI 私有 SQLite 和独占写锁：交互进程已持有锁时宁可报错，也不得静默覆盖。当前没有中央同步、Agent 消息总线或自动写回飞书/TAPD。真实需求一项、真实缺陷一项、至少两类跨仓场景的人工使用与整机联调，**后续集中安排一次验收**；当前自动化测试只证明本地 CLI/存储契约，不代表 Stable 已获许可。

## 证据与数据安全
<!-- docs-section: safety -->

SQLite 保存个人元数据与规划覆盖层；默认**不持久化**用户和助手消息原文。若需本地全文检索，在 config.toml 中配置 `[search] persist_local_transcripts = true`；最多 10,000 条，消息原文和标题分别最多存储 4,096 字符；按首次本地索引时间计算最多保留 30 天（在维护运行时清理）。重复打开同一已缓存消息不会延长其保留期限。关闭此选项后下次启动逻辑删除本地文本/FTS 行，但不保证闪存物理擦除。Codex 仍是对话权威。数据损坏时不得用删除数据库代替诊断，先保留 Store/WAL/SHM 并设计离线恢复。`doctor bundle --output ./codex-tui-support` 需复核隐私；Bug 记录必须带精确源码 SHA。TUI 操作不授权 Stable 发布。

