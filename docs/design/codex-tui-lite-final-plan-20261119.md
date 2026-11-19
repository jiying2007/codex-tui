# codex-tui Lite｜产品范围与落地实施终版方案

**版本：** 方案 L1.0（定稿归档，待实施）  
**日期：** 2026-11-19  
**基线仓库：** `jiying2007/codex-tui`  
**审查基线：** `37ec96292698a17e188c180a011772bdeb086d51`  
**适用对象：** 个人与 1–5 人小团队；主要环境 Windows Terminal／SSH → Ubuntu  
**产品版本：** 另行确定，不把方案版本当作已发布软件版本。

> **归档说明**：这是一份下一阶段的产品及实施决策，**不是功能已开发、设备已验收或 Stable 已发布的证明**。截至审查基线，v1.4 的自动化工作流通过，但 `stableReady=false`、`publicationAllowed=false`；现场 Windows SSH→Ubuntu、真实 Codex、内部 GitLab 以及管理员发布核验仍须单独取得证据。既有历史设计、PR、Release、Tag 和安全回执原样保留。本次归档不直接更改运行产品的功能或权限。

## 0. 一页结论

**终态定位**：一个本地优先、无需团队服务器的轻量研发协作工作台，围绕**研发事项 → 相关会话 → 依赖/交接 → 验证证据**，提高个人和小团队在多仓库、跨模块开发中的准确性与效率。

**只保留五项产品责任**：

1. **发现会话**：精准定位实际 Codex 会话、执行主机、仓库及需要人工关注的请求。
2. **关联事项**：将需求、Bug 或联调事项与一个或少量会话关联；GitHub/GitLab Issue 是已有事项的权威。
3. **关联依赖**：为 SoC↔MCU、驱动↔应用、OTA、KWS 等跨模块工作引用共同协议或接口契约。
4. **受控交接**：以有界摘要、确切目标与明确确认交换行动项和证据引用；不进行无界 Agent 对话。
5. **验证闭环**：准确区分代码已提交、CI 已通过、集成已验证及用户已验收。

**不做**：Git Commit/Push/Merge/Worktree 写入、MR/PR 创建或合并、完整需求管理平台、通用 Agent 编排、后台持续巡检、中央数据库、远程终端管理、任意自动硬件写入。

**路线**：G0 真实试用 → G1 会话与 Attention 最小链路 → G2 单仓需求/Bug 闭环 → G3 跨仓联调和受控交接 → G4 实质性删减及候选资格。**G0 未通过前，不启动破坏性大范围删码。**

## 1. 审查依据与问题根因

| 已核实事实 | 对终版决策的影响 |
|---|---|
| 当前 `main` 是 `37ec962…`；开放 PR 为 0；自动化工程检查全绿 | 证明工程基线，不证明真实用户可用或产品必要性 |
| `src/main.rs` 启动 Git/Forge/Worktree 写入服务，Git/Forge 还存在 10s/15s 刷新 | 必须减少实际服务与轮询，单纯隐藏菜单不是瘦身 |
| `src/app/registry.rs` 和 `src/notification.rs` 依赖 Planning/WorkCard 的部分派生数据 | 先替换原生 Attention 和通知投影，再删 Planning |
| 现有 `thread/resume`、`turn/start`、`turn/interrupt` 及安全请求响应链 | 不能预先删除所有会话控制；先验证原生 CLI 接力 |
| `src/forge_mutation.rs`、`src/worktree.rs` 维护写入与 OutcomeUnknown 恢复 | 退出前必须确认未决回执的安全处置，不能删除数据库记录或假定写入失败 |
| GitHub/GitLab Issue 概览有界且可能不完整，摘要缺少完整验收字段 | 不把列表当需求库；用精确 Issue 链接/ID 获取权威内容 |
| VS Code/Remote SSH 已能操作 Git、分支和 Worktree | TUI 保留最小只读仓库身份，写入交由原生工具 |

**根因**不是 CI 缺失，而是普通用户的“发现、定位、继续、联调、验收”主路径被过多平行入口、后台服务及发布治理包围。功能已实现不能成为保留它的理由。

