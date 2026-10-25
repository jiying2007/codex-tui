<!-- docs-id: ux-qualification -->
<!-- docs-lang: zh-CN -->
# 使用体验验收：证明有用、好用、易用
<!-- docs-section: overview -->

**语言 / Language:** [English](../../guides/ux-qualification.md) · [简体中文](ux-qualification.md)

这是需要现场人员执行的**真实体验对比**，不是代码 CI，也不意味着允许 Stable 发布。必须绑定候选精确源码 SHA 和实际二进制摘要。结果应归档到现有 [#211 外部资格交接](https://github.com/jiying2007/codex-tui/issues/211) 的私有证据位置，不创建第二套任务权威，不把真实任务、内部 GitLab 信息或口令上传公共仓库。

## 实验准备
<!-- docs-section: preparation -->

- 明确 Linux 程序的 SHA-256 与候选 Preview 包一致；运行 `codex-tui doctor compat`、`doctor codex`、`doctor terminal`，有内网授权才执行 `doctor forge`，仅保留经过脱敏的私有诊断。
- 准备至少 4 个仓库、50 个真实 Codex 会话和 30 个有代表性的任务，覆盖待输入/审批、执行中、完成、失败、评审及普通历史；1 万条以上历史另行测量，不把合成规模测试当作真实操作资格。
- 使用**同一位操作者、主机、终端尺寸、任务清单和真实权威后端**对比：A = 官方 Codex CLI + 现有 tmux + `glab`/`gh`；B = codex-tui。
- 以 ABBA/BAAB 顺序交替执行随机化任务块，区分冷启动和热切换。从展示任务要求起计时，到屏幕显示**正确任务上下文**为止；记录错误跳转与多余操作。
- 新手实验另请没有阅读完整长手册的人员参加，只提供一分钟入门，不要预先教授所有快捷键。
- 比较提醒前必须明确启用本地终端 BEL，并用 `printf '\a'` 验证 Windows SSH 终端的实际响应。分别使用 60×24、100×30 终端做检查。

## 私有记录格式
<!-- docs-section: recording -->

每次尝试一条 CSV，任务只用不透明编号，不包含仓库名称、真实任务正文、认证信息或对话全文：

```text
trial_id,chain,scenario,terminal,session_state,duration_ms,correct_target,accidental_mutation,notification_observed,notes
T001,A,needs-you,60x24,warm,12300,true,false,na,opaque-note
T001,B,needs-you,60x24,warm,8400,true,false,na,opaque-note
```

非提醒场景使用 `na`；首次失败必须保留。原始 CSV 与程序摘要共同归档，不应事后修改原始样本。

## 建议的验收目标（尚未取得现场结果）
<!-- docs-section: acceptance -->

| 维度 | 目标 | 评价方法 |
|---|---|---|
| 任务效率 | 配对端到端耗时中位数降低至少 25% | 按场景给出基线、候选、样本数；不能用合成 CPU 基准推导 |
| 跳转正确率 | 至少 100 次任务跳转，正确率 ≥99% | 错误会话、错页返回也要记录 |
| 安全操作 | 误审批、误合并、误删除或修改次数为 0 | 发现即阻塞，保留原始复现 |
| 定位任务 | 常见 Needs You 在 5 秒内找到 | 真实用户完整操作时间，不是画面渲染耗时 |
| 新手上手 | 无额外指导，10 分钟内完成浏览/搜索/切换 | 记录操作步数和帮助使用情况 |
| 提醒 | 记录 SSH BEL/视觉提示实际观测到的送达与回跳 | 区分真实观测和未收到，不承诺离线或 Windows Toast |
| 稳定性 | 完成现有 46 项设备/TTY/Forge 矩阵及约定多小时现场运行 | 托管 300 秒合成 Soak 不替代真实长时间验收 |

保留任务端到端耗时的 p50/p95、正确率、新手完成情况、不同场景失败数量和局限。没执行的指标必须标记 `NOT_RUN`，不可标记 `PASS`。

## 决策边界
<!-- docs-section: decision -->

如果原生 Codex + tmux + Forge 已经同样便捷，codex-tui 只保留跨仓库待处理聚合与交付状态投影；禁止继续叠加内嵌聊天客户端、独立任务调度、第二套 Agent/Queue 或终端管理器。优先删减重复能力，而不是新加功能。

真实 Windows→SSH Ubuntu、已认证 Codex、内网 GitLab、长稳、管理员及独立签核，以及后续 Stable `publish=false` 演练，全部仍由 [#211](https://github.com/jiying2007/codex-tui/issues/211) 控制；本文不改变 `stableReady=false`、`publicationAllowed=false`。
