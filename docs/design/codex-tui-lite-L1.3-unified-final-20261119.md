# codex-tui Lite L1.3｜基于真实 Codex 使用记录的统一终版方案

**日期：** 2026-11-19  
**适用对象：** 个人及 1–5 人小团队；Windows、Windows Terminal → SSH Ubuntu、Linux、VS Code Remote SSH、原生 Codex CLI。  
**性质：** 下一阶段产品决策、技术设计和实施验收规范，**不是已实现、已部署或已发布的功能声明**。  
**审查代码基线：** `jiying2007/codex-tui@1b9bc387f7df8a3d43ad23a46fda7b56192cd274`（L1.2 #418 合并后）。  
**前置归档：** [L1.1 真实需求平台方案](codex-tui-lite-final-plan-L1.1-feishu-tapd-20261119.md)、[L1.2 账号会话证据](codex-tui-lite-L1.2-codex-sessions-evidence-20261119.md)。  
**方案优先级：** L1.3 对**下一阶段**产品范围、设计与验收具有单一解释优先级；L1.1/L1.2 保留为来源和历史，不并列作为实施清单。当前软件行为以实际 `main` 和已部署版本为准，旧 ADR/Release 仍是历史事实，不能据本方案声称 Stable 资格成立。

> **一句话定位：Codex CLI 负责实际开发；codex-tui 是跨仓库、跨执行目标的会话定位与注意力入口，提供最小事项关联和验证证据，而不成为另一个研发管理平台。**
>
> **唯一日常主路径：发现重要工作 → 定位准确 Thread/主机/仓库 → 回到原生 Codex → 处理阻塞 → 引用代码与测试证据 → 返回原飞书项目/TAPD 事项。**

## 1. 决策摘要：只承担五件事

1. **Find**：找得到用户真正需要的会话；主会话、子 Agent、历史档案不混淆；准确知道哪个主机、Target、仓库、工作树。
2. **Attention**：真正需要批准、输入或检查异常的会话优先；不能被子 Agent 折叠或后台失败淹没。
3. **Resume/Locate**：安全进入正确的原生 Codex Thread；无法接管时提供准确定位，不伪装成功。
4. **Link/Handoff**：一条真实需求/缺陷/联调事项引用 1–4 条会话、共同接口契约和显式确认的交接摘要。
5. **Evidence**：清楚显示代码、CI、集成测试和业务验收各自的证据，不替其他系统判定“完成”。

**明确不做：** 独立 Agent/Goal/任务编排、长链多 Agent 对话、第二套对话编辑器、重复 Thread Queue、团队账户与中央数据库、常驻多主机扫描 daemon、后台定时巡检、完整 Feishu/TAPD 工作流、Git Commit/Push/Merge/Worktree 写入、PR/MR 自动创建与合并、远程任意 shell 管理、未经批准的硬件烧录/OTA/运动控制。VS Code、Codex、Git、GitHub/GitLab 和飞书项目/TAPD 各司其职。

**对历史版本的决策：** 现有代码实现多，不代表都值得保留；先证实新主路径可靠，再有顺序删除旧模块。旧测试门禁不能通过直接关闭的方式绕过。

## 2. 真实证据及其边界

### 2.1 私有会话档案的全量目录规模

**最新真实审计证据（2026-11-20）：** 维护者上传了本机 `audit-v3.1.aggregate.json`；见 [v3.1 全量对象哈希与内容异常审查](../reviews/2026-11-20-independent-v3.1-real-audit.md)，之前 [v2/v3 对账](../reviews/2026-11-19-v2-v3-real-audit-reconciliation.md) 保留为版本历史。v3.1 已对固定快照 **2,170/2,170 个 Git 对象完成哈希验证、2,168/2,168 个候选组完成读取尝试**，且修复了 v3 的唯一双 UUID 漏检；**`status=PARTIAL` 仍然正确**，因为 2 个零字节档案、32 组含 67 条无效 JSON 的部分异常以及 79 组重复元数据仍待定性。

对 `jiying2007/sessions` 2026/04—11 目录递归枚举：**2,170 个 Git blob 文件对象、200 个有记录日期目录、累计 8,274,174,477 bytes**。分类：普通 `.jsonl` 1,875 个、独立 `.jsonl.zst` 291 个、压缩分卷 4 个（两组），**归并后 2,168 份候选 rollout 档案**；**2 个零字节 JSONL**（2026-08-18、2026-09-02），其相同 Git Blob SHA 不能证明有效会话重复。全体文件名日期与所在目录一致；249 个不小于 8MB 的文件占总字节约 72.2%。

