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

对 `jiying2007/sessions` 2026/04—11 目录递归枚举：**2,170 个 Git blob 文件对象、200 个有记录日期目录、累计 8,274,174,477 bytes**。分类：普通 `.jsonl` 1,875 个、独立 `.jsonl.zst` 291 个、压缩分卷 4 个（两组），**归并后 2,168 份候选 rollout 档案**；至少一个普通文件为零字节。

**这不是 2,168 个有效且互不重复的 Thread，更不是账号全部对话的证明**：一些会话有父子 Agent、压缩分卷或缺损，另有未上传主机/时间窗口的可能。Git 仓库 size 与 blob 累加量属于不同口径，不混写。

L1.2 已对 112 份非压缩档案抽取 `session_meta`，并对 6 份不同月份的长会话做事件分析。样本确证 VS Code、CLI、`codex_exec`、子 Agent、自动 Guardian、Windows/POSIX 路径、长 Turn 与 compact 等模式确实存在；样本按日期和大小选取，**比例不得推广到全仓**。本版仍未声称对 8.27 GB 进行完整语义审阅。

要获得全量去重、失败类型、子 Agent 树和历史主机归属，使用可信本地环境**一次性离线流式审计**（附带的私有 `codex_sessions_offline_audit.py`），报告解析错误、零字节、压缩/分卷、缺失元数据和不能判断的主机；禁止上传原文到本公开代码仓库。该审计**不应成为 TUI 开机或持续轮询依赖**。

### 2.2 已证实的产品需求

| 会话证据/真实工作 | 对应必须解决的问题 |
|---|---|
| Windows 本机 MCU 工程；Linux/SSH SoC、MCU、算法工程 | 相同路径/仓库名不得跨主机误定位；支持异构路径显示 |
| 同一工程中的 VS Code、CLI 和 Exec 来源 | 来源字段与当前交互入口分离，不以 `originator` 替代实际 Client |
| 父子 Agent、自动 Review/Guardian | 默认汇总到主会话，审批/错误仍可达；禁止把 Worker 当独立业务事项 |
| 历史数千条会话、长会话 compact、多次 Turn | 最近窗口 + 精确搜索 + 结构化进度，不能一次平铺或复制原始日志 |
| SoC↔MCU UART、Boot↔App↔OTA、BSP↔App、Audio↔KWS | 同一事项的 2–4 会话、版本化协议和联合验证证据 |
| 产品与测试使用飞书项目/TAPD | 原平台权威、手动精确链接优先；不把 GitHub/GitLab Issue 当默认业务入口 |
| VS Code/Git 已能操作分支、Commit、PR | TUI 只显示最小只读仓库上下文，不制造重复写入流程 |

### 2.3 官方 Codex 已有能力：适配，不复制

当前上游协议源码提供 `sessionId`、`parentThreadId`、`originator`、`source` 等字段，并有 `thread/list`、`thread/search`、`sourceKinds`、`currentAccountOnly`、`readState` 等能力/实验字段。参考：
- [Thread 数据结构](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/thread_data.rs)
- [Thread 查询协议](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/thread.rs)
- [App Server README](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)

**上游 `main` 能力 ≠ 用户已安装的 Codex 版本有该能力**。协议 adapter 需要在实际 Target 运行时探测、容错、降级；实验字段不能作为通用必需字段。优先复用原生搜索/已读状态/子 Agent 列表，只有证明缺失或不可靠时才做小范围本地派生投影。`currentAccountOnly` 也不等于所有历史记录都可严格按账号隔离，不能代替本地访问控制。

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

**检索规则：** 首页只查当前活动且可达的 Target；`/` 优先利用原生搜索和已有有界索引；“全部已配置目标”必须由用户主动选择、分目标有界查询并展示 Partial；历史档案只在明确切到“历史引用”范围时加入。默认不在启动时扫描 8GB，也不把归档命中当作在线可恢复 Thread。

## 4. 身份、来源和状态：三者必须分离

**SessionKey = 已验证的 TargetIdentity + canonical ThreadId**。单独的线程 ID、窗口标题、仓库 URL、CWD、机器别名或 SSH 隧道名都不够。TargetIdentity 包含用户配置目标、可信连接端身份/作用域和可观察的能力信息；未能验证时拒绝高风险操作。连接别名用于展示，不是主机真实性证明。

**GroupKey** 优先取同 Target 下的 `sessionId`，其次用经确认的 `parentThreadId` 父子关系；字段缺失则不猜测。区分 `forkedFromId`（分叉沿革）与 `parentThreadId`（子 Agent 所属）；跨 Target 相同 `sessionId` 也不可盲合。子 Agent 仅是导航分组，不产生新任务权威。

