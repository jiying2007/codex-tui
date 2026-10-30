# Idle planning-result reclamation / 空闲规划结果回收

## English

The planning worker previously reclaimed a retired result only when the next projection input arrived. After a quiet successful installation, a complete source snapshot and old cards could remain owned by the foreground indefinitely.

Projection and reclamation now share the existing worker and its two one-slot channels. When an update or clock expiry already requires computation, retirement still travels with that input. Otherwise, the foreground sends a reclamation-only job. The worker drops the retired data and returns an empty acknowledgement. It does not capture a new snapshot or build another projection. Updates received while reclamation is in flight coalesce in the authoritative AppState; the next input is captured only after acknowledgement.

There remains one in-flight job. No timer, additional thread, network call, new authority, persistence, dependency or write replay is introduced. Source generation, operator intent, freshness and selected-item admission remain unchanged. Disconnects report degradation and retain the installed projection. The existing unit-test identifiers are preserved when their module is extracted; new deterministic tests cover idle reclamation, coalesced updates, snooze expiry and disconnects, plus a real worker-thread regression.

This closes idle ownership of retired data, not a measured RSS target. Allocators may retain freed pages. Snapshot cloning, continuous high-churn convergence, real SSH latency and human productivity still require their own evidence. Existing release and real-environment gates remain unchanged: `stableReady=false`, `publicationAllowed=false`.

## 简体中文

旧实现只有在下一次规划输入到来时才回收旧结果。成功安装后如果没有新事件，前台可能一直持有完整来源快照和旧卡片。

本次复用既有 Worker 和两个单槽通道：需要计算时，旧结果仍随输入交给 Worker；无需计算时，只发送回收任务，由 Worker 释放后返回空回执，不克隆快照、不重新计算。回收期间的新变化继续保存在当前 AppState，收到回执后才捕获最新输入，多次变化合并为一次计算。

仍然只有一个在途任务，不增加定时器、线程、网络请求、持久化或写入重放。来源代次、操作意图、过期判断和选中身份规则不变；Worker 断开时明确降级并保留已安装视图。原单元测试迁移后保留标识，新增空闲、连续变化、Snooze 过期、断开及真实 Worker 线程回归。

这证明的是旧数据所有权能够在空闲时释放，不是 RSS 必然下降；分配器可以保留已释放内存页。快照复制、持续高频变化下的收敛、真实 SSH 延迟和人时收益仍需独立验收。正式发布与真实环境门禁不变：`stableReady=false`、`publicationAllowed=false`。