**这不是 2,168 个有效且互不重复的 Thread，更不是账号全部对话的证明**：一些会话有父子 Agent、压缩分卷或缺损，另有未上传主机/时间窗口的可能。Git 仓库 size 与 blob 累加量属于不同口径，不混写。

L1.2 之前对 112 份非压缩档案抽取 `session_meta`，并抽样分析长会话；这仅是历史抽样。新收到的 v2 真实审计把来源分布扩展到 **2,166 份非空档案**：明确子 Agent 747、其他子 Agent 12、VS Code 669、CLI 481、Exec 230、Unknown 27；其结果仅代表当前已上传的归档集合，**不能外推为当前 TUI 漏报率、全部主机覆盖率或业务完成率**。

**内容审计最新状态：** v3.1 已在授权本机完成 8,274,174,477 字节的已知 Git 对象哈希核对和全部 2,168 候选组扫描，解码 2,166 个非空组、解析 3,041,293 条有效 JSON 记录；67 条无效记录中 64 条为 JSON 语法错误、3 条为无效 UTF-8；79 个组有重复 `session_meta`，其重复 ID 被合并归类为“不同或缺失”，**不能直接声称 79 个真实 ID 冲突**。已知哈希全通过并不代表正文无损，且无法证明全部用户主机均已上传。任何在线接续仍以真实 Target 身份为准；独立归档审计不进入 TUI 开机或轮询流程。

### 2.2 已证实的产品需求

| 会话证据/真实工作 | 对应必须解决的问题 |
|---|---|
| Windows 本机 MCU 工程；Linux/SSH SoC、MCU、算法工程 | 相同路径/仓库名不得跨主机误定位；支持异构路径显示 |
| 同一工程中的 VS Code、CLI 和 Exec 来源 | 来源字段与当前交互入口分离，不以 `originator` 替代实际 Client |
| 父子 Agent、自动 Review/Guardian | 默认汇总到主会话，审批/错误仍可达；禁止把 Worker 当独立业务事项 |
| 历史数千条会话、长会话 compact、多次 Turn | 最近窗口 + 精确搜索 + 结构化进度，不能一次平铺或复制原始日志 |
| SoC↔MCU、Boot↔App↔OTA、BSP↔App、Audio↔KWS、服务/API 之间 | 关联多个真实会话、按需版本化契约、定向交接和联合验证；不将 UART 作为产品边界 |
| 产品与测试使用飞书项目/TAPD | 原平台权威、手动精确链接优先；不把 GitHub/GitLab Issue 当默认业务入口 |
| VS Code/Git 已能操作分支、Commit、PR | TUI 只显示最小只读仓库上下文，不制造重复写入流程 |

### 2.3 官方 Codex 已有能力：适配，不复制

当前上游协议源码提供 `sessionId`、`parentThreadId`、`originator`、`source` 等字段，并有 `thread/list`、`thread/search`、`sourceKinds`、`currentAccountOnly`、`readState` 等能力/实验字段。参考：
- [Thread 数据结构](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/thread_data.rs)
- [Thread 查询协议](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/thread.rs)
- [App Server README](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)

**上游 `main` 能力 ≠ 用户已安装的 Codex 版本有该能力**。协议 adapter 需要在实际 Target 运行时探测、容错、降级；实验字段不能作为通用必需字段。优先复用原生搜索/已读状态/子 Agent 列表，只有证明缺失或不可靠时才做小范围本地派生投影。`currentAccountOnly` 也不等于所有历史记录都可严格按账号隔离，不能代替本地访问控制。已核对官方协议：`thread/list` 的 `sourceKinds` **省略或为空均默认交互来源**；`archived` 默认只列非归档，`modelProviders` 可显式设置筛选，**显式空数组表示全部 Provider**。不同已安装版本的默认行为仍须能力探测；未确认来源/归档/Provider 范围及分页完整性时，不允许宣称会话全覆盖。

## 3. 终态操作界面（仅一个 Mission Control）

默认首页不再是完整 Thread 平铺列表；按照 **Needs You → Pinned → Working → 最近 20 条主会话** 汇总，去重后显示。子 Agent 在父会话行内显示数量和是否有待处理；自动 Guardian/纯只读 Worker 默认折叠，可手动展开和搜索。无父会话或源字段不完整时，标为“孤立子会话/待核实”，**不能静默隐藏待处理请求**。

