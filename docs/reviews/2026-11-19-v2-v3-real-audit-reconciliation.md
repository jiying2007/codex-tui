# Codex 会话 v2/v3 真实审计对账与 L1.3 优化结论

**日期：** 2026-11-19
**证据输入：** 维护者在授权本地克隆上实际运行后提供的脱敏 `codex-rollout-audit/v2` 与 `codex-sessions-independent-audit/v3` JSON；私有仓库固定 `sessions@98cc404fd6f3c621bfe022d67c3efe82d01562b5` 的 Git Tree；当前 `codex-tui` 源码。
**性质：** 新真实审计结果的分析与改进依据，不是 Stable 发布或功能部署证明。公开文件只写聚合数字、代码定位与诊断结论，不导出原始 ThreadId、用户消息、文件名、内部路径或凭据。

## 1. 关键证据与本轮更正

| 指标 | v2 实际报告 | v3 实际报告 | 结论 |
|---|---:|---:|---|
| 仓库文件对象 | 2,170 | 2,169 | v3 漏识别一件有特殊文件名的已跟踪对象 |
| 归并候选 rollout 组 | 2,168 | 2,167 | v3 少一组；这不是证明本地克隆丢文件 |
| 文件字节 | 8,274,174,477 | 8,260,293,970 | 精确差 13,880,507 |
| 非空读取/解码组 | 2,166 | 2,165 | 差一份 |
| 已知零字节组 | 2 | 2 | 原始内容缺失无法从审计恢复 |
| 部分 JSON 异常组 | 32 | 32 | 需定性，不可视为全部内容完整 |
| 无法解析 JSON 行 | 67 | 67 | 原因未知，保留原件和失败证据 |
| 已解析的记录 | 3,041,293 | 3,037,573 | 差 3,720，符合漏识别单个档案 |
| 独立记录的 ThreadId（无主机证明） | 2,165 | 2,165 | 不等于已去重的全账号用户工作 |
| 明确父子边 | 747 | 747 | 父边未在导出内丢失，但不能代表实时权限 |
| 重复 ThreadId 观测 | 1 | 0 | 提示漏识别档案可能涉及已出现的 ID；不能仅凭汇总认定 |
| 重复 `session_meta` 的组 | v2 可见 `2245 - 2166 = 79` 条附加元数据 | 79 | 不能直接推断数据损坏 |
| 已确认的 Git Blob 哈希数 | v2 无逐对象哈希门禁 | 2,169 | v3 仅对纳入清单的对象进行核验 |

**漏识别根因已核对：** 2026-09-04 的 Git Tree 存在一件长度 **13,880,507 字节**的 `rollout` 文件，其名字由两个完整 UUID 用下划线连接；v3 正则只接受单个 UUID，导致 `unsupported_tracked_objects=1`、`missing_tracked_objects=1`。同一漏检造成 v3 `all_known_groups_attempted=true` 表意错误：它实际只尝试了自己发现的 2,167 组，而不是固定提交中的全部 2,168 组。

**重要纠正：** L1.3 早先“全量原始内容尚未执行”只适用于收到真实用户报告之前。现在 v2 已提供“全部 2,168 个已知候选组均尝试”的本地执行证据；但 **2 个空组、32 个 JSON 部分损坏组、67 条非法行**意味着“全部正文完整无损”仍为 false。无论 v2/v3 都**不证明所有主机的所有会话已上传**。

## 2. 真实开发记录支持的产品优先级

v2 中可读的 2,166 个非空档案按 `session_meta.source` 分类：明确子 Agent 747、其他子 Agent 12、VS Code 669、CLI 481、Exec 230、Unknown 27。子 Agent 共 759（约 35%），不能将线程平铺在 Mission Control，也不能隐藏其待审批请求。

`originator=codex-tui` 1,615 次，只是历史创建来源标签，**不能推出当前 TUI 正在运行或控制这些会话**。`task_complete` 17,798 次只是 Codex Turn 结束，`token_count` 400,139 次是事件次数，均非实际业务验收、支付 Token 合计或账户花费。

**现有源码须区分已有与缺失：**
- `src/app_server.rs:1795-1835` 已支持 `sortKey=recency_at` 并有旧协议降级；不重复实现。
- 同一 `thread/list` 请求目前未显式传 `sourceKinds`，存在遗漏 Exec/Sub Agent/Review 来源的覆盖风险（不能由档案推断实际 TUI 漏报率）。
- `src/app_server_registry.rs` 以 ThreadId 为键，只能在当前单 Target 意义下有效；任何多目标融合需要完整目标身份。
- `src/app/registry.rs` 的 Attention 仍引用 Planning/WorkCard；先建立原生安全请求投影、验收无漏报，再退役旧投影。
- `src/app_server_target.rs` 已有 named Target/TLS/隧道校验，继续复用而非新建服务。

