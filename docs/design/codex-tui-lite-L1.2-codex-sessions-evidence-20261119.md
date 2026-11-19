# codex-tui Lite L1.2：账号级 Codex 会话证据补充与终态收敛

**日期：** 2026-11-19  
**性质：** 设计证据与实施决策补充；不是功能完成、现场通过或 Stable 发布声明  
**代码审查基线：** jiying2007/codex-tui main 19ab7a99a7ec0c399c82625fa696340342ce403e  
**上位产品方案：** [L1.1 飞书项目/TAPD 实际入口终版](codex-tui-lite-final-plan-L1.1-feishu-tapd-20261119.md)  
**适用：** 个人与 1–5 人研发小团队，Windows / Windows Terminal / SSH / Linux / VS Code / 原生 Codex CLI。

> 本补充仅修订 L1.1 的跨主机历史发现、会话生命周期、父子 Agent、数据身份及落地验收。L1.1 的业务权威边界、Git/Forge 写入退出策略、审批与 OutcomeUnknown 安全要求保持不变。不要把“方案已归档”写成“产品已部署”。

## 1. 证据范围与强制区分的统计口径

本次对维护者提供的私有 Codex rollout 归档，在 2026-11-19 的 Git 提交快照上完整递归枚举了 2026/04—2026/11 的 Git Tree。结果：**2,170 个文件对象、200 个有记录的日期目录、累计 blob size 8,274,174,477 bytes**。GitHub 仓库元数据的 size 为另一种口径，不能与累计 blob size 混用。

| 2026 月份 | Git blob 个数 | 普通 .jsonl | 单文件 .jsonl.zst | 分卷 part |
|---|---:|---:|---:|---:|
| 04 | 63 | 63 | 0 | 0 |
| 05 | 253 | 238 | 15 | 0 |
| 06 | 569 | 560 | 9 | 0 |
| 07 | 305 | 302 | 3 | 0 |
| 08 | 188 | 180 | 8 | 0 |
| 09 | 351 | 115 | 236 | 0 |
| 10 | 233 | 216 | 13 | 4 |
| 11（至 19 日） | 208 | 201 | 7 | 0 |
| **总计** | **2170** | **1875** | **291** | **4** |

10 月的 4 个分卷对象恰好构成 **2 份**压缩 rollout，因此归一化是 **2,168 份候选 rollout 档案**；其中至少 1 份普通 JSONL 为零字节。此数不是独立有效 Thread 数，不是用户任务数，也不保证等于账号下所有从未上传的会话。

本次按月份/日期执行两轮有界抽样，读取 112 份非压缩、大小不超过 8 MB 的 rollout 的 session_meta。样本中 source 分类为 VS Code 58、CLI 37、子 Agent 17；CWD 路径类型为 POSIX 98、Windows 12、未能分类 2。**这组分布有日期和文件大小选择偏差，仅能证明多种来源确实存在，不可外推为全仓概率。** CWD 的操作系统形态也不是主机身份；originator 不等于用户实际看到的 UI。

另外完整解析了 6 份不同月份的事件样本：
- 04-17 嵌入式 Boot：14 task_started、11 task_complete、3 turn_aborted、8 compacted；
- 07-11 长时研发：14 task_complete、3 compacted；
- 09-30 Linux 工程：7 task_complete、79 次工具调用；
- 10-28 Linux 工程：3 task_complete、44 次工具调用；
- 11-18 Linux 工程：3 task_complete、100 次工具调用；
- 11-19 Linux 工程：6 task_started、5 task_complete、145 次工具调用。

这些仅是日志事件观测：不能由 task_complete 推断需求完成；不能由末尾没有 task_complete 推断任务失败。未解压全量 .zst 和分卷、未语义读取所有大文件，故本次不是“2,168 条会话全部逐条阅读”的证明。若要做全量去重、主机归属和错误分布，必须在有权访问私有档案的本地可信环境完成**一次性流式离线统计**，包含分卷重组、缺损计数和结果脱敏；不为此创建定时巡检或使用 Codex/ChatGPT Work 额度。

## 2. 产品终态仍只保留五项责任

**一条主路径：找工作 → 找准确会话 → 回到原生 Codex → 检查阻塞与证据 → 回到原业务事项。**

1. 精准发现：跨主机/跨仓库查找仍可访问的真实 Codex Thread，离线历史仅供定位。
2. 人工关注：仅将可操作审批、输入请求、明确错误、待核对最终结果置前。
3. 业务事项关联：需求/缺陷以飞书项目/TAPD 为权威；GitHub/GitLab 为代码与 CI 权威。
4. 有界跨会话交接：同一事项可关联 2–4 条会话，SoC/MCU 或算法/应用共享准确契约与版本。
5. 证据闭环：清楚区分 Agent 事件、代码变更、CI、真实设备验证和业务验收。