**RepoRef** 分开记录 Git 仓库身份、工作树根、原始 CWD、平台路径类型、可达性和观察时间；非 Git 项目会话合法。跨主机同名目录绝不能映射为同一写入地点。

**来源字段：** `source` 表示来源种类，`originator` 可能是创建时记录的发起客户端，`threadSource`/角色用于更细分的自动子线程/Guard；这些字段都不等于“当前正在由谁操作”。不认识的枚举保留为 Unknown，并显示原值的安全摘要，而非丢弃记录或把 JSON 原样堆在列表。

| 独立状态轴 | 可用状态 | 禁止推断 |
|---|---|---|
| 连接/可信度 | LiveConnected / Disconnected / ArchiveOnly / Partial / Unknown | 文件存在不代表仍可 Resume；断线不等于 Idle |
| 会话/Turn | Working / NeedsYou / Ready / Idle / Error / Unknown | Turn complete 或 Idle 不等于功能完成 |
| 业务事项 | Open / InProgress / Closed / Unknown（以原平台为准） | PR 已合并不等于需求/缺陷关闭 |
| 验证 | NotRun / Failed / Partial / Passed / Accepted / Unknown | CI PASS、模拟 PASS 不等于真实设备或人工验收 |

当前官方 ReadState 可用且权威时，优先采用；不可用时本地“已看过”仅是 UI 偏好，不冒充云端/其他客户端同步已读。

## 5. 实时发现、原生接力、归档检索：三条链路

**Discover（实时）：** 复用既有 `app_server_target.rs` 的 stdio/WebSocket/Unix Socket 目标配置，优先一个活动目标。多目标按需串行/有界并发取只读摘要，结果保留 target 标签、抓取时间和完整性；某目标失败不得拖垮本地首页，也不能暗示该目标无待办。不增加常驻团队服务或后台轮询。若要远端安全连接，沿用现有 TLS/Loopback SSH Tunnel 和认证边界。

**Resume/Locate（接力）：** 选择 Thread 后重查目标可达性、Thread 身份、当前工作目录与原生接管能力；优先返回原生 Codex CLI 的准确会话或可复制的恢复/定位指令。不能保证 VS Code ↔ CLI 直接接管时，显示“仅定位/需在原客户端继续”，不能启动另一条同名会话冒充成功。若保留内嵌最小回复/审批/中断，则只限真实验证不可靠的接力场景，沿用既有审计与请求身份安全检查。

**Archive（历史）：** 一次性离线读取用户明确提供的私有档案，生成有界脱敏元数据，默认不保存正文、完整 Prompt、凭据或私有设备日志。查询结果明确为“ArchiveOnly，记录来源/完整性/最后观察时间”，不自动更新 Live 的会话状态。零字节、残缺、分卷或缺少终止事件均标不完整；无正式身份验证不得把跨主机同 Thread ID 的档案合并。

**LastProgress（长会话）：** 从结构化事件提取最近 Turn 的开始、结束、取消、最后活动、待请求和最近可靠证据引用，不发起额外模型调用。`task_complete` 只是 Turn 已结束；缺尾事件是 Unknown，不自动标记 failed/success。允许手动查看原始来源（须有权限），首页不显示工具调用流水账。

## 6. Attention：先保正确，再谈聪明

待处理来源按**真实审批/输入请求、明确系统/测试阻塞、用户待核对结果**优先；工作中与完成后无待操作的后台 Worker 不刷屏。子 Agent 需要用户确认时，父组必须展示聚合提醒，同时能一键定位**真实子 Thread/Request**；不能在父 Thread 上错误回复，也不能因为折叠吞掉它。

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
| `Link` | 一条事项对应 1–4 条 SessionRef、职责/角色；不复制外部任务状态权威 |
| `ContractRef` | repo + path + 完整 Git SHA + 协议版本/接口约束；旧 SHA 被修改后标“待核对” |
| `EvidenceRef / Handoff` | 提交/测试/设备版本/时间/结论来源；交接摘要、目标、显式确认及结果 Unknown 回执 |

**MVP 事项接入：** 用户从实际飞书项目/TAPD 复制原始工作项 URL/ID，人工选择平台与类型，准确关联已有会话；不要求管理员 API 凭证，不根据标题猜工单 ID，也不从普通飞书 Wiki URL 假装解析出飞书项目工单。若双方实际使用两平台，分别试用手动引用；API 只读 adapter 在真实节省时间、获管理员授权且接口字段得到确认后再加。平台更新、评论和状态流转在原系统完成。

