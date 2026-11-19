# Codex 会话档案清点及 codex-tui L1.3 P0 复核

**日期：** 2026-11-19  
**档案固定基线：** `jiying2007/sessions@98cc404fd6f3c621bfe022d67c3efe82d01562b5`  
**TUI 方案原基线：** `jiying2007/codex-tui@1f9e50a8fd015a2de52e9b5c569dce63cc7b22f0`（PR #419）  
**定位：** 有界验证和 L1.3 方案的 P0 纠偏证据；不是软件重构完成、Stable 放行或全量原始对话正文已经读完的声明。

## 1. 全量 Git 文件对象审计

通过 GitHub 已授权仓库读取，对 2026/04—11 的 8 个 Git Tree 逐月递归枚举，均未截断；共 **200 个日期目录、2,170 个文件对象、8,274,174,477 个 Blob 字节**。

| 月份 | 文件对象 | 普通 JSONL | ZST | 分卷 | 非零且小于 8MB 的普通 JSONL |
|---|---:|---:|---:|---:|---:|
| 04 | 63 | 63 | 0 | 0 | 49 |
| 05 | 253 | 238 | 15 | 0 | 220 |
| 06 | 569 | 560 | 9 | 0 | 548 |
| 07 | 305 | 302 | 3 | 0 | 266 |
| 08 | 188 | 180 | 8 | 0 | 163 |
| 09 | 351 | 115 | 236 | 0 | 95 |
| 10 | 233 | 216 | 13 | 4 | 150 |
| 11（至19日） | 208 | 201 | 7 | 0 | 153 |
| **合计** | **2,170** | **1,875** | **291** | **4** | **1,644** |

四个 `.zst.part-*` 对应两份完整编号的分卷候选，因此**2,168 份候选 rollout**。普通 JSONL 中有**两个零字节文件**（8月18日、9月2日）；唯一一组相同 Blob SHA 就是这两个空文件，不是重复有效 Thread 的证据。按文件名 UUID 未发现两组合法分卷之外的重复；日期目录与文件名日期零不一致。未从 Git Tree 发现更多同 Blob 内容的非零文件。

**规模风险：** 249 个文件单体 ≥8 MB，合计约 5.975 GB（72.2%）；其中 29 个 ≥50 MB，合计约 1.995 GB。仅凭目录/Blob 长度、文件名和 SHA 不能证明全部正文已解析，也不能对每个 Blob 重新验算本地字节 SHA。

## 2. 已执行的内容审计与不可推断项

此前 L1.2 从 112 份 JSONL 抽取 `session_meta`、完整分析 6 份长会话。本轮又完整解析 8 份按月份选择的不同 JSONL 文件，均无 JSON 行解析错误；这 8 份出现 `task_started` 15 次、`task_complete` 14 次、`turn_aborted` 1 次、`token_count` 103 次、白名单函数调用约 95 次。**这些仅为该八份样本的事件统计，不是全仓失败率、token 总数或完成任务数。**

样本与可检索的账号历史 65 条**事件/主题**共同确认 Windows VS Code、Linux/SSH、CLI/Exec、子 Agent/Guardian、不同 Codex 版本、多个 `CODEX_HOME`、Provider/relay 和原生环境错误的存在；65 条事件绝不能当成 65 个独立 Thread。不同源的 `originator` 和 `source` 不等价，真实账户登录状态也不等于有能力运行模型。

**未完成：** 2,168 份档案的逐份解压、事件完整性和去重，缺失主机及未上传会话的全覆盖证明，实际模型 Token 账单，真实运行中的失败分布。此环境没有私有 8.27 GB 原始工作树，不具备可靠地读取全部大文件及压缩分卷原始字节的通路；且不使用私有仓库 Actions/收费 Runner 代替此操作。不能宣称全量对话正文审计 PASS。

## 3. 与官方 Codex 协议对照发现的 P0 风险