**不做：** 第二个 Codex 聊天客户端、Agent/Goal 编排、独立任务队列、自动 Git/Forge 写入、第二套需求平台、团队中心服务、持续巡检、跨主机 shell 自动化、自动下发 OTA/烧录/运动测试。旧模块的存在不是保留理由。

## 3. 会话身份与来源模型（对 L1.1 的关键补强）

**SessionKey = target_identity + canonical Thread ID**，而非 Thread ID、CWD、仓库名称或窗口标题单独作为键。

- target_identity 要由用户配置的目标与已验证的连接端身份构成，存储稳定标识和必要的当前能力；SSH tunnel/代理名称仅是展示标签，不能自动推断远端主机相等。
- RepoRef 分开记录 Git repository identity、worktree root、原始 cwd、主机可见性和 observed_at。相同 CWD 字面值不能授权访问另一主机文件；没有 Git 仓库的会话也合法。
- ThreadRef 中保留 parent/root ID、subagent 深度、source（vscode/cli/subagent）、起始时间和原生恢复能力；子 Agent 在首页聚合到父线程，缺失父线程时标“孤立子会话”，绝不伪造父任务成功。
- Provenance 标识 **LIVE_CONNECTED / LIVE_DISCONNECTED / ARCHIVE_ONLY / UNKNOWN**。导入私有归档后，不得自动升级为 LIVE，也不得因为找到文件就启用 resume。
- 窄 UI 一行显示：状态/理由、项目或仓库、来源主机/目标、会话标题、最近活动；完整 CWD、契约与证据放在详情。

只要有身份歧义、不可访问目标、CWD 未核实或能力未验证，任何回复/审批/中断/恢复必须先拒绝并提示如何核对。保留现有对 remote target 的 ForeignUnix 防护。

## 4. 发现、恢复与历史索引是三个不同能力

**发现 Discover：** 从当前有权访问的 App Server 读取有界元数据；每个配置目标分别维护 connected/partial/unknown，必要时手动刷新。允许有限数量目标**按需**读取，并在 UI 合并排序；不要对所有主机常驻 daemon 或高频轮询。

**恢复 Resume：** 仅当目标当前可达、Thread 仍由该目标原生拥有、能力已检测通过、身份复核成功时打开原 Thread。原生 Codex CLI 作为优先执行入口；若 CLI 无法安全接管该 Thread，则条件保留极小的现有回复/审批/中断闭环，而不是假装接管成功。

**归档 History：** 对上传的 rollout 只做可选一次性元数据盘点和精确历史定位。数据不自动同步到用户各台机器，不在 TUI 启动时扫描 GB 级原始 JSONL。历史结果明确标“只能查看引用/尝试在原主机定位”，不声称可 resume。默认不持久化 transcript 正文；需要全文搜索时使用原生能力或用户明确启用的受限私有索引，并标注索引完整性。

**去重：** 先按经过验证的 target + ThreadId 去重实时会话；归档的同 ID、多次上传或分卷，只能在源环境进一步核实，不能跨主机盲合并。一个仅有 session_meta、零字节或缺失结束事件的 rollout 不能自动归入“完成”。

## 5. 长会话最近进展快照（不产生额外模型调用）

通过 Codex 结构化事件派生 LastProgress：
- 最近 Turn 的 started/completed/aborted/unknown 与最后观察时间；
- 最近一次明确的审批、输入请求、系统错误或 usage/budget 警报；
- 关联到对应来源目标、线程、可核验的代码/测试证据；
- 最后观察状态的完整性与新鲜度。

task_complete 表示 Codex Turn 结束，不表示业务已完成；idle 表示当前无执行，不表示 PASS；没有最终事件表示**未知**而非 failed。快捷显示“最近结果/待核对原因/下一步定位”，不要把数百次工具输出复制到 Mission Control，也不要让 AI 根据任意正文自动做权威状态判定。

## 6. 多仓库与业务闭环