## 2. 三种标准工作流

### 2.1 单仓需求开发（P0）

**输入**：精确 Issue URL／ID，或明确标为“本地临时事项”的文字描述；仓库、验收条件、影响范围。

**最短链路**：选中事项 → 选择已有会话或显式新建会话 → 明确目标仓库/执行主机 → Codex 开发 → VS Code/Git 提交和 PR → 收集测试/CI → 人工验收并在原平台关闭。

**完成判据**：验收条件至少逐条对应测试或检查证据；“已提交代码”“已合并 PR”和“已验收需求”是不同状态。一个会话即可完成，不强制创建 Case、Board 或跨仓通信。

### 2.2 单仓 Bug 处理（P0）

**最短链路**：Bug 来源 → 实际/预期现象、环境与源码版本 → 最小复现或标记“待复现” → 关联修复会话 → 根因及修复 SHA → 原问题回归 + 相邻场景 → 验证人员确认 → 在原 Issue 平台关闭。

**重要规则**：偶现问题允许“证据不足／待复现”；不能编造根因。Bug 修复代码的 CI 绿灯不等于设备异常已消失。失败或回归复发时，仍关联原始事项和原始证据，不创建重复的第二套 Bug。

### 2.3 跨仓库联调（P1，先用 UART 验证）

**最短链路**：一个外部事项 → 关联 SoC、MCU 两个会话 → 锁定 UART 契约位置与 Git SHA → 双端独立实现和测试 → 用户确认的定向交接 → 共同测试向量 → 模拟联调 → 真实 UART 测试 → 联合证据和验收。

**关键规则**：两个仓库单测通过不等于联调通过。协议版本、双方提交、设备/固件版本与收发记录必须能够对应；不依赖自然语言“我已经完成”判定硬件测试成功。

## 3. 典型开发场景的统一支撑

| 场景 | 共享的契约或关键依赖 | 最终证据 | MVP 决策 |
|---|---|---|---|
| SoC ↔ MCU UART/SPI/I²C/CAN | 帧格式、命令字、字节序、CRC、超时/重试 | 双端测试、真实收发记录 | G3 首选 |
| Linux BSP ↔ 应用 | DTS、IOCTL、设备节点、启动与错误约定 | 驱动与应用回归、实际节点验证 | 复用同模型 |
| Bootloader ↔ App ↔ OTA | 镜像/分区、版本组合、升级与回滚 | 联合版本矩阵、掉电测试 | G3 扩展样本 |
| SoC 策略 ↔ 电机 MCU | 角速度/电流与反馈、保护状态机 | 负载、温升、过流、运动实测 | 复用同模型 |
| Audio ↔ KWS ↔ 端侧部署 | 采样率、特征、模型版本、量化 | 锁定评估及设备 CPU/内存/时延 | G3 扩展样本 |
| 传感器 ↔ SLAM | 时间戳、坐标系、标定/同步 | 回放和整机测试 | 后续复用 |
| 前端 ↔ 后端／SDK ↔ App | OpenAPI、消息 Schema、契约版本 | 消费方契约回归 | 后续复用 |
| 开发 ↔ 测试/CI | 复现步骤、测试环境、失败证据 | 修复 SHA + 回归结果 | G2 内验证 |

这些场景采用**同一个事项关联与证据模型**，不建立每类场景专用模块。

## 4. 信息模型与真正的权威边界

**研发事项 WorkItemRef**：`kind=requirement|bug|integration`、外部 `provider/project/item-id/url`，可选本地草稿。标题、优先级、责任人和最终关闭状态以原 Issue 为权威；本地草稿不得冒充平台事项。

**会话 SessionRef**：`AppServer target + transport/host identity + ThreadId + repo identity + cwd + observed_at`。任何写入或交接前重新查验完整身份；不同 target 即使 ThreadId 相同也不能串用。远程目标路径与本地同名不等于同一主机。