示意（文字，不代表当前代码已实现）：

```text
Needs You  2  | Working  3 | Recent 20 | Target: ubuntu-A
> [!]  SoC 应用 / UART 接口       需要批准  (子 Agent 2)
  [!]  MCU 固件 / UART 协议       测试失败  (子 Agent 1)
  [*]  KWS / Hey Mero             运行中
       Audio / AEC               等待核验
  / 搜索会话   Enter 打开   ? 帮助
------------------------------------------------------------
选中详情：Target · 来源 · 仓库/工作树 · CWD · Thread 身份
最近一次真实事件 · 待操作对象 · 事项链接 · 契约 SHA · 证据
```

**只保留高频键：** `↑/↓` 或 `j/k` 选择、`Enter` 定位/打开、`/` 搜索、`Esc` 返回、`?` 帮助；`!` 可查看最近提醒。`Alt+T` 是可选的终端焦点切换，保留经现场验证的 `t` 或 `Ctrl+]` 等兼容替代；**不用 F6**。不要新增一套难记的快捷键。切换会话时保留筛选和选中位置，事件更新不抢焦点。

**窄终端：** 60×24、80×24 和 100×30 必须能看到状态、理由和选中行；详情可折叠；Terminal Drawer 不得遮挡滚动条/选中行。Windows Terminal 经 SSH 无法当然弹出 Windows 上的 VS Code；优先显示可复制的准确路径、目标和原生打开方法。终端能力差异按检测结果提供替代，不以某个按键必然穿透 SSH 为前提。

**检索规则：** 首页只查当前活动且可达的 Target；`/` 优先利用原生搜索和已有有界索引，按能力选择 `recency_at` 等最近活动排序（不把默认 created_at 误作最近工作），无法取得准确活动顺序时标注依据；“全部已配置目标”必须由用户主动选择、分目标有界查询并展示 Partial；历史档案只在明确切到“历史引用”范围时加入。默认不在启动时扫描 8GB，也不把归档命中当作在线可恢复 Thread。

## 4. 身份、来源和状态：三者必须分离

**SessionKey = 已验证的 TargetIdentity + canonical ThreadId**。单独的线程 ID、窗口标题、仓库 URL、CWD、机器别名或 SSH 隧道名都不够。TargetIdentity 包含用户配置目标、可信连接端身份/作用域、`CODEX_HOME` 所在存储作用域、当前账号/Provider 边界（若可验证）和能力信息；这些均应以最小脱敏 fingerprint 表示，不保存凭据。未能验证时拒绝高风险操作。连接别名用于展示，不是主机真实性证明。

**GroupKey 只用于明确的父子 Agent 折叠**：优先由同 Target 下可核实的 `parentThreadId`（及版本支持时的直接附属关系）构建树；字段缺失则不猜测。`sessionId` 是关联的会话树标识，**Fork 可能共享 `sessionId` 但仍是独立可执行工作**，仅作为相关会话导航，不能据此合并工作/审批；`forkedFromId` 只是分叉沿革。跨 Target 同 ID 一律不自动合并，孤立子 Agent 的待办仍须可达。

**RepoRef** 分开记录 Git 仓库身份、工作树根、原始 CWD、平台路径类型、可达性和观察时间；跨仓/子模块协议还要引用对应 submodule gitlink SHA 与双端提交，不能凭同名分支认定接口一致。非 Git 项目会话合法。跨主机同名目录绝不能映射为同一写入地点。

**来源字段：** `source` 表示来源种类，`originator` 可能是创建时记录的发起客户端，`threadSource`/角色用于更细分的自动子线程/Guard；这些字段都不等于“当前正在由谁操作”。不认识的枚举保留为 Unknown，并显示原值的安全摘要，而非丢弃记录或把 JSON 原样堆在列表。

| 独立状态轴 | 可用状态 | 禁止推断 |
|---|---|---|
| 连接/可信度 | LiveConnected / Disconnected / ArchiveOnly / Partial / Unknown | 文件存在不代表仍可 Resume；断线不等于 Idle |
| 会话/Turn | Working / NeedsYou / Ready / Idle / Error / Unknown | Turn complete 或 Idle 不等于功能完成 |
| 业务事项 | Open / InProgress / Closed / Unknown（以原平台为准） | PR 已合并不等于需求/缺陷关闭 |
| 验证 | NotRun / Failed / Partial / Passed / Accepted / Unknown | CI PASS、模拟 PASS 不等于真实设备或人工验收 |

