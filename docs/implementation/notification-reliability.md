# Notification reliability / 通知可靠性

This change implements the notification portion of the 2026-10 audit. It does not grant Stable, real-account, terminal, GitLab or productivity qualification.

本次变更落实十月审查的通知部分，不授予 Stable、真实账号、终端、内网 GitLab 或效率收益资格。

## Outcome authority / 结果权威

A runtime transition from Working or WaitingHuman to Ready means only **Now idle; check result**. It does not establish a successful turn, task or delivery. Interrupted, failed and unknown outcomes must not be inferred successful from idle. Only an observed transition of an existing Goal to Complete emits **Goal completed**. This is a Goal observation, not independent acceptance of the overall task. Simultaneous Goal completion replaces the redundant idle notice for the same thread.

Working 或 WaitingHuman 转为 Ready 仅显示“**已空闲；请核对结果**”，不能证明轮次、任务或交付成功。中断、失败及未知结果不得从 idle 推断为成功。只有已经观测过的 Goal 明确转为 Complete 才提示“**Goal 已完成**”，仍不是任务独立验收结论。同一会话同时出现 Goal 完成和空闲时，只保留前者。

## Failure and limit edges / 失败与限制提醒

SystemError, UsageLimited and BudgetLimited now join approval, input, pipeline, Goal and review routing. Identical snapshots do not repeat an alert. Clearing a condition and observing it again produces a new edge. Initial/historically discovered items remain silently seeded, and snooze retains its existing routing semantics. This does not invent an event for changes that the upstream source has not reported.

SystemError、UsageLimited、BudgetLimited 现在加入审批、输入、流水线、Goal 和评审提醒。相同快照不重复告警；恢复后再次出现才生成新事件。首次或历史发现仍静默建立基线，Snooze 保持原有语义；不虚构上游尚未上报的状态变化。

## Silent operation / 静音运行

`[notifications].mode = "off"` disables **external delivery**, not in-app observation. It creates no notification actor or external-command queue and emits no bell or toast. The same one-item, 20-second recent-event header and `!` exact-thread jump work in silent mode. External delivery errors and queue overflow cannot prevent selection of the in-app event. Overflow still reports incomplete external delivery; visual display is not proof of bell/toast receipt.

`[notifications].mode = "off"` 关闭的是**外部投递**，不是界面内状态观察。它不创建通知分派任务或外部命令队列，不响铃、不弹系统通知，但保留原有单项、20 秒最近提醒和 `!` 精确回跳。外部通道错误或拥塞不会阻断界面提醒选择；拥塞仍明确报告外部投递不完整。界面显示不能证明响铃或桌面通知送达。

The tracker stores only the current projection, not a growing history. Its cost scales with the currently loaded registry. A redundant title-map allocation was removed. No persistent notification ledger, background service, scheduler, new dependency, mutation replay or protocol extension was added.

跟踪器仅保留当前投影，不积累历史；开销随当前已加载会话数量变化，并移除了重复标题映射分配。未新增持久通知台账、后台服务、调度、依赖、写操作重放或协议扩展。

## Verification boundaries / 验证边界

Regression coverage is in `src/notification/tests.rs`, `src/runtime_notifications.rs` and `tests/notification_semantics.rs`: idle versus success, Goal deduplication, historical seeding, repeated and recovered errors/limits, snooze, silent visual routing, exact-thread identity, delivery failure and queue bounds. Tests use local synthetic projections only. Candidate and merged-main CI must be recorded separately. This document is a behavior contract, not a claim that tests have already run or that field qualification passed.

回归覆盖包括空闲与成功区分、Goal 去重、历史基线、错误/限制重复与恢复、Snooze、静音界面提醒、精确会话身份、投递失败与队列边界，均使用本地合成投影。候选和合并主线的 CI 必须分别记录。本文是行为约定，不声称测试已执行或现场验收已通过。
