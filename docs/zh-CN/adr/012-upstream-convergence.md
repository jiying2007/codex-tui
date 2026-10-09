<!-- docs-id: upstream-convergence -->
<!-- docs-lang: zh-CN -->
# ADR-012：收敛到 Mission Control，不复制 Codex CLI
<!-- docs-section: overview -->

**语言 / Language:** [English](../../adr/012-upstream-convergence.md) · [简体中文](012-upstream-convergence.md)

**状态：已接受。** 官方 Codex CLI 正不断增加会话、Agent、Worktree、Queue 和日常交互能力。codex-tui 的长期价值不在于重复实现这些能力，而在于作为跨仓库、跨目标、跨 Git/Forge 的薄控制平面。此 ADR 约束新增功能和维护成本，审计以 [英文 ADR](../../adr/012-upstream-convergence.md) 为历史权威。

## 背景与问题
<!-- docs-section: context -->

多 Agent、对话客户端和终端的本地副本会形成双重状态机，增加互操作风险、维护成本和团队培训成本；同样的功能还可能被上游更好地实现。用户更需要可靠的 Needs You 聚合、准确仓库身份与 Review/Delivery 定位，而不是第二个 Codex。

## 定位与权威
<!-- docs-section: decision -->

codex-tui 必须维持**本地优先的薄 Mission Control**，其差异化能力是：
- 跨仓库/跨 App Server 的 Codex Session Registry；
- Needs You / Working / Ready 聚合、搜索和 Saved Views；
- 从 Codex/Git/Forge 派生的 Board 与 WorkCard；
- Git 工作树风险、GitLab/GitHub Forge 投影与经确认的写操作；
- Doctor、无头只读快照与 Support Bundle。

**Codex** 负责对话、Agent、Thread、Queue；**Git** 负责代码/Worktree；**GitLab/GitHub** 负责 Issue/MR/PR/流水线；**用户终端** 负责一般 Shell 生命周期。codex-tui 只存个人操作层元数据。

## 维护型功能与单一限额
<!-- docs-section: maintenance -->

现有本地会话编辑器、Thread Queue UI 与 Terminal Drawer 仅作安全/缺陷/互操作维护；新功能优先在官方 Codex 中实现。`release/v1.5-convergence.json` 维护重叠模块的名称、分类和策略（**不**保存第二套数值 LOC 上限）；`release/v1.4-plan.json` 是唯一的全 Rust 模块 LOC Ratchet。CI、Development Qualification 与 Release Gate 都会运行 `scripts/architecture/check_upstream_convergence.py` 并拒绝绕开冻结。

上游 Worktree 能力另设冻结边界：官方 Codex v0.162.0 已增加受功能开关
控制的托管 Worktree 创建与列表。原有 `src/worktree*`、`src/operation.rs`
与 `src/runtime_commands.rs` 仅保留 Git 安全校验、恢复和必要的兼容后备，
统一登记为 `managed-worktree-operations`；不新建第二套 Worktree 权威，
也不继续堆叠重复创建/列表界面。只有真实 Codex 目标完成能力协商、
工作树身份与冲突安全等价及失败恢复验证后，才能考虑委托上游。
合成测试不能代替互操作验收，Git 始终是仓库/Worktree 状态的权威。

## 明确冻结的重复能力
<!-- docs-section: freeze -->

不得引入自有 Agent 编排/委派/消息系统、独立任务 Queue 权威、独立会话协议、通用 Agent Provider、共享协作后端或通用终端管理服务。未来真的需要改变架构时，应先有可量化的用户价值和设计 ADR，再调整保护清单；不能在补功能时悄悄改变边界。

## 工程后果和退出标准
<!-- docs-section: effects -->

Mission Control、Attention、Forge 和诊断可以继续改善；与官方 CLI 重叠的本地能力应保持不增长，必要时在证据充足后退役。Board 始终为投影，不是另一套团队数据库。即使所有机器测试通过，也不能据此宣称真实 Codex/GitLab/SSH 终端已验收。