1. **`sessionId` 不应成为主子折叠的直接 GroupKey。** 根线程与 forked Thread 可能共享会话树 `sessionId`，但 fork 是独立工作，子 Agent 折叠需可核实的 `parentThreadId`/直接附属关系。`forkedFromId` 只表示分叉沿革。
2. **`thread/list` 默认不是所有会话。** 不带 `sourceKinds` 时偏向 CLI/VS Code 交互来源；`archived=false` 不列归档；部分版本默认只列当前 Provider。必须按目标能力列出实际覆盖范围、逐页取到游标结束。Exec/Sub Agent/Review/Guardian 不可仅凭默认页显示“零待办”。
3. **会话操作的身份必须可证。** Target、`CODEX_HOME` 的存储/用户作用域、Provider/账号权限、ThreadId 共同决定定位和写入许可。别名、CWD 字面值、hostname 或同名 Git repo 不足以跨主机重用身份；本地存脱敏稳定指纹，不存凭据。现有 `status_from_initialize` 已解析 `codexHome`，可以复用，但该字符串自身不能充当主机认证。
4. **继续 Thread 不等于能读取历史。** 支持时用 `thread/resume` 的 `excludeTurns: true` 与 turns/items 分页；遇到 `modelProviderMismatch` 需清楚地区分原 Provider 可续、当前只可读、未知，禁止静默 Fork。
5. **Attention 必须与完整性分开。** 源列表 Partial、子 Agent 未枚举、远程断联、队列未读完都不等于没有人工请求。待回复关联正确 Target+Thread+Request；先替换目前的 Planning/WorkCard 派生依赖，再退役旧模块。

官方参考：[Codex App Server](https://learn.chatgpt.com/docs/app-server)；[Thread 协议源码](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/thread.rs)。

## 4. 其他应保留的实际使用约束

- 列表为 NeedsYou→Pinned→Working→最近20条；当前 `src/app_server.rs` 已优先请求 `sortKey=recency_at` 并提供旧协议分页降级，**保留且验证**这一实现，不重复建设排序；子 Agent 默认折叠但请求不能丢失。
- Windows Terminal→SSH 的终端恢复与滚动、`Alt+T` 替代按键要在真实 60×24 / 80×24 验证；**不用 F6**，不能认为 `Ctrl+]` 必然穿透。
- Feishu Project/TAPD 仍是产品与 QA 的实际需求/缺陷权威；GitHub/GitLab 管理代码、MR/PR、CI；不回退到把 GitHub Issue 强制设为需求主入口。
- SoC↔MCU UART、Boot/OTA、Audio/KWS 等联调的契约应记录文件 SHA、两端提交及适用时 `submodule gitlink SHA`；同名分支并不证明协议一致。
- 单事项界面默认突出显示 1–4 条主会话即可，不把这个值设为绝对数据模型上限。
- 两个以上 Target 的身份碰撞与不同 `CODEX_HOME` 是 G1 必测负例，不能只靠单机 G0 宣称跨主机支持。
- 用户明确禁止消耗 Codex/ChatGPT Work 共享额度；UI/审计用已有结构化事件与原生 CLI 适配，无模型后台摘要、无定时巡检、无私有仓库 Actions。
- G0 真实试用优先于大范围删除；未知的 Pending/OutcomeUnknown 写入回执要先备份、可核对，再安全退出旧 Forge/Worktree 写入模块。

## 5. 离线内容审计的完成标准

在**能够访问用户授权私有会话克隆的本地环境**一次性运行可复核的流式审计，至少检查普通 JSONL、独立 ZST、两组分卷、两份空文件、缺 session_meta、过大/损坏行、来源与父子 Agent、全量文件字节与 2,168 候选数。不输出原始聊天/绝对路径/凭据/ThreadId。若预期中的两份空文件依旧为空，审计应报告“已遍历全量已知候选、两份无法还原”，不能给“全部正文都完整读取”打 PASS。

独立工具 `codex_sessions_offline_audit_v2.py` 已在隔离合成夹具测试：普通文件、分卷 ZST、子 Agent、恶意枚举脱敏、空文件、缺分卷和预期体积不匹配通过。**该合成测试不是对私有 8.27 GB 的实际运行报告。**

## 6. 方案冻结与下一步

只修订 [L1.3 统一方案](../design/codex-tui-lite-L1.3-unified-final-20261119.md) 的关键语义，不产生另一个互相竞争的 L1.4 大方案。执行顺序仍是 **G0 真试用 → G1 精准会话/Attention → G2 需求/Bug → G3 两仓 UART → G4 删除冗余并重核发布资格**。若对照原生 Codex CLI+VS Code 的真实操作没有提升，就停止扩张，仅保留最薄的准确定位与可操作 Attention。