当前官方 ReadState 可用且权威时，优先采用；不可用时本地“已看过”仅是 UI 偏好，不冒充云端/其他客户端同步已读。

## 5. 实时发现、原生接力、归档检索：三条链路

**Discover（实时）：** 复用既有 `app_server_target.rs` 的 stdio/WebSocket/Unix Socket 目标配置，优先一个活动目标。对目标版本能力探测后明确区分交互、Exec、App Server、子 Agent、自动 Review 等来源；分页必须到结束游标，Provider/归档范围按用户确认展开，未覆盖范围标为 Partial。多目标按需串行/有界并发取只读摘要，结果保留 target 标签、抓取时间和完整性；某目标失败不得拖垮本地首页，也不能暗示该目标无待办。不增加常驻团队服务或后台轮询。若要远端安全连接，沿用现有 TLS/Loopback SSH Tunnel 和认证边界。

**Resume/Locate（接力）：** 选择 Thread 后重查目标可达性、Thread 身份、当前工作目录、原保存 Provider 与原生接管能力；版本支持时优先 `thread/resume` 的 `excludeTurns: true` 和 turns/items 有界分页，不加载整段历史；出现 `modelProviderMismatch` 明确提示仅可读历史/需返回原 Provider，**不得自动 fork 冒充接续**；优先返回原生 Codex CLI 的准确会话或可复制的恢复/定位指令。不能保证 VS Code ↔ CLI 直接接管时，显示“仅定位/需在原客户端继续”，不能启动另一条同名会话冒充成功。若保留内嵌最小回复/审批/中断，则只限真实验证不可靠的接力场景，沿用既有审计与请求身份安全检查。

**Archive（历史）：** 一次性离线读取用户明确提供的私有档案，生成有界脱敏元数据，默认不保存正文、完整 Prompt、凭据或私有设备日志。查询结果明确为“ArchiveOnly，记录来源/完整性/最后观察时间”，不自动更新 Live 的会话状态。零字节、残缺、分卷或缺少终止事件均标不完整；无正式身份验证不得把跨主机同 Thread ID 的档案合并。

**LastProgress（长会话）：** 从结构化事件提取最近 Turn 的开始、结束、取消、最后活动、待请求和最近可靠证据引用，不发起额外模型调用。`task_complete` 只是 Turn 已结束；缺尾事件是 Unknown，不自动标记 failed/success。允许手动查看原始来源（须有权限），首页不显示工具调用流水账。

## 6. Attention：先保正确，再谈聪明

待处理来源按**真实审批/输入请求、明确系统/测试阻塞、用户待核对结果**优先；工作中与完成后无待操作的后台 Worker 不刷屏。子 Agent 需要用户确认时，父组必须展示聚合提醒，同时能一键定位**真实子 Thread/Request**；不能在父 Thread 上错误回复，也不能因为折叠吞掉它。

**发现完整性前提：** 已上传档案的约 35% 是子 Agent 来源，Exec 另有 230 份，**但这不是实测在线漏报率**。当前 `src/app_server.rs` 已按 `recency_at` 排序，却未显式提供 `sourceKinds`；官方 `thread/list` 不传来源或传空数组均默认交互来源。adapter 必须在目标版本支持时显式请求可用来源（含 Exec/Sub Agent/Review）、按需切换 archived/Provider 并完成全部页；不支持时标注具体缺口为 `Partial`，绝不把未检索到的子 Agent 审批当作零待办。

- 每条 Attention 带 `target + ThreadId + Request/TurnId + observed_at + reason`，过期必须重验。
- 只有确实送达的事件可标“已通知”；BEL、Windows Toast、视觉提示的语义分别记录。静音不隐藏页面待办。
- 连接丢失、历史不完整和限额不可判断时显示 Partial/Unknown；不能把“没查到”变成“无待办”。
- 通知既不能自动授权、自动回复，也不能自动切换用户焦点。
- 先从 `Planning/WorkCard` 迁出当前 `thread_needs_attention` 和通知必要依赖，再删派生层；保持实测无漏报、无错目标。

## 7. 研发事项来源与最小数据模型

