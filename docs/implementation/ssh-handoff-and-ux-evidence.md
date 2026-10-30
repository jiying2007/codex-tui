# SSH handoff and UX evidence / SSH 外部交接与体验证据

## External opening / 外部打开

Opening an editor or browser always executes on the **TUI host**, not automatically on the SSH client. With `SSH_TTY` or `SSH_CONNECTION` present, an unconfigured opener is now refused before process creation. The existing notice explains the location, names `CODEX_TUI_EDITOR` or `CODEX_TUI_BROWSER`, and shows the sanitized target for manual copying. This avoids silently invoking a GUI on an unattended Ubuntu host.

编辑器和浏览器命令始终运行在 **TUI 所在主机**，不会自动在 SSH 客户端运行。检测到 `SSH_TTY` 或 `SSH_CONNECTION` 后，未配置的默认打开器会在创建进程前被拒绝。现有提示说明执行位置、相应的 `CODEX_TUI_EDITOR` 或 `CODEX_TUI_BROWSER`，并显示已净化的目标供手动复制，避免静默调用无人值守 Ubuntu 上的图形程序。

Configure a **single executable**, not a shell command string, when a trusted remote-editor or browser adapter is available. Its only target argument remains the validated URL or canonical in-worktree path. The adapter must be explicitly configured on the TUI host; this project does not install a bridge, copy credentials, use OSC 52, execute client commands, or parse shell snippets. Native non-SSH defaults remain unchanged. Empty overrides, unsafe URLs and escaping paths still fail before spawn. Existing child reaping is retained.

需要可信远程编辑器或浏览器适配时，配置的是**单一可执行程序**，不是 shell 命令字符串。唯一目标参数仍是校验后的 URL 或工作树内规范路径。适配程序须明确配置在 TUI 主机；项目不安装桥接、不复制凭据、不使用 OSC 52、不执行客户端命令、不解析 shell 片段。非 SSH 默认行为不变；空配置、危险 URL、越界路径仍在启动前拒绝，原有子进程回收保留。

A successful spawn means **handoff requested**, not that a document appeared on the client or that the external application later exited successfully. Later application exit status is still not a UI receipt. Long targets may be clipped by the existing notice; use the original Review/Forge source to obtain the full target. This change does not claim automatic Windows desktop opening or a new copy dialog.

启动成功只表示**已请求交接**，不证明客户端已展示文档，也不证明外部程序最终成功退出。后续退出状态仍不是 UI 回执；长目标可能被现有提示行截断，应从原 Review/Forge 来源取得完整目标。此变更不宣称自动打开 Windows 桌面或新增复制对话框。

## Quiet notifications / 静音通知

As implemented in #385, `notifications=off` disables external delivery only. In-app observation and the existing one-item, 20-second `!` jump remain available. Runtime idle means **Now idle; check result**, not task success. An observed Goal completion is only a Goal observation. System errors and usage/budget limits join the edge-triggered alert categories. The [notification behavior contract](notification-reliability.md) is the detailed authority for these semantics.

按 #385 的实现，`notifications=off` 仅关闭外部投递；界面内观察和原有单项、20 秒 `!` 回跳保留。运行态空闲仅表示**已空闲；请核对结果**，不是任务成功；Goal 完成也只是 Goal 观察。系统错误、额度与预算受限加入边沿触发告警。详见[通知行为约定](notification-reliability.md)。

## Offline analysis / 离线分析

The following standard-library Python 3.8+ tools consume already captured files. Neither tool executes the product, calls a backend, sends model requests or grants release authority. Use a new output filename; existing outputs are never overwritten. Real task CSVs and private targets must not be uploaded to this public repository.

以下工具仅使用 Python 3.8+ 标准库，读取已采集的文件；不执行产品、不访问后端、不调用模型、不授予发布资格。输出须使用新文件名，已有文件不覆盖；真实任务 CSV 和私有目标不得上传公共仓库。

```bash
python scripts/release/check_frontend_budgets.py interaction-performance.json --source-sha SOURCE_SHA --output proposed-budgets.json
python scripts/release/evaluate_ux.py trials.csv --source-sha SOURCE_SHA --binary-sha256 BINARY_SHA256 --data-kind recorded --output ux-analysis.json
```

Replace the uppercase placeholders with the full source SHA and executable digest from the retained experiment. The budget tool checks source identity, samples, complete fixture coverage, numeric validity and proposed p95 budgets. Exit 3 denotes a budget miss, 2 invalid input and 0 compliance with the **proposal only**. It is not a repository-required timing gate or terminal SLO and does not fix the known 50k foreground cost.

大写占位符须替换为所保留实验的完整源码 SHA 和可执行文件摘要。预算工具校验来源、样本、完整场景及数值，并对比建议的 p95 预算。退出 3 表示超预算，2 表示输入无效，0 仅表示符合**建议值**；它不是仓库必需的时延门禁或终端 SLO，也没有修复已发现的 50k 前台成本。

The UX tool follows the existing [UX trial protocol](../guides/ux-qualification.md). It keeps every failed attempt in aggregate and scenario/terminal/cold-warm summaries, strictly pairs A/B trials, never exports notes or trial identifiers, and binds input bytes by SHA-256. Exit 0 only covers its enumerated numeric target subset. Input identity, ABBA randomization, live authenticity, repository/session counts, notification delivery, onboarding and prolonged stability remain **unverified**. Both tools always emit `stableReady=false` and `publicationAllowed=false`.

体验工具沿用既有[体验试验协议](../zh-CN/guides/ux-qualification.md)，在总计及场景/终端/冷热分组中保留全部失败，严格配对 A/B，不输出备注和试验编号，并以 SHA-256 绑定输入字节。退出 0 只覆盖列明的数值目标子集；输入身份、ABBA 随机化、真实测量、仓库/会话数量、提醒送达、上手及长期稳定性仍**未验证**。两个工具始终输出 `stableReady=false`、`publicationAllowed=false`。

## Remaining acceptance / 剩余验收

Native CI must independently qualify this exact change. SSH process traps are synthetic tests, not Windows-to-Ubuntu field qualification. F3 explicit startup retry, F4 foreground production optimization, full external-app feedback, F7 onboarding/accessibility and real efficiency evidence still require their own implementation or acceptance. Keep #211 open; do not infer completion from a green analysis unit test or publish/deploy Stable.

原生 CI 须独立验证本次精确源码；SSH 进程陷阱只是合成测试，不是 Windows→Ubuntu 现场资格。F3 显式启动重试、F4 前台生产优化、完整外部程序反馈、F7 上手/可访问性和真实效率证据仍需各自实施或验收。保持 #211 开放，不凭分析工具单测通过推导完成，不发布或部署 Stable。
