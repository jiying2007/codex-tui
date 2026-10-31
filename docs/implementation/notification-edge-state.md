# Notification edge-state retention / 通知边沿状态驻留

## Scope / 范围

The notification tracker needs previous identities and states, not previous
presentation text. Between observations it now retains only active event keys,
known card IDs, runtime statuses and Goal statuses. Thread titles, aliases, Goal
objectives and full notification payloads remain available in the current input
or emitted event, but are not duplicated in the tracker's previous-state maps.

通知跟踪器跨两次观察只需要事件键、已知卡片 ID、运行状态和 Goal 状态，不需要
保留旧标题、别名、Goal 正文或完整事件。当前输入和输出事件的文本不变；这里只
去掉跟踪器中不参与边沿判断的重复驻留数据，不删除任务、会话、草稿或本地元数据。

Empty previous maps provide the same initial-history suppression: a card must
already be known to emit a new attention edge, and runtime/Goal transitions need
a previous status. An empty observation forgets removed identities. Renaming a
subject alone is not a new edge; a later edge uses its current subject.

首次观察仍不重放历史：待处理事件必须对应已知卡片，运行态与 Goal 边沿必须存在
先前状态。空快照会遗忘消失的身份；单独改名不产生提醒，后续事件使用最新名称。
Goal completion still suppresses a simultaneous idle event / Goal 完成仍合并同批空闲提示。

## Validation / 验证

`tests/notification_edge_state.rs` covers retention, clone de-duplication, current
aliases and fallback Goal titles, repeat/clear/reappear behavior, removal and
rediscovery, Snooze, and 10k/50k registries with long titles. Existing notification
unit tests, runtime delivery tests and the exact-source quiet visual PTY matrix
remain the acceptance path. No new dependencies, process, model call, persistence
schema, event queue, timer or release authority is introduced.

新增回归覆盖上述驻留与边沿行为，并使用 10k/50k 长标题夹具验证正确目标和历史
抑制；沿用原生 CI 与已入库的静音视觉终端矩阵。未新增依赖、进程、模型调用、
持久格式、队列、定时器或发布权限，未放宽模块上限。

This is a retained-state reduction, **not** an assertion of measured RSS or CPU
improvement. Observation construction still owns its current strings and maps;
planning snapshot copying and sustained high-churn convergence are separate work.
The debug-state assertion checks the retained representation, not secure memory
erasure or an allocator-level byte measurement. Real SSH, accessibility, human
productivity, multi-hour and #211 qualification remain independent.

本改动不宣称已测得 RSS/CPU 改善：构造当前观察仍会分配字符串和映射；规划快照
复制与持续更新收敛仍是独立事项。Debug 回归检查驻留表示，不证明内存清零或精确
分配字节数。真实 SSH、读屏器、人时收益、多小时使用及 #211 资格仍需独立证据。
`stableReady=false` and `publicationAllowed=false` remain unchanged.