**外部权威：** 产品/测试在**飞书项目或 TAPD**提出和验收需求、缺陷；TAPD 即使从企业微信打开，权威仍是 TAPD；GitHub/GitLab 负责仓库、PR/MR、CI 而非默认产品工单；Git 负责源代码及协议；Codex 负责会话生命周期。所有来源状态独立，不做自动双向同步。

**只需五类本地引用**（复用现有 SQLite，禁止为了轻量重写存储栈）：

| 类型 | 只存最小必要字段 |
|---|---|
| `SessionRef` | 已确认的 TargetIdentity、ThreadId、父子/SessionId、来源、可达性观察时间 |
| `WorkItemRef` | provider（Feishu Project / TAPD / LocalDraft）、space/workspace、type、ID、原始 URL、可选只读缓存修订 |
| `Link` | 一条事项默认突出展示 1–4 条 SessionRef；更多折叠检索（不作硬性总数限制），每条含职责/角色；不复制外部任务状态权威 |
| `ContractRef` | 可选契约：kind（协议/API/ABI/Schema/配置/数据/模型等）+ 权威来源 + path/URL/工件 ID + 可验证版本；Git 契约记录完整 SHA，子模块适用时含 gitlink SHA；不一致或无法核验标待核对 |
| `EvidenceRef / Handoff` | 提交/测试/设备版本/时间/结论来源；交接摘要、目标、显式确认及结果 Unknown 回执 |

**MVP 事项接入：** 用户从实际飞书项目/TAPD 复制原始工作项 URL/ID，人工选择平台与类型，准确关联已有会话；不要求管理员 API 凭证，不根据标题猜工单 ID，也不从普通飞书 Wiki URL 假装解析出飞书项目工单。若双方实际使用两平台，分别试用手动引用；API 只读 adapter 在真实节省时间、获管理员授权且接口字段得到确认后再加。平台更新、评论和状态流转在原系统完成。

**隐私：** 私有工单内容、客户信息、日志、会话原文不导入公开仓库，也不能因为开发者可看网页就自动允许所有 Agent 查看。用户显式选择脱敏摘要和可分享范围。

## 8. 三个完整交付路径 + 复用场景

### S1 单仓需求（P0）

飞书项目/TAPD 原需求 → 复制真实 URL/ID → 关联已验证的仓库/Target/主 Thread → 原生 Codex 开发 → Git/PR/CI 证据 → 验收条目对应检查结果 → 产品/测试在原平台确认。纯技术讨论、周报、知识查询无需强制创建工单。

### S2 单仓 Bug（P0）

原缺陷及复现环境、固件/软件版本、实际/预期 → 定位修复会话 → 可复现则记录测试，偶发则明确待复现 → 修复 SHA + 原场景和相邻场景回归 → 测试复测 → 原平台关闭。CI PASS 不等于设备 Bug 已消失；失败重启应关联原缺陷和旧证据，而不是创建第二套 Bug。

### S3 通用跨仓协作与联合验证（P1）

真实研发事项（可来自飞书项目/TAPD，也允许明确标识的纯技术协作）→ 手工关联 **2 个及以上仓库 / Target 的真实会话** → 按需要引用接口/API/ABI/协议/Schema/配置/数据/模型等**可核验版本的共享契约**（Git 管理的契约用完整 commit SHA，涉及子模块时加 gitlink SHA；非 Git 工件用来源权威的版本/摘要）→ 各方独立开发与验证 → 用户确认最小交接摘要并按准确目标手工复制 → 由接收方明确确认 → 执行场景适用的真正联合验证 → 记录每个仓库/工件版本、测试环境、失败原因、完整证据与人工结论。

四项通用能力仅为 **Session Link / Contract Reference / Directed Handoff / Joint Evidence**，不建立 Agent 消息总线、第二套任务平台或 UART 专属模块。契约文件是*按需要引用*，不能强制每个协作都有单独的协议文件；默认突出显示 1–4 个会话，但不是硬性上限。复制摘要不等于消息成功送达，分仓单测 PASS 也不等于联合 PASS。验收明确区分 Passed / Failed / Partial / Unknown。