官方资料：[ThreadListParams](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/schema/typescript/v2/ThreadListParams.ts)、[Codex App Server](https://learn.chatgpt.com/docs/app-server)。`thread/list` 省略或传空 `sourceKinds` 时默认为交互来源；`archived` 默认只返回未归档；提供空 `modelProviders` 显式包含所有 Provider。`sessionId` 中 Fork 可能共享会话树标识，不能直接折叠独立 Fork；父子 Agent 关系以明确 `parentThreadId` 为准。

## 3. SDD 独立审计工具 v3.1 的结果与边界

**实现：** 补充单/双 UUID 文件名白名单、严格检查文件名与记录中任一 UUID 的关系；区分 `all_discovered_groups_attempted` 和 `all_known_groups_attempted`；将重复 `session_meta` 分为同 ID / 冲突 ID，将解析错误分类；可通过显式 `--private-triage-out` 输出 0600 权限的本机异常文件相对路径、错误类别与前 12 个行号，不能上传公开仓库或分享给无权限的人。

**验证：** 33/33 个合成用例通过，ZIP 新目录解压后也重新执行 33/33；Python 3.8 语法解析通过，运行环境 Python 3.13。v3.1 正则重新核对已知 Git Tree 2,170/2,170 个文件名，其中一件双 UUID 变体。

**尚未完成：** 对真实授权私有 8.27 GB 克隆重新运行 v3.1，因而**不能预报**全部 2,170 哈希必过；67 条 JSON 错误与 79 组重复元数据是否可以安全归类也需要本机诊断。不可为了 `PASS` 修改历史 Git Blob 或忽略不确定性。

## 4. L1.3 终态方案修正（不加新管理平台）

| P0 功能 | 改造与验收重点 | 不应做的事 |
|---|---|---|
| 来源完整性 | `thread/list` 显式枚举目标版本支持的 CLI、VSCode、Exec、SubAgent/Review；根据 `archived` 和 Provider 范围按需查询、分页到完成。缺能力、断联、未加载的结果标 Partial | 把省略来源过滤的“没有结果”当成“没有待办” |
| 身份与分组 | Target + 可靠 `CODEX_HOME`/Provider/访问作用域 + ThreadId；父子显式树；Fork 保持独立 | 同名 CWD、相同 `sessionId` 或机器别名跨 Target 合并 |
| Attention | 原生 Approval/Input/Error/Turn 证据优先，子会话待办向父组投影，点击最终路由实际子 Thread/Request | 先删 Planning 再补提醒、自动代用户审批 |
| 定位与恢复 | 能力检测，优先原生 `thread/resume` 与有界历史；`modelProviderMismatch` 拒绝伪装可继续 | 在另一个 Provider 直接 Fork 假装接续成功 |
| 工作与交接 | 飞书项目/TAPD 原工单、Git SHA、适用时 gitlink 和真实测试引用；复制交接优先 | 自造 Git/Forge 写入、业务工单平台、Agent 消息总线 |

**保留路线 G0 → G1 → G2 → G3 → G4。** G0 可以使用已有会话观察和不调用模型的 App Server 验证；未经授权不触发新 Codex/Work 模型 Turn。独立历史审计是研发证据工具，不能成为 TUI 开机扫描任务。需求、Bug、跨仓 UART 的实际验收分别回到飞书项目/TAPD、Git/CI 与真实设备。

## 5. 后续必须独立取得的实测

- v3.1 应验证全部 2,170 Blob、2,168 候选、8,274,174,477 字节；失败按文件名识别、Git Hash、解码、JSON、元数据一致性独立报告。
- 私有 triage 核实 32 个问题组的 67 条非法 JSON 是 UTF-8、语法或尾部片段问题；核实 79 个重复 `session_meta` 是否同 ID/不同 ID，不能自动降低门禁。
- 不论内容分类如何，两个零字节组必须保持“内容不可用”；审计器的 `PARTIAL` 可能是完全正确的诚实结论。
- G1 用子 Agent、Exec、不同 Provider/`CODEX_HOME`、跨目标同 ThreadId、断链/重连、窄 SSH 终端、正确 Request 路由做反例测试。
- 以原生 Codex CLI + VS Code 的相同实际工作为基线量测有无时间与错误率收益，收益不成立时继续删减重复功能。

**结论：本轮应把“真实全量归档已尝试、内容仍部分缺损”替换此前的“全量尚未运行”；L1.3 的方向保持不变。P0 仅修复来源、身份、Attention 和准确接力，勿发散成重型管理系统。**
