<!-- docs-id: troubleshooting -->
<!-- docs-lang: zh-CN -->
# 故障排查：Codex、Forge、SSH 与 SQLite
<!-- docs-section: overview -->

**Language / 语言:** [English](../../guides/troubleshooting.md) · [简体中文](troubleshooting.md)

先确认失败的是哪个权威，不要靠删除本地状态、改写上游凭据或伪造恢复成功。记录精确源码 SHA、目标类型和脱敏错误类别。敏感漏洞按 [安全披露](../../SECURITY.zh-CN.md) 处理。

## App Server 连接失败或线程缺失
<!-- docs-section: codex -->

执行 `codex-tui doctor codex`（命名目标可附 `--target NAME`），独立检查真实 `codex --version` 和账号登录。远程 WebSocket/Unix 建连及握手受超时限制，超时不代表已发出的请求必定未执行。核对服务进程、TLS 代理、认证及能力错误；可选方法可以降级，但不会自动切换 `--fake`。Issue 不得粘贴 Bearer Token。

## GitLab/GitHub 不可用或字段不全
<!-- docs-section: forge -->

进入出现问题的仓库，**私下**检查 `git remote -v`，依次执行 `codex-tui doctor git`、`codex-tui doctor forge`。在工具外登录对应主机的 `glab/gh`。GitLab Issue/MR/Pipeline 独立探测，一项能力失败不应让其他成功内容消失。`doctor compat` Ready 不等于内部 GitLab PASS；内部部署参考 [能力准入](../qualification/provider.md)。

## Windows 路径拼接或仓库会话不匹配
<!-- docs-section: cwd -->

Windows→SSH Ubuntu 使用的是 **Ubuntu 侧** Codex Home、当前工作目录及 Git 身份。历史会话包含 Windows cwd 时属于外来源，不是本地 Ubuntu Checkout。核对 `doctor codex` 的路径和实际仓库目录的 `doctor git`；不要拼接 Windows 绝对路径，也不要编造不存在的 Ubuntu 本地会话。

## Drawer 聚焦、Ctrl+C、窗口重绘异常
<!-- docs-section: terminal -->

使用应用内 `?` 查看实际键位：`t` 打开 Drawer，`F6`（或支持时 `Ctrl+]`）返回聚焦。远程终端或 SSH 客户端可能截获快捷键。运行 `codex-tui doctor terminal`，确认 TERM、TTY、窗口调整和子进程退出。Stable 必须拥有真实控制 TTY 的 Focus/Resize/Ctrl+C/Restore 记录，托管 PTY 不代替。

## SQLite 损坏或未来 Schema 拒绝
<!-- docs-section: store -->

先运行 `codex-tui doctor store`，停止所有进程并保留数据库/WAL/SHM 和恢复镜像。不要把正在写的数据库直接复制为一致性备份，或让多个进程同时改写。v1.4 首次安装只使用当前 SQLite v4，拒绝未部署的 v1–v3 镜像且不自动清空。

## 提供最小脱敏证据
<!-- docs-section: artifacts -->

`codex-tui doctor bundle --output ./codex-tui-support` 生成脱敏元数据和摘要，分享前仍需检查内部主机名、个人路径、凭据、Prompt、原始日志和评论正文。提供最小复现命令、退出码、源码 SHA 和关联 CI Run ID，未获取的真实环境证据明确标为**尚未验证**。