**关联 Link**：一条 WorkItem 关联一个或少量 SessionRef；可标注 SoC/MCU/测试/算法角色，但不生成新的任务调度层。个人默认最多展示 2–4 个相关会话；更多则拆成具体事项。

**契约 ContractRef**：现有 Git 仓库内的文档/Schema、repo + path + 完整 SHA + 可选协议版本；不复制另一份协议真值。旧 SHA 的证据发生漂移时标记“待重验”。

**交接 Handoff**：`id + source/target SessionRef + 摘要 + 需要行动 + 契约/提交/测试引用 + 发送状态`。v0 先“生成/复制/人工转交”；v1 才允许用户显式确认的受控提交。状态至少区分“草稿、待核实、已送达、已处理”；**未知结果不得自动重发**，不得向正在工作的 Turn 静默 steer。

**证据 EvidenceRef**：`类别、来源 URL/路径、仓库 SHA、测试环境/设备版本、验证人或运行 ID、时间、结论`。避免保存令牌、原始敏感提示词、密钥或整段设备日志。引用缺失时显示“未验证”，不自行补全。

**三条互不等价的状态轴**：事项（Open/Closed/Unknown）、会话（Working/NeedsYou/Idle/Disconnected）、验证（NotRun/Failed/Partial/Passed/Accepted）。不得把“Idle”或“PR Merged”映射为“Accepted”。

## 5. 最终界面与交互

**唯一首页**：待处理会话、进行中、最近 20 条，以及用户主动关联的少量研发事项；按需搜索历史。行内仅展示状态/原因、仓库、标题和活动时间；选中项显示完整来源和验证信息。窄 SSH 窗口仍能看到原因，列表选择与滚动不被 Terminal Drawer 挤出屏幕。

**轻详情**：当前会话、来源主机、工作目录、最新结果/请求、关联 Issue/契约/证据；跨仓事项显示双方阻塞，但不提供多个并列的 Forge/Board/Workspace 子系统。

**高频操作**：`↑/↓` 或 `j/k` 选择，`Enter` 打开，`/` 搜索，`Esc` 返回，`?` 帮助；`Alt+T` 切换唯一可选的本地 Terminal Drawer（不使用 F6）。`!` 可选查看最近观察到的提醒。新增跨会话操作须经详情显式确认，不扩展大量键盘快捷键。

**原生交互边界**：优先验证原生 Codex 能否安全接管选中的同一 Thread；若跨客户端接管不可靠，则**条件保留**已有最小回复、审批、停止链路与其权限检查，不做富文本聊天客户端或独立 Agent Runtime。

**外部打开**：Windows SSH 上不假定 Ubuntu 进程能弹出 Windows VS Code。显示准确的仓库路径/URL 与可复制命令/文本，由用户在 VS Code Remote SSH 打开。

## 6. 代码瘦身与架构约束

**保留核心**：App Server 会话注册与订阅、有限分页、状态/请求索引、错目标拒绝、精简 Registry、最小输入安全边界、终端安全恢复（若真实试用证明有价值）、只读 Git 仓库身份、有限脱敏 Doctor。

**先停用后移除**：Forge Mutation、Worktree Mutation、Launch Presets；普通用户不再通过 TUI 创建/合并 PR/MR、切换分支、删除 Worktree。Git/Forge 全局 10s/15s 刷新停止，替换为所选事项按需只读查询。

**先迁移后删除**：`Planning/WorkCard`、Board、Saved Views、Scratch、Goal 派生层与相关 SQLite 表。当前 `thread_needs_attention` 和通知依赖 WorkCard，必须先构建原生会话/request 状态投影并验证无漏报，再逐个摘除。旧 Pending/OutcomeUnknown 操作回执需隔离、备份和可解释处置，不能通过直接删除掩盖不确定性。

**本地存储**：首轮复用现有受保护的 SQLite/原子备份路径；最终只存置顶、有限偏好、事项/会话关联与交接回执。是否改为 JSON/TOML 必须有确凿收益，不能为“轻量”重建存储系统；第一次真正部署前可制定清晰的新 schema，不承诺未部署旧版本的虚构兼容，且不能静默擦除已有测试数据。