跨仓真实场景采用不同类型覆盖，而不是只复用同一种接口：
- **跨设备/驱动接口**：SoC↔MCU 或 BSP↔App，支持实际所需 UART、SPI、I2C、CAN、USB、共享内存等，记录驱动/固件版本和对应设备或仿真收发证据。
- **跨软件组件/服务**：BSP↔HDI↔App、前端↔API↔后端或服务间 RPC/MQTT，核对 API/ABI、Schema 和跨组件回归。
- **跨数据/算法流水线**：Audio↔KWS↔部署、Sensor↔SLAM↔Navigation，核对数据/特征/模型/坐标/时间戳等版本契约与集成评估证据。

交接 v0 只“生成、复制、人工粘贴”，复制不自动等于送达；未来若允许直接投递，需目标/权限校验、一次性提交、回执与 OutcomeUnknown 不自动重试。**不做无界 Agent 间聊天或自动打断工作中的 Turn。**

### 其余场景共用同一模型

| 场景 | 共享契约 | 关键验收证据 |
|---|---|---|
| Linux BSP ↔ App ↔ HDI | DTS、IOCTL、设备节点、API/ABI | 实际设备节点与应用回归 |
| Bootloader ↔ App ↔ OTA | 分区、镜像、版本与回滚 | 双端组合、掉电、升级/回滚 |
| SoC ↔ 电机 MCU | 速度、电流、保护及故障码 | 负载、温升、过流、运动实测 |
| Audio ↔ KWS ↔ 端侧部署 | 采样率、特征、模型/量化版本 | 锁定评估、设备资源和实时性 |
| 传感器 ↔ SLAM | 时间戳、坐标、标定 | 回放和整机验证 |
| 前端 ↔ API ↔ 后端 | Schema、OpenAPI、兼容版本 | Consumer 契约测试 |
| 服务/CI/审查 | 复现步骤、基线 SHA、流水线 | 原始 CI 结果和人工复核 |

以上不引入任何新专属“联调模块”“KWS 看板”“OTA 工作流”，只是 WorkItemRef + SessionRef + ContractRef + EvidenceRef 的不同实例。

## 9. 最小技术架构与实际代码落点

```text
      Codex App Server（按 Target 的当前在线事实）
                       |
       适配/能力探测 + 有界查询 + 目标校验
                       |
       Session Registry / Parent Group / Attention
           |                 |                 |
      Mission Control   最小只读 Git     外部事项/证据引用
           |                 |                 |
        原生 CLI        Git/Forge 链接       SQLite 小型元数据
                       |
       历史档案：只在可信本地一次性离线统计（不进启动链）
```

**工程边界：** 一个 Rust 二进制、一个主界面、默认同机 stdio App Server，少量按需 Target；不另设 HTTP 后端、消息 Broker、统一账号中心或大规模跨主机同步。当前源码可复用：

| 代码位置 | 下一步职责 | 风险/特殊要求 |
|---|---|---|
| `src/codex_protocol.rs` | 有界可选解析 SessionId/Parent/Source/Originator、兼容未知字段 | 支持不同 Codex 版本；不要直接强依赖实验字段 |
| `src/domain.rs` | SessionKey/GroupKey/来源/连接状态与基本证明 | 同 ID、同路径、同仓库跨目标不碰撞 |
| `src/app_server_target.rs`、`src/app_server.rs`、`src/main.rs` | 复用 named target，单活动目标优先，按需读取其他目标 | 连接丢失、过期和错误路由必须 fail closed |
| `src/app_server_registry.rs`、`src/app/registry.rs` | 目标内去重、父子聚合、稳定筛选/Attention | 不能用全局 ThreadId Map 汇总多个 Target |
| `src/ui/registry.rs`、`src/app/target_locality.rs` | 简单首页/详情、窄终端、远端路径防护 | Terminal Drawer 不遮挡，远端目录不冒充本地 |
| `src/notification.rs` 及相关状态 | 只保留真实会话 Attention 的可靠提醒 | 不把 Quiet 解释为消失 |
| 现有 SQLite/Doctor | 置顶、偏好、WorkItemRef、证据、回执；脱敏诊断 | 数据迁移原子备份，敏感字段默认不保存 |

**明确分阶段退出：** Forge Mutation、Worktree Mutation、Launch Presets、Planning/WorkCard/Board/Saved Views/Scratch/Goal、独立 Queue 的重复 UI、多余 10s/15s Git/Forge 全局刷新、无价值的并行视图。顺序必须是**建立替代 → 证明没有漏提醒/丢回执 → 停用写入口 → 执行迁移/退役 → 删无用代码和测试**。保留未知结果的历史回执和备份；不能简单丢弃 `Pending` 或 `OutcomeUnknown`。

