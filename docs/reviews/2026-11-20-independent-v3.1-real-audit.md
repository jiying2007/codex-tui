# 2026-11-20 Codex Sessions 独立 v3.1 真实审计：结果与 L1.3 决策

**审计来源：** 维护者提交的脱敏 `audit-v3.1.aggregate.json`（`schema=codex-sessions-independent-audit/v3.1`，`tool_version=3.1.0`，报告 UTC 时间 2026-11-20 02:03:52）；审计针对既有固定 `jiying2007/sessions@98cc404fd6f3c621bfe022d67c3efe82d01562b5` 快照。  
**方案来源：** [Lite L1.3 统一方案](../design/codex-tui-lite-L1.3-unified-final-20261119.md) 与 [v2/v3 实测对账](2026-11-19-v2-v3-real-audit-reconciliation.md)。  
**数据边界：** 本文仅包含安全聚合统计，不复制私有审计 JSON 或档案正文、会话 ID、主机身份、实际文件路径、凭据。审计器的本地运行结果不是已部署 TUI 的现场状态，也不是全账号所有主机覆盖保证。

## 1. 本次真正完成的三道门禁

| 项目 | v3.1 真实报告 | 判定 |
|---|---:|---|
| Git 文件对象 | 2,170 / 2,170 | `inventory_matches_expected=true` |
| Git Blob 内容哈希 | 2,170 / 2,170 | `all_known_objects_hashed=true` |
| 候选档案组 | 2,168 / 2,168 | `all_known_groups_attempted=true` |
| 发现字节总量 | 8,274,174,477 | 与既定快照一致 |
| 双 UUID 特殊文件名 | 1 个组 | 本次匹配，`source_name_irregularities=0` |
| HEAD 中途变化 | 0 | `git_head_pinned=true`，`head_changed_during_audit=0` |
| 缺少/未支持的跟踪对象 | 0 | `missing_tracked_objects=0`，`unsupported_tracked_objects=0` |
| 非空已解码档案 | 2,166 | 其余 2 个零字节 |
| 可解析的 JSON 记录 | 3,041,293 / 3,041,360 行 | `records_parsed`，不等同业务会话数量 |
| 无损内容完整性 | 未通过 | `status=PARTIAL`；`full_content_audit_complete=false` |

**结论：v3 漏掉的 13,880,507 字节特殊命名档案，已在 v3.1 的完整范围和哈希核验中收口。** 此结论仅对上述固定仓库快照有效。

## 2. 剩余的真实内容异常（不能掩盖）

- **2 个空档案**：有合法对象但无正文。不能恢复为有内容的原会话；`all_content_available=false`。
- **32 个组存在 JSON 解析错误，合计 67 行**：`json_syntax=64`、`invalid_utf8=3`。`oversized_lines=0`；没有原始异常行就不能推断是终止写入、来源污染、编码错误或磁盘损坏。其占非空档案约 1.48%；按观察到的全部行占约 0.0022%，极低的行比例不意味着可忽略。
- **79 个组出现重复 `session_meta`**，`repeated_session_meta_records_same_thread_id=0`，`repeated_session_meta_records_conflicting_or_missing_id=79`。**注意后者是合并分类**：现有公开汇总无法区分“第二条无 ID”与“第二条带不同 ID”，不得报告“已证实 79 个真实 ID 冲突”。这些额外 `session_meta` 可能属于恢复/追加形态，但尚未证实。
- **2165 个无主机身份验证的独立 rollout ThreadId、1 个重复 ID 观测**：不能将文件名或同 ID 跨主机合并。`unresolved_parent_edges_in_export=0` 只证明已解析的 747 条父边在归档索引中有目标，不证明实时服务能正确路由。
- 数据能证明**已上传已知档案**的哈希与范围，不能证明其他主机/未上传档案存在与否；`all_account_hosts_covered=false`。

建议在有授权本地克隆的主机上只用私人 `--private-triage-out` 自查 32 个组和 79 个组，记录**缺失 ID 与真实不一致 ID 的分别计数、67 行错误的稳定复现情况**。不要手工修补原始 Git Blob 以求 PASS；如需产出脱敏增量统计，应明确区分“原档完整”和“有界可恢复的局部索引”。

## 3. 真实使用结构：针对性优化，不新增重型系统

`source` 合计 2166 个已解码档案：明确子 Agent 747 + 其他子 Agent 12（759，约 35.04%）；VS Code 669；CLI 481；Exec 230；其他/未知 27。`originator=codex-tui` 为 1615，仅代表历史发起标签，不等于当前 UI 控制或使用模型。

| P0 决策 | 必须具备的行为与验收 |
|---|---|
| **查询覆盖说明** | 在具体 App Server Target 上按版本探测 `thread/list` 来源、归档、Provider 与分页范围；缺来源不能展示“全部无待办”，而是 `Partial/Unknown`。`sourceKinds` 省略/为空时不能冒充包括全部子 Agent/Exec |
| **精准会话分组** | 只基于已验证 `parentThreadId` 折叠子 Agent；Fork 保持独立；所有可操作子线程审批仍可从父会话准确跳转到真实 Request |
| **跨 Target 安全身份** | Target + `CODEX_HOME` 作用域 + 账号/Provider（可验证时）+ ThreadId；别名和同名 CWD 不具备身份权威 |
| **只读/可继续区分** | 原生 Resume 能力探测、Provider 不匹配拒绝假接续；归档索引不得从历史文件静默升级为 Live 会话 |
| **Attention 退役前置** | 在 `Planning/WorkCard` 删除前建立真实 Request/Turn 的注意力投影，以混合来源、断线、旧协议和 747 条父边类别做回归 |
| **真实开发闭环** | 飞书项目/TAPD 继续为需求和缺陷权威；SoC↔MCU 联调使用契约 SHA、适用时 gitlink SHA、双端真实验证 |
| **用户价值** | 先 G0（真实 Windows SSH/Ubuntu）→ G1（会话与 Attention）→ G2/G3（需求/Bug/UART）→ G4（删除冗余），用原生 CLI+VS Code 对照真实找回耗时和误操作 |

**不得新增：** 在线 8GB 历史扫描、定时巡检、Agent 消息总线、第二套 Git/Issue/Forge 写入、自动审批/模型推理、私有记录复制到公共仓库。

## 4. “完成”与“未完成”的准确表述

**已完成：** 已知固定仓库快照的 Git Tree 对象、字节、哈希和候选组全量覆盖；3,041,293 条有效 JSON 记录的流式审计；真实 v2/v3/v3.1 对账；审计规格和方案 P0 边界调整。

**未完成：** 32 个解析异常组与 79 个重复元数据组的源级定性；无损全正文资格；所有账号主机覆盖证明；G0/G1 的 TUI Rust 运行时改造和实机验收；Stable 发布资格。

**决策：** v3.1 的 `PARTIAL` 是诚实的内容层判定，不能通过隐藏 67 行错误或忽略 79 组重复元数据“改为 PASS”。L1.3 继续作为唯一下一阶段方案，无需引入并行设计版本。