**工程结构**：一个 Rust 二进制、同一 Ubuntu 上一个默认本地 stdio App Server、一个 TUI 主界面、少量按需读取；不新增服务、团队账号、中心数据库、消息 Broker、长期 Agent 调度、自动巡检。大部分 Git 与 CI 依赖现有工具和仓库。

**新范围治理**：现行 `release/v1.4-plan.json` 的 216 源码模块 ratchet 和旧 Stable 条件针对现有产品。完成 Lite 重构时必须明确重定范围与对应验证门禁，在受保护 PR 中替换不适用的旧功能资格；不得简单关闭安全检查、破坏历史 Release/Tag，或把未完成的用户验收改写为 PASS。

## 7. 安全与失败语义

1. 任何真实操作以具体 target、主机、repo、Thread、Turn/Request 与必要的 SHA 绑定；身份不明、过期或变化则拒绝。
2. 用户交接默认先编辑与预览；实际发送只做一次，结果不确定即待核实，不自动重放；接收会话活跃时默认排队人工处理而非静默打断。
3. 编码器/校验器的回执与真实 UART/OTA/机器人验证分层；硬件烧录、OTA、运动控制、危险设备测试仍需原审批与操作流程。
4. 未观察到完整历史、丢失连接或读取失败时明确标注 Partial/Unknown，不把“无数据”当作“无待办”。
5. GitHub/GitLab Issue/PR/CI 和 Git/仓库文件仍是权威；不在 TUI 内重建批量写入能力。
6. 默认不持久化聊天原文、凭据和私有日志；本地关联信息与证据引用需可查看、可清理、可脱敏导出。
7. 现场资格须使用真实账号与 SSH/设备证据；CI 测试、合成数据和 `--fake` 不得冒充真实用户或硬件通过。

## 8. 按关口实施，不再无限扩张

| 关口 | 本关口真正交付 | 退出条件 | 禁止以什么替代 |
|---|---|---|---|
| **G0 首次真实可用** | 可信预构建包、Ubuntu 实机运行、连接真实 Codex、识别至少两个仓库/会话、准确 CWD、终端与退出 | 一条实际用户操作链可重复，准确记录失败与原因 | 用新 CI、文档、fake 演示代替 |
| **G1 会话精简** | 一个工作台+轻详情+原生 Attention/提醒；按需 Git 只读；必要安全交互验证 | 状态正确、无漏待办、无错目标、窄 SSH 可操作 | 仅隐藏旧菜单 |
| **G2 需求/Bug P0 闭环** | 外部 Issue 精确关联或本地临时事项、验收要求、1 个会话、测试/修复 SHA 证据 | 单仓需求和 Bug 各完成 1 次真实闭环 | 在本地再建 Issue 管理器 |
| **G3 跨仓协作 P1** | 2–4 会话关联、契约 SHA、人工确认定向交接、版本/阻塞与联合测试引用 | SoC↔MCU UART 完成一次交接及真实验证，OTA/KWS 复用验证 | 自动 Agent 对话或无限轮询 |
| **G4 去冗余与发布判断** | 移除无用后台写入/规划/数据结构和相应测试，收口 Lite 门禁与真实效率对比 | 工程与实测均满足、授权/发布范围独立明确 | 统计删码量或用过时 v1.4 Stable 门禁评判 Lite |

每关口只保留一份明确的 PR/验收回执；不同关口并行只限不会造成权威漂移的只读准备。**G0 无真实结果时，先改善安装与诊断，而不是启动 G3 或重构新平台。**

## 9. 客观验收与投入退出规则

