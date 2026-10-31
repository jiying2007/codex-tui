# Registry filter recovery / 会话列表筛选恢复

## English

When known sessions exist but none pass the current search or scope filters, Mission Control now displays `No matches · / l g ?` inside the otherwise empty list. `/` edits search, `l` toggles local-only scope, `g` toggles repository-only scope and `?` explains the existing keymap. The short hint remains readable in narrow terminals and Simplified Chinese.

This is display-only: it never clears filters, changes the selected identity, requests history, starts a backend or mutates tasks. A missing, failed or still-loading empty upstream inventory is not relabeled as a search miss. Clearing a filter restores normal rows without a phantom hint. Native TestBackend regressions cover both languages, 24/40/80-column windows, scope recovery, unknown inventory, resizing, Drawer space and the existing key bindings.

This reduces one discoverability problem; it does not establish novice completion time, screen-reader compatibility, real SSH latency or human productivity. Those remain separate F7/#211 evidence. `stableReady=false`, `publicationAllowed=false`.

## 简体中文

已有会话但被当前搜索或范围条件全部过滤时，任务中心在空列表中显示 `无匹配 · / l g ?`。`/` 编辑搜索，`l` 切换仅本机，`g` 切换仅仓库，`?` 查看既有键位帮助；提示保持简短，适用于窄终端和中文显示。

仅改变显示：不会自动清除过滤器、改动选中身份、读取历史、启动后端或修改任务。尚未观测、失败或仍在加载的空来源不会被误称为搜索无匹配。清除过滤后恢复正常行，不保留虚假提示。原生 TestBackend 回归覆盖双语、24/40/80 列、范围恢复、未知来源、缩放、Drawer 空间和实际键位。

这只修复一个可发现性问题，不证明新手操作时间、读屏支持、真实 SSH 延迟或人时收益；F7/#211 的现场证据仍需独立验收。`stableReady=false`、`publicationAllowed=false`。