- **单仓需求：** 从飞书项目/TAPD 复制准确原始 URL/ID → 关联 1 条主会话 → 原生 CLI 开发 → Git/CI 证据 → 原工单由有权限者验收。
- **单仓 Bug：** 原缺陷 → 复现/环境/版本 → 修复会话 → 回归证据 → 原平台复测，偶现允许 UNKNOWN。
- **SoC ↔ MCU UART 联调：** 一条外部事项关联 SoC、MCU 2 条主会话 → 共同契约 repo/path/SHA → 双端实现 → 定向人工确认的摘要/向量/接口变更交接 → 模拟验证 → 真实 UART 联合测试。不能以双方单测 PASS 冒充联调 PASS。
- **Linux BSP ↔ 应用、Boot/OTA、运动控制 MCU、Audio ↔ KWS ↔ 部署：** 复用相同 WorkItemRef + SessionRef + ContractRef + EvidenceRef；禁止另建专属任务编排系统。

交接 v0 默认“生成可复制摘要、用户粘贴”；若以后启用 TUI 一次性提交，需强制校验完整目标身份、确认、回执和 OutcomeUnknown 不重发。不能向正在工作的 Turn 静默插话。

## 7. 现有代码审查所见的实际改造点

本次针对 main 基线复核：
- src/app_server_target.rs 已有 named targets、active 单目标、stdio/websocket/unix-socket 解析与远程传输安全校验。**当前有目标配置，不等于已有统一多目标在线 Registry**；以单一 active target 的简化交互先完成 G0。
- src/app_server_registry.rs 的 by_id 目前按 thread.id.0 键控；如果将来合并多个 target 的结果，必须先完成组合身份与 target-local 事件隔离，避免碰撞或错目标。
- src/app/registry.rs 的 NeedsYou 仍部分依赖 Planning/WorkCard 派生投影；必须先建立原生 thread/request 的替代投影并回归无漏报，再摘除 Planning。
- src/app/target_locality.rs 已对远程目标的“路径同名冒充本地路径”执行拒绝；Lite 重构必须保留。
- 当前 transcript 搜索持久化为 opt-in、有界、可能不完整。不要把它冒充全账号归档权威。
- Git/Forge/Worktree 写操作、Queue、Board、Goal、Launch Presets 和多余轮询按 L1.1 G4 分阶段退役；撤销前核验旧未决回执，不删除未知结果。

## 8. 最小交互与实施关口（继承 L1.1 的 G0–G4）

**唯一首页：** Needs You → Pin → Working → 最近 20 条；更多历史通过 / 搜索；选中保持稳定，终端抽屉不能遮挡列表。使用 ↑↓/j/k、Enter、/、Esc、?；Alt+T 为可选终端焦点切换，必须保留终端兼容替代操作。**不使用 F6**，也不引入大量新快捷键。

| Gate | 本次新增的最低判据 |
|---|---|
| **G0 真实首次可用** | Windows Terminal → SSH Ubuntu，真实 Codex，至少两个不同仓库及准确 cwd；正确返回原生 CLI，无误目标，无“已发布”误报 |
| **G1 会话与 Attention** | 实测大量历史下最近 20+置顶+待处理可达；父子 Agent 不泛滥；多目标未验证时绝不混合；最近进展与 UNKNOWN 准确 |
| **G2 需求/Bug** | 各走通 1 个来自飞书项目/TAPD 的真实需求和缺陷；原平台状态、代码、CI 与验收独立 |
| **G3 UART 联调** | 两仓独立会话 + 契约 SHA + 人工交接 + 双端版本 + 真实收发证据；随后抽样 Audio/KWS 长会话复用 |
| **G4 瘦身与发布** | 无用写入/队列/规划、后台轮询和对应状态完全退出；安全回执无丢失；现场可用性优于原生 CLI+VS Code 的组合，才考虑保留扩展；Stable 资格另行核验 |

真实体验至少记录：找回指定会话正确率、跨主机定位耗时、从历史回到可执行 Thread 的耗时、Needs You 漏报/误报、错目标次数、交接错误次数、窄 SSH 终端操作失败、一次冷启动与长会话操作性能。用原生 CLI+VS Code 的相同任务作对照；没有实证效率提升的便利功能不保留。禁止为“完成 Gate”创造伪造 CI、设备或用户通过。

## 9. 数据安全与落地说明

私有 rollout 可能包含聊天、源码片段、内部地址和凭据信息；不向此公开仓库复制原文、绝对路径、内网 Git remote、完整 prompt、密钥或推理内容。只保留本补充的聚合数字、结论与操作规则。未来离线全量审计应在有权限的本机流式处理普通 JSONL、zstd 与分卷；显式报告读取失败、零字节、损坏、重复、覆盖范围及不可推断的主机身份；输出默认只含脱敏聚合。

本轮**只归档设计补充，不修改 TUI 运行时，不授权 Stable 发布，不触发任何 Codex/ChatGPT Work 模型用量**。L1.1 的 G0 现场资格与 G4 安全迁移要求继续有效。
