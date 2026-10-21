<!-- docs-id: release-qualification -->
<!-- docs-lang: zh-CN -->
# v1.4 发布门禁、真实验收与交接
<!-- docs-section: overview -->

**Language / 语言:** [English](../../guides/release-qualification.md) · [简体中文](release-qualification.md)

v1.4 已完成开发范围，**并非**已发布的 Stable，也不是完成真实部署的产品。公开 v1.0.0 只是历史发布记录，未被应用部署。本页是使用指南，不设立新发布权威；以 [正式资格 JSON](../../../release/v1.4-criteria.json) 和 [#211 交接单](https://github.com/jiying2007/codex-tui/issues/211) 为准。

## 机器自动化资格
<!-- docs-section: automated -->

同一**受保护 main 精确 SHA** 应按现行策略通过 Linux/macOS/Windows + Rust CI、Development Qualification、Security/Release Gate、10k/50k 规模、UI 诊断、PTY 和结构化 Soak。保留每个 Run ID 及 Artifact SHA-256。发行包还需原生元数据、ABI、许可证、Notices 与校验和。PR 通过不能替代 fresh-main。

Development Qualification 额外保留 `failure-case-execution.json`，逐项记录 Failure Matrix 测试确实被单独执行、进程耗时和输出摘要。回执标识 `recoveryLatencyMeasured=false`：测试进程的启动/运行耗时不等同于程序故障恢复延迟；没有独立的故障注入计时证据，就不得声称已满足 `maxRecoveryMs`。

协议资格按证据级别区分：**L1** 是注册表、未知事件与非法 JSON 回放；**L2** 使用合成 WebSocket/Unix 传输和 App Server Registry 生命周期测试（含通知顺序、断开与非法消息）。L2 另外验证正在等待 RPC 时到来的服务端交互请求能及时交回事件分发器：保留请求，不将原 RPC 误标成功。两者均非真实 Codex 认证。**L3** 必须在同一候选 SHA 上实际观察最新 Codex App Server 能力、真实终端和 Provider，未完成前保持外部阻塞。

## 真实环境必须单独验收
<!-- docs-section: external -->

[设备集成手册](device-validation.md)和[初始 NOT_RUN 见证矩阵](device-matrix.md)使用仓库内辅助工具执行现有夹具、只读进程观察和一致性 SQLite 备份；不记录真实 TTY PASS，不触发发布。

`capture_forge_capability.py` 和 `validate_internal_gitlab.py` 拒绝已有输出文件和符号链接（含悬空链接），并以独占创建防止并发写入覆盖已保留证据。Forge 采集在生成收据前校验构建身份、时间、Provider 和能力字段类型；启动、解码、快照读取和写入错误不回显任意子进程输出或非法字段。重复且冲突的能力要求会在探测前被拒绝。可选服务器元数据缺失、Provider 别名、格式正确的新增能力名称及明确不合格的诊断收据仍受支持；未满足要求时保留诊断并返回退出码 3，不提升为准入通过。内部 GitLab 门禁仍对原始输入字节计算哈希，保留时效、源码、认证和能力检查，且不授予 Stable 权限。这些保护不代表本地存储不可变，也不证明真实凭据已完成验收。

`capture_compat.py` 对兼容性 doctor 设置 45 秒执行超时，失败时不回显子进程输出或非法报告字段。只有 ready、源码匹配、构建身份字段类型正确且观测时间为正整数的报告才能被保留。已有输出文件（包括符号链接）会在探测前被拒绝；独占创建还防止覆盖探测期间由另一采集进程写入的报告。失败不输出成功摘要。每次采集应选择新的输出路径；这能避免意外替换已保留的证据，但不代表文件系统不可变。

`linux_qualify.py` 会在网络查询、编译和基准测试之前校验传入的 Linux 终端收据；缺失、格式错误、非 PASS 或源码不匹配时立即停止。通过校验的 UTF-8 原始字节保留到新 `--output-dir` 下的 `terminal-restoration.json`，解析和 SHA-256 均使用这一份快照，不再重新读取原输入路径；已有快照不会被覆盖。这只是验证操作者提供的收据，不替代真实终端见证，不生成虚构 PASS，也不授权发布。

必须独立采集并绑定当前源码：①真实登录 Codex App Server/Provider 能力；②内部使用档真实 `glab` 及 Issue/MR/Pipeline 可用；③Linux 和 Windows→SSH Ubuntu 的**控制终端** Focus/Resize/Ctrl+C/退出/恢复；④实际机器的性能、资源和异常恢复。托管 CI 不得伪造为真实 PASS。

## 管理员独立治理
<!-- docs-section: admin -->

Stable 前管理员须验证 `main` 严格 Required Checks（含 GitHub Actions App ID 绑定）、禁止 Force Push/删除、管理员同样受保护，以及 **GitHub Immutable Releases** 已开启。没有权限或读取失败就是阻塞，不能绕过；管理员令牌不得进入公共 PR 工作流。

`configure_main_protection.py` 默认只生成不写入的计划，不默认新增审批要求；已有审查规则及其撤销/绕过主体、推送限制、额外检查及 App 绑定、线性历史、讨论解决要求、分支锁定、分支创建限制和 Fork 同步设置均须保留。只有完整读取保护配置成功且能安全规范化策略，才能给出可执行计划。完整读取失败（包括 403/404）、策略不完整或保护语义未知时拒绝写入；404 不证明保护不存在。本辅助工具不初始化不可读或未受保护的 `main`；管理员须先建立已审阅且可读取的初始保护策略。

应用仍须显式传入 `--apply --confirm-repository jiying2007/codex-tui`。每轮完整 GET 成功后，缺失的可选资源都通过各自端点确认。工具在 PUT 前立即重读保护配置和分支元数据，发现可观察的策略变化、保护失效或 main 偏离计划的精确源码 SHA 即停止；PUT 后通过独立 GET 验证最低策略、全部保留设置及同一受保护 main SHA。签名保护不写入此 PUT，且已有签名要求不得下降。此端点没有 `If-Match` 原子更新保证，因此这些检查不能消除并发修改竞态。失败不会自动放松保护、重试写入或回滚；若 PUT 后读回失败，结果仍未核实，设置可能已经改变。操作步骤见[主线保护说明](../../implementation/m7d3-release.md#main-branch-protection)。

## 精确候选 Stable 非发布演练
<!-- docs-section: dryrun -->

Stable `publish=false` 与普通 Preview 自检不是同一资格。必须绑定候选版本/Tag、Canonical CI、真实 Linux Doctor/TTY/性能及 `release-evidence/v5` 必须字段，才可宣布 Stable 演练有效。不能编造时间戳、掩盖摘要或复用旧候选证据。

## 正式发布必须授权
<!-- docs-section: publication -->

正式 `publish=true` 之前必须审阅发布说明/日期、不可变 Releases 和主线 HEAD 的再次读取、与演练完全一致的保留产物，以及**明确的所有者授权**。发行发布和远程资产校验属于独立敏感操作，任何文档或模拟 Fixture 不能绕过。

## 交接与历史记录
<!-- docs-section: archive -->

在真实/管理员证据全部取得之前，保持 [#211](https://github.com/jiying2007/codex-tui/issues/211) 为 Open。不要强删独立分叉的历史分支、重写 v1.0.0 GitHub 发布事实或把 v1.2/v1.3 完成记录当作实际部署后的升级链。本次为 **v1.4 首次安装**，不是历史数据迁移。