| 类别 | 最小客观标准 |
|---|---|
| 首次安装 | 可信源码 SHA 与包校验；目标 Ubuntu 直接运行，无需本地编译 Rust |
| 主机和会话 | 至少 2 个实际仓库；一百次导航切换的错目标为 0；跨 host/target 同名 ID 与 cwd 不串位 |
| Attention | 审批、用户输入、错误、断连的真实或受控注入案例可区分，不漏报已观测待办，不伪造全局完整性 |
| 需求 P0 | 一条需求：原始验收条件 → 代码/PR → 测试证据 → 人工验收可追溯 |
| Bug P0 | 一条 Bug：复现条件/待复现说明 → 根因证据 → 修复 SHA → 回归 → 验证人结论 |
| 跨仓 P1 | 一条 UART 联调：协议 SHA + 双端 SHA + 双端软件测试 + 真实串口记录，状态分层准确 |
| 安全 | 未知交接结果零自动重发；无未授权 Forge/Git/硬件写入；无错误目标审批与错误主机终端 |
| 实用性 | 对比原生 Codex + VS Code 约 10–20 个真实场景，记录完成率、操作步数和中位定位耗时；25% 节省仅为观察目标，非虚构 PASS |
| 性能 | 有界大列表性能与键盘响应回归；p95 50 ms 是本地交互目标，不能冒充远端响应 SLA |
| 发布 | 软件范围清晰、真实部署见证和安全/授权证据齐全；`publish=false` 的打包成功不等于 Stable 发布 |

**停止／退出条件**：如果 G0 无法通过，停止加新功能，集中解决部署；如果 G2/G3 真实场景与原生工具对比没有持续的效率或准确性收益，停止新增协作功能，收缩为只读 Session Radar；若雷达价值也无法证明，停止独立产品研发，仅保留仓库与历史证据。

## 10. 决策边界、尚未确认项与下一步

**已定事项**：一屏主工作台；需求/Bug P0；跨仓联调 P1；只读 Git 身份；外部 Issue/CI/Git 权威；人工确认交接；不建中心服务；以真实操作验收而非测试数量决定继续投入。

**待现场核实，不擅自承诺**：原生 Codex 与 TUI 是否能无冲突接力活跃 Thread；SSH 用户真正的账号和 App Server 连接；Windows SSH 下 `Alt+T` 的可用性；内网 GitLab 版本与权限；最终设备试验所需授权。只有真实证据才解锁对应功能或发布资格。

**下一步唯一优先级**：在当前发布门禁下取已验证的 Linux 预览包，先完成 G0 真实使用，记录定位失败与实际操作过程；G0 没完成之前不要再次创建宏大的“全面终态”功能列表。

## 11. 归档与来源

**源代码依据（只读核对）**：`src/main.rs`、`src/app/registry.rs`、`src/notification.rs`、`src/app_server.rs`、`src/app_server/subscriptions.rs`、`src/forge_mutation.rs`、`src/worktree.rs`、`src/sqlite_schema.rs`、`release/v1.4-plan.json`、`release/v1.4-completion.json`；均以本方案首页的精确 main SHA 为准。

**现行历史依据**：`docs/design/final-plan.md`（2026-09-29 历史设计）、`docs/adr/012-upstream-convergence.md`（2026-10-08）、`docs/zh-CN/implementation/v1.4-first-deployment-baseline.md`。与 Lite 新范围冲突时，以新版本代码落地后的正式 ADR/发布标准为准；**本归档尚不改变现行 v1.4 的运行、CI 或发布约束**。

**外部参考**（按核对时访问的官方页面，未来需定期再验证）：

- Codex App Server 与官方集成界限：https://developers.openai.com/blog/codex-as-a-platform
- Codex App Server 的 Turn/Resume 应用接口说明：https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server
- VS Code Git 分支和 Worktree：https://code.visualstudio.com/docs/sourcecontrol/branches-worktrees
- VS Code Git Remote 操作：https://code.visualstudio.com/docs/sourcecontrol/repos-remotes
- GitHub 可复用工作流：https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows

**归档规则**：此文件记录一次冻结决策（方案 L1.0）。以后改变终态范围，必须写出需要变化的实际用户流程、原有方案冲突、成本和实测证据；不要反复覆盖本文件的历史结论。软件实施通过独立 PR 完成，未验收项目一律记录为未完成。