因为 v1.0/v1.4 尚未实际部署，**无需伪造面向未部署旧版本的永久兼容**；但当前真实设备或试用生成的状态和历史 Release/Tag 不得静默删除。`release/v1.4-plan.json` 与旧 Stable Gate 针对旧范围，重构时以受保护 PR 正式替换为新范围门禁，不能临时关闭安全检查或未经现场验收直接宣称 Stable。

## 10. 实施顺序：五个必须退出的关口

| 关口 | 交付内容 | 必须取得的客观证据 | 失败时 |
|---|---|---|---|
| **G0 首次真实可用** | 可信包、Windows Terminal→SSH Ubuntu、真实 Codex、两个不同仓库、正确 CWD、终端退出和原生接力 | 真实操作录屏/脱敏记录、精确二进制摘要、错误/恢复原因 | 暂停 Lite 破坏性删码，先修基本体验 |
| **G1 会话定位/Attention** | 主/子分组、Target、最近 20+置顶+待处理、搜索、最近进展、未知状态、80×24 导航 | 真实与合成覆盖 CLI/VS Code/Exec/747 个已观测父子关系的类别、Guardian、Fork、不同 Provider、两个不同 `CODEX_HOME`/Target；来源/归档分页缺失明确 Partial，审批准确路由；无错目标、无请求漏显、无焦点丢失 | 不开 G2 扩展，不把模糊状态当成功 |
| **G2 真实需求/Bug** | 手工精确关联飞书项目/TAPD 原事项、提交和测试证据 | 真实产品需求 1 项、测试缺陷 1 项；若两平台均在使用则各测入口，验收由原责任人完成 | 保持手动入口，不提前做平台 API |
| **G3 通用跨仓协作与联合验证** | 两个及以上仓库/Target 的真实会话关联、按需契约引用、人工定向交接与联合证据 | 至少两类不同的跨仓真实场景，包含跨设备/软件/数据之一及另一不同类别；追溯各端版本、目标确认与联合结果，人工验收 | 不把各仓单测 PASS 冒充联合 PASS；证据不足标 Partial/Unknown |
| **G4 删除冗余/发布判断** | 废弃写入、Planning、重复 Queue/Board 与轮询；完善回执、迁移、Doc、Gate | 精确源码/测试结果、数据备份恢复、真实体验优于原生 CLI+VS Code；另核 Stable 资格 | 若效益不成立，只保留最薄会话定位或停止扩张 |

**顺序不是“所有功能都实现再试用”**：G0 必须第一；G1 完成后才允许对核心结构做破坏性精简；G2/G3 可先由外部工具完成人工流程验证，再逐步最小化产品实现。不能用 mock、CI 或文档充当真实设备与用户验收。

### 10.1 代码实施 PR 建议

0. **G0 已有现场基线**：维护者确认既往 Windows SSH/Ubuntu 与 Codex 基本链路已验证，不再要求重复全项 G0；当前功能变更仍需 exact-SHA CI、回归和独立 Stable 资格。
1. **PR-A（无破坏）**：适配可选 Thread 字段 + 来源归一化 + fixture 测试；保持原 UI 和运行功能可用。
2. **PR-B（核心替代）**：SessionKey + Parent Group + Attention 原生投影；混合来源、父子请求、断链/重连与错 Target 回归。
3. **PR-C（交互）**：唯一首页和轻详情、原生接力、60/80 宽 SSH、状态/最近进展；复用 G0 既有证据，针对新增 G1 行为增量回归，不再全量重跑 G0。
4. **PR-D（最小业务模型）**：飞书项目/TAPD URL/ID 手工绑定、通用契约与证据引用、复制交接；以需求、Bug 和至少两类不同跨仓集成场景验证，UART 仅可作为一个用例。
5. **PR-E（瘦身）**：在替代和数据回执验收后移除不再使用的写入、Planning、重复队列与多余刷新；同步新门禁/双语操作说明。

每个 PR 都要给出输入/输出、目标身份、失败语义和可复现测试，不把“新增文件、自动化通过”当成用户价值。**优先修改现有模块，不为每个小场景新建框架或数据库。**

## 11. 客观可用性与性能验收

