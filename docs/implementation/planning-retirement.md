# Bounded planning retirement / 有界规划结果回收

## Production change / 生产路径变更

The existing planning worker now owns bulk reclamation of the preceding projection. Installing a current result swaps cards and indexes instead of destroying the old vectors/maps on the interaction thread. Rejected results also remain owned rather than being destroyed inside the rejection branch. The runtime holds at most **one retired result** and transfers it with the next input to the existing worker, which drops it before computing the new projection.

现有规划 Worker 接管上一份结果的集中回收。安装有效结果时交换卡片和索引，不在交互线程销毁旧集合；拒绝过期结果时同样保留其所有权。运行时最多保留**一份待回收结果**，随下一次输入交给原 Worker，在新投影计算前释放。

There is still one computation in flight and one slot in each channel. No extra thread, timer, unbounded queue, backend call, dependency, persisted state or schema is added. Source generation, operator intent, clock expiry, current selection and the 64-card rebase/10k eligibility rules are unchanged. This is an ownership/latency change, not permission to install stale state.

仍只有一个在途计算及两个各为单槽的方向队列；不增加线程、定时器、无界队列、后端调用、依赖、持久状态或格式。来源代次、操作意图、时钟过期、当前选中项，以及 64 卡片调整预算和 10k 资格边界均保持。这是所有权与时延整改，不允许安装过期状态。

## Resource and timing boundaries / 资源与计时边界

When no new projection is needed, the single retired result remains resident until the next refresh or shutdown. This trades bounded retention for avoiding an interaction-thread bulk drop; it does not claim lower RSS. Snapshot capture still clones metadata on the foreground thread. Sustained changes above 10k can still require retries; this change alone does not establish convergence under continuous churn or a terminal latency SLO.

没有新投影需求时，单份旧结果保留到下一次刷新或退出；这是以有界保留换取避免前台集中析构，不宣称降低 RSS。快照捕获仍在前台克隆元数据；超过 10k 的持续变化仍可能要求重试，不能仅凭此改动宣称连续变化下已收敛或达到终端时延指标。

The retained frontend diagnostic now times borrowed application/rejection without destroying the result inside that scope, matching the production deferral. Reclamation remains real work on the worker, not removed work. Compare exact-source p95/p99/max, worker throughput and RSS before declaring an improvement. Proposed budgets are unchanged; no timing gate is relaxed.

前台诊断计时改为借用结果执行安装/拒绝，作用域内不销毁结果，与生产延后回收一致。回收仍是真实的后台工作，不是消失的成本。必须比较精确来源的 p95/p99/max、Worker 吞吐和 RSS 后才判断效果；建议预算不变，没有放宽时延门禁。

## Regression and qualification / 回归与资格

The deterministic stale-result test verifies that retirement travels with the replacement input. Public worker regressions cover 20/12k metadata rows, consecutive changes, idle polls and complete removal, alongside the existing source/intent/clock/selection cases. They use synthetic metadata only. Native CI and exact-SHA diagnostics must validate the candidate; no live Codex, SSH, GitLab or user-efficiency qualification is granted. Keep #211 open and Stable publication disabled.

确定性过期结果用例验证旧结果随替换输入转交；公开 Worker 回归覆盖 20/12k 条元数据、连续变化、空闲轮询和全部移除，并保留原来源/意图/时钟/选中项用例。仅使用合成元数据；候选仍须原生 CI 和精确 SHA 诊断，不授予真实 Codex、SSH、GitLab 或效率资格。#211 保持开放，Stable 发布继续关闭。