**隐私：** 私有工单内容、客户信息、日志、会话原文不导入公开仓库，也不能因为开发者可看网页就自动允许所有 Agent 查看。用户显式选择脱敏摘要和可分享范围。

## 8. 三个完整交付路径 + 复用场景

### S1 单仓需求（P0）

飞书项目/TAPD 原需求 → 复制真实 URL/ID → 关联已验证的仓库/Target/主 Thread → 原生 Codex 开发 → Git/PR/CI 证据 → 验收条目对应检查结果 → 产品/测试在原平台确认。纯技术讨论、周报、知识查询无需强制创建工单。

### S2 单仓 Bug（P0）

原缺陷及复现环境、固件/软件版本、实际/预期 → 定位修复会话 → 可复现则记录测试，偶发则明确待复现 → 修复 SHA + 原场景和相邻场景回归 → 测试复测 → 原平台关闭。CI PASS 不等于设备 Bug 已消失；失败重启应关联原缺陷和旧证据，而不是创建第二套 Bug。

### S3 双仓 UART 联调（P1 的最小验收样本）

一个真实外部事项 → 关联 SoC 应用主会话与 MCU 主会话 → 选定唯一**协议契约 repo/path/full SHA**（帧格式、命令、字节序、校验、时序、错误/重试）→ 双方各自实现与单测 → 生成**可复制、有人确认的定向交接摘要** → 共同测试向量 → 模拟联调 → 真实 UART 收发/回归记录 → 双端完整固件/代码版本和人工结论。

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
| **G1 会话定位/Attention** | 主/子分组、Target、最近 20+置顶+待处理、搜索、最近进展、未知状态、80×24 导航 | 用实际会话和合成大规模元数据验证无错目标、无请求漏显、无焦点丢失；不同版本/来源覆盖 | 不开 G2 扩展，不把模糊状态当成功 |
| **G2 真实需求/Bug** | 手工精确关联飞书项目/TAPD 原事项、提交和测试证据 | 真实产品需求 1 项、测试缺陷 1 项；若两平台均在使用则各测入口，验收由原责任人完成 | 保持手动入口，不提前做平台 API |
| **G3 跨仓联调** | SoC+MCU 两个独立会话、契约 SHA、人工交接、联合测试引用 | 双端版本 + 模拟/真实 UART 收发 + 人工确认；再以 KWS/OTA 抽样复用 | 不允许双方单测 PASS 冒充联合 PASS |
| **G4 删除冗余/发布判断** | 废弃写入、Planning、重复 Queue/Board 与轮询；完善回执、迁移、Doc、Gate | 精确源码/测试结果、数据备份恢复、真实体验优于原生 CLI+VS Code；另核 Stable 资格 | 若效益不成立，只保留最薄会话定位或停止扩张 |

**顺序不是“所有功能都实现再试用”**：G0 必须第一；G1 完成后才允许对核心结构做破坏性精简；G2/G3 可先由外部工具完成人工流程验证，再逐步最小化产品实现。不能用 mock、CI 或文档充当真实设备与用户验收。

### 10.1 代码实施 PR 建议

0. **G0 先行（不改业务代码）**：用当前可信候选包跑首次真实 Windows SSH/Ubuntu 与 Codex 的基本链路，保留原始失败证据；未通过不能宣称 Lite 首次可用，也不能先做破坏性删除。
1. **PR-A（无破坏）**：适配可选 Thread 字段 + 来源归一化 + fixture 测试；保持原 UI 和运行功能可用。
2. **PR-B（核心替代）**：SessionKey + Parent Group + Attention 原生投影；混合来源、父子请求、断链/重连与错 Target 回归。
3. **PR-C（交互）**：唯一首页和轻详情、原生接力、60/80 宽 SSH、状态/最近进展；在新代码上重跑 G0，正式开展 G1 用户验收。
4. **PR-D（最小业务模型）**：飞书项目/TAPD URL/ID 手工绑定、证据/契约与复制交接；在真实需求、Bug、UART 下验证。
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
| 2,168 份档案逐条内容级统计与多主机全覆盖证明 | **未完成**；仅本地一次性审计可补充，不能凭现有数字推断 |
| Lite 主路径代码重构与旧服务清理 | **未完成** |
| 真实 Windows SSH、Codex 登录与设备/内部 GitLab 验收 | **未完成** |
| Stable 发布许可 | **未获得**，保持 `stableReady=false` / `publicationAllowed=false` 的既有安全结论，直至准确代码基线重新核验 |

**最终决策：先 G0，再 G1，然后以真实需求、Bug、双仓 UART 证实 G2/G3；最后进行 G4 瘦身。除可审计的实证收益外，不再向这个个人/小团队工具增加新的管理层。**
