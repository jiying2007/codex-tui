# Quiet visual regression / 静音视觉回归

## English

The existing `terminal-regression` Linux job now runs a source-bound, offline matrix against its actual freshly built binary: English and Simplified Chinese, 60x24 and 100x30, and normal/quiet/screen-reader presentation (12 cases). No additional scheduler, workflow or application feature is introduced.

Before entering the PTY, `scripts/testing/quiet_visual_matrix.py` checks the binary's embedded source SHA using one undersampled local diagnostic, then binds its SHA-256 to fixture metadata. This sample is identity evidence, not performance qualification. The expected SHA is the exact checked-out PR head or push commit, never a prior successful artifact. Relative output paths are resolved before children change working directory.

Each case uses a fresh profile, a fixed absolute Python mock App Server and a trap-only PATH, without inherited credentials or user configuration. The only allowed RPCs are initialization, queries and subscription operations; resume must exclude turns and conversation reads must target beta. A local Git trap may be invoked by ordinary repository probing, but the real Git client, Codex, Forge clients and notification programs are never resolved from the operator environment.

The probe verifies no historical replay at startup, idle not being claimed as success, visible alerts while external notifications are off, help retaining focus, exact-thread jump and body display, return to Registry, system-error visibility, no BEL, one backend start, normal exit and termios restoration. Its small terminal observer is tested with wide characters and rejects unsupported controls; it is not a general terminal emulator.

The matrix fails on the first unsuccessful case and never retries it. Partial checkpoints remain failed. Receipts, bounded terminal bytes, snapshots and fixture RPC traces are retained under the existing success/partial terminal artifacts. Existing output directories cannot be overwritten. The workflow limits the matrix step to four minutes and retains its existing ten-minute job limit. The 41 tooling tests also run under ordinary Python, `-O` and `-OO`.

This is hosted synthetic regression only: real Codex, real SSH, screen-reader qualification, human time savings and Stable authority remain unverified. `realCodexQualification=false`, `realSshQualification=false`, `screenReaderQualification=false`, `humanTimeSavings=null`, `stableReady=false`, `publicationAllowed=false`. F4 scale/convergence and F7 real onboarding/efficiency remain separate work under #211.

## 简体中文

既有 `terminal-regression` Linux 工作现在对刚构建的真实程序执行离线矩阵：英文/简体中文、60x24/100x30、normal/quiet/screen-reader，共 12 个组合。不新增调度、工作流或产品功能。

进入控制终端前，用一次不足采样的本地诊断核对二进制内嵌源码 SHA，再将二进制 SHA-256 绑定到夹具元数据。这只用于身份核验，不是性能资格。期望来源必须等于实际检出的 PR head 或 push 提交；不能复用旧成功产物。输出路径先转为绝对路径，避免子进程切换工作目录后丢失证据路径。

每个场景使用全新配置、固定绝对路径 Python 假后端和仅含陷阱的 PATH，不继承账号、密钥或用户配置。只允许初始化、查询和订阅 RPC，resume 必须排除 turns，会话读取只能指向 beta。普通仓库探测可能调用本地 Git 陷阱，但不会从操作者环境解析真实 Git、Codex、Forge 或通知程序。

实际检查启动不重放历史、空闲不冒充成功、静音视觉提醒、帮助层焦点保护、准确跳转及正文显示、返回列表、系统错误可见、无响铃、单次后端启动、正常退出和终端属性恢复。小型终端观察器覆盖宽字符并拒绝不支持的控制序列，不宣称完整终端模拟。

任一场景失败即停止，不自动重试；部分矩阵保持失败。回执、有界终端字节、画面快照和 RPC 轨迹进入既有成功/部分失败产物；不覆盖已存在的证据目录。矩阵步骤上限四分钟，既有工作上限十分钟保持。41 个工具用例在普通、`-O`、`-OO` 模式运行。

这些仍是托管合成回归，不代替真实 Codex、Windows SSH、读屏器、人时收益或 Stable 验收。所有真实资格及发布权限保持 false，人时收益为 null；F4 规模/收敛和 F7 真实上手/效率仍由 #211 分开跟踪。