**对照组：** 同一开发者/主机/终端/任务集合，对比 A=原生 Codex CLI + VS Code/tmux + 现有 Git 工具，B=codex-tui。建议沿用现有 [用户体验验收](../guides/ux-qualification.md)：30 个代表性任务、独立重复、ABBA/BAAB 交错比较，保存脱敏原始结果。目标是**配对中位完成时间改善至少 25%**；100 次任务定位中正确率至少 99%，但**任何实际错目标审批、写入或破坏性操作都是零容忍阻塞项**。首次使用应在 10 分钟内完成查找、搜索、进入正确会话；以上是拟定合格线，**不是已经量测的业绩**。

补测：至少 50 条真实历史、10k 级模拟元数据、父/子 Agent 混合、断线/重连、跨目标同 ID、跨主机相同 CWD、断尾日志、缺失 Experimental 字段、60×24/80×24 SSH、提醒在静音/错误 BEL 下的视觉表现。分别报告 p50/p95 找回时间、WrongTarget、NeedsYou 漏报/误报、交接错误、首帧/交互延迟、资源与长期运行；没有观测写 **NOT_RUN**，读取失败写 **PARTIAL/UNKNOWN**。

**退出判据：** 如原生 Codex+VS Code 已能同样便捷安全地完成任务，删除重复会话/终端/看板代码；只有跨仓库、跨目标 Attention 与准确证据关联的实测差异化价值足够，才维持额外复杂度。

## 12. 安全、回滚与资源边界

- 写操作在目标身份、Thread/Turn/Request、仓库/工作树、必要 SHA 变化或观察过期时一律重新确认；不确定时 fail closed，不自动重复投递。
- 交接只给目标会话传递批准的最小文字与引用，默认人工复制；源私有工单、设备日志、客户信息和秘密不能进入公开仓库或无权限 Agent。
- 不自动执行硬件烧录、OTA、运动测试，不通过 CI PASS 伪造上电/实车验证。
- 既有未决 Mutation/OutcomeUnknown 回执须可审计、独立备份，完成退出前不得抹掉；任何迁移先备份、可回滚，保持同源真实性。
- 旧版本发布/Tag、受保护主线、不可变发布等治理不因 Lite 范围收敛而被绕过；现场资格仍以实际设备和准确 SHA 为准。
- **成本：** 设计、解析、索引和 UI 不另触发模型调用；不启动 Codex/ChatGPT Work 的共享额度，不添加自动定时巡检或大规模后台同步。GitHub Actions 仅用于正常代码变更 CI，不替代用户试用。

## 13. 本版的完成定义与未完成事项

本版完成的是**统一的下一阶段实施决策**，并把五件事、三条标准流程、身份安全、最小实现、G0–G4、性能对照、风险退出写在一处。它不改变现有程序行为。

| 检查项 | 截至本方案 |
|---|---|
| 会话仓库目录级枚举与 L1.2 证据归档 | 已完成（仅档案中现有 2026/04—11 内容） |
| L1.3 单一方案的产品、架构、工作流和门禁定义 | 本文件为目标交付 |
| 2,168 份档案的读取尝试 / 无损内容资格 / 全账号主机覆盖 | **v3.1 真实重跑已完成：2,170/2,170 Git 对象哈希核验与 2,168/2,168 组读取尝试全部通过。** 内容 `PARTIAL`：2 个空组，32 组含 67 条解析错误（64 JSON 语法、3 UTF-8），79 组重复元数据（不同 ID/缺失 ID 尚未拆分）；无损全部正文与全部主机覆盖均**未证实** |
| Lite 主路径代码重构与旧服务清理 | G1 部分落地（PR #423–#425）；安全 Target 身份、原生接力、独立 Attention 与 G2/G3/G4 待完成 |
| G0 既往真实使用基线 | 维护者确认前期已验证，不再重复阻塞当前 G1；原始现场资格与新版本 Stable 资格仍单独判定 |
| Stable 发布许可 | **未获得**，保持 `stableReady=false` / `publicationAllowed=false` 的既有安全结论，直至准确代码基线重新核验 |

**当前实施决定（2026-11-20）：维护者确认前期 G0 实机基线已验证，不再重复作为 G1 阻塞；G1 的来源覆盖、血缘和主子聚合已在 PR #423–#425 落地主线，Target 安全身份、原生接力和独立 Attention 继续实施；G2 采用真实需求/Bug，G3 采用通用跨仓协作与联合验证，最后有条件进入 G4 瘦身。先前 G0 不自动代表新版本 Stable 资格。除客观收益外，不增加新的研发管理层。**
