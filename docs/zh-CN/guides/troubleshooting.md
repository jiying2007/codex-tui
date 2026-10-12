<!-- docs-id: troubleshooting -->
<!-- docs-lang: zh-CN -->
# 故障排查：Codex、Forge、SSH 与 SQLite
<!-- docs-section: overview -->

**Language / 语言:** [English](../../guides/troubleshooting.md) · [简体中文](troubleshooting.md)

先确认失败的是哪个权威，不要靠删除本地状态、改写上游凭据或伪造恢复成功。记录精确源码 SHA、目标类型和脱敏错误类别。敏感漏洞按 [安全披露](../../../SECURITY.zh-CN.md) 处理。

## App Server 连接失败或线程缺失
<!-- docs-section: codex -->

执行 `codex-tui doctor codex`（命名目标可附 `--target NAME`），独立检查真实 `codex --version` 和账号登录。远程 WebSocket/Unix 建连及握手受超时限制，超时不代表已发出的请求必定未执行。核对服务进程、TLS 代理、认证及能力错误；可选方法可以降级，但不会自动切换 `--fake`。Issue 不得粘贴 Bearer Token。

stdio 半帧缓冲会跨定时器或命令取消保留；后端 EOF 的最后离线快照只交付一次。重复或循环游标、分页无进展以及总预算耗尽都会明确标为清单不完整，不能作为完整的变更授权证据。搜索跳转在有界全局历史中定位精确条目，并保留相同查询范围的续页游标；目标删除或预算耗尽时报告失败，不静默跳到其他位置。

RPC 通知最多保留 1024 条且受 16 MiB 载荷记账预算限制。待处理交互请求最多 64 条、每条 256 KiB、累计 4 MiB。分页限制为 1024 页、200,000 个身份、32 MiB 身份存储记账预算及整次扫描 120 秒。这些是载荷或记账限制，不是实测 RSS 保证。过载会明确终止或隔离会话，不随机丢弃审批消息，也不重放写 RPC；正常 5 万条清单仍可在这些预算内处理。

发送被取消或失败后，连接进入隔离状态：部分交付结果未知，后续请求不能接到未完成帧上，也不会自动重放写操作。包括通知在内的每次发送都受五秒截止时间限制。RPC 期间的底层读取、JSON 解码失败或 EOF 不能再被报告为连接正常。合法的可选方法拒绝，以及仅等待响应时被取消，不会隔离本来完整的连接。

连接状态覆盖整个 RPC 会话，而不只是底层通信：消息条数或字节准入失败与传输失败，都会使会话不可用并标为离线。Registry 启动加载已隔离时，不再发起可选 loaded-list 探测，也不覆盖首个故障原因。合法的可选方法拒绝仍表示连接正常但能力降级。

## GitLab/GitHub 不可用或字段不全
<!-- docs-section: forge -->

进入出现问题的仓库，**私下**检查 `git remote -v`，依次执行 `codex-tui doctor git`、`codex-tui doctor forge`。在工具外登录对应主机的 `glab/gh`。GitLab Issue/MR/Pipeline 独立探测，一项能力失败不应让其他成功内容消失。`doctor compat` Ready 不等于内部 GitLab PASS；内部部署参考 [能力准入](../qualification/provider.md)。

GitLab 审批状态 JSON 异常，或缺少 `rules`、`approvals_required`、`approved` 时，证据属于未知，客户端阻断合并。明确有效的空规则数组不同于字段缺失；客户端检查不替代 GitLab 服务端保护。

Forge 的 stdout/stderr 读取与命令共享同一个异步作用域及原有五秒期限。超时或调用者取消时，两个本地管道读取端同时释放，即使后代进程仍持有写端，也不会遗留脱离作用域的读取任务。这不授予终止无关进程组或重试未知结果写操作的权限。

## Windows 路径拼接或仓库会话不匹配
<!-- docs-section: cwd -->

Windows→SSH Ubuntu 使用的是 **Ubuntu 侧** Codex Home、当前工作目录及 Git 身份。历史会话包含 Windows cwd 时属于外来源，不是本地 Ubuntu Checkout。核对 `doctor codex` 的路径和实际仓库目录的 `doctor git`；不要拼接 Windows 绝对路径，也不要编造不存在的 Ubuntu 本地会话。

## Drawer 聚焦、Ctrl+C、窗口重绘异常
<!-- docs-section: terminal -->

使用应用内 `?` 查看实际键位：`t` 打开 Drawer，`F6`（或支持时 `Ctrl+]`）返回聚焦。远程终端或 SSH 客户端可能截获快捷键。运行 `codex-tui doctor terminal`，确认 TERM、TTY、窗口调整和子进程退出。Stable 必须拥有真实控制 TTY 的 Focus/Resize/Ctrl+C/Restore 记录，托管 PTY 不代替。

PTY 子进程从创建、初始化到分离 actor 退出始终由终止与回收守卫持有。取得 reader/writer、发布 Ready 或启动读取线程失败，不能放弃回收责任。故障注入使用本地子进程覆盖各启动阶段，不代替真实 SSH 验收。

可捕获的 Unix 进程信号（SIGTERM、SIGHUP、SIGINT）进入交互程序的正常退出路径，先恢复终端 raw 模式、备用屏幕与光标，再执行原有的有界最终保存；保存失败仍返回错误。raw 模式 Ctrl+C 按键与 OS SIGINT 分别验收。SIGKILL 与已经断开的物理终端不属于应用可保证恢复的范围；无界面子命令不安装这些交互退出处理器。

## Registry 繁忙时的看板更新
<!-- docs-section: board -->

规划 worker 同时只计算一份元数据快照。计算期间仅有少量会话来源变化时，使用**当前**输入重新投影最多 64 张受影响卡片（包括成员增删与共享 worktree 冲突影响），再安装结果，不直接接受旧 generation。人工编辑、本地/Forge/Git/目标/后端权威变化或更大的差异仍要求重新捕获全量快照。这避免单条高频会话无限废弃其他有效更新，但不保证任意全局事件风暴下的绝对延迟。保留的 5 千条持续事件夹具与性能诊断属于合成证据，不代表人工效率或多小时真实使用已认证。

## SQLite 损坏或未来 Schema 拒绝
<!-- docs-section: store -->

使用 `codex-tui doctor store` 执行明确的 SQLite 完整性扫描；交互程序启动时也会进行一次检查，但常规读写不再每次打开数据库都重复全库 quick_check。之后停止所有进程并保留数据库/WAL/SHM 和恢复镜像。不要把正在写的数据库直接复制为一致性备份，或让多个进程同时改写。 TUI 运行期间保持跨进程 SQLite 独占写者锁；如有写者，离线恢复会被拒绝。打开 SQLite 前将拒绝已存在的数据库文件或写者锁文件符号链接、非普通文件；这是私有路径防护，不代表可原子阻止同一用户的恶意进程并发替换文件。v1.4 首次安装只使用当前 SQLite v4，拒绝未部署的 v1–v3 镜像且不自动清空。

## 提供最小脱敏证据
<!-- docs-section: artifacts -->

`codex-tui doctor bundle --output ./codex-tui-support` 生成脱敏元数据和摘要，分享前仍需检查内部主机名、个人路径、凭据、Prompt、原始日志和评论正文。提供最小复现命令、退出码、源码 SHA 和关联 CI Run ID，未获取的真实环境证据明确标为**尚未验证**。

