<!-- docs-id: support -->
<!-- docs-lang: zh-CN -->
# 支持说明、诊断与已知边界
<!-- docs-section: overview -->

**Language / 语言:** [English](SUPPORT.md) · [简体中文](SUPPORT.zh-CN.md)

codex-tui 是本地优先开发工具，公开支持以 Issue 为主，不承诺固定 SLA。公开问题不适合粘贴凭据、内部仓库路径或未披露漏洞；敏感事项见 [安全政策](SECURITY.zh-CN.md)。

## 先执行的检查
<!-- docs-section: triage -->

~~~bash
codex-tui --version
codex-tui doctor compat
codex-tui doctor codex
codex-tui doctor store
codex-tui doctor terminal
~~~

在**目标仓库目录**中执行 `codex-tui doctor forge`，并确认该服务器的 `gh/glab` 已认证。通用 `doctor compat` 即使 Ready 也不表示内部 GitLab 已可用；内部部署需额外 [能力资格](docs/zh-CN/qualification/provider.md)。

## 如何提交有效 Issue
<!-- docs-section: issue -->

使用 [中英 Bug 表单](.github/ISSUE_TEMPLATE/bug_report.yml)，注明版本/源码 SHA、系统和终端（含 Windows→SSH Ubuntu、TERM）、Codex 版本与目标类型、真实/`--fake` 场景、期望与实际表现、脱敏的复现命令以及相关 CI Run ID。`codex-tui doctor bundle --output ./codex-tui-support` 生成的材料也应先自行复核隐私；模拟 CI 不能替代真实环境 PASS。

## 限制与处理边界
<!-- docs-section: limits -->

v1.4 Stable 尚未发布，v1.0/v1.4 也无实际部署记录。内部 GitLab 认证与能力、真实 Codex 账号、SSH 控制终端、管理员发布设置和最终授权属于外部资格。遇到 Degraded/Blocked 不应自动删除 SQLite；采取破坏性措施前先保留原始状态并参考 [排障指南](docs/zh-CN/guides/troubleshooting.md)。

