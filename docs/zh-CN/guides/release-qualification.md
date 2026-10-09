<!-- docs-id: release-qualification -->
<!-- docs-lang: zh-CN -->
# v1.4 发布门禁、真实验收与交接
<!-- docs-section: overview -->

**Language / 语言:** [English](../../guides/release-qualification.md) · [简体中文](release-qualification.md)

v1.4 已完成开发范围，**并非**已发布的 Stable，也不是完成真实部署的产品。公开 v1.0.0 只是历史发布记录，未被应用部署。本页是使用指南，不设立新发布权威；以 [正式资格 JSON](../../../release/v1.4-criteria.json) 和 [#211 交接单](https://github.com/jiying2007/codex-tui/issues/211) 为准。

## 机器自动化资格
<!-- docs-section: automated -->

同一**受保护 main 精确 SHA** 应按现行策略通过 Linux/macOS/Windows + Rust CI、Development Qualification、Security/Release Gate、10k/50k 规模、UI 诊断、PTY 和结构化 Soak。保留每个 Run ID 及 Artifact SHA-256。发行包还需原生元数据、ABI、许可证、Notices 与校验和。PR 通过不能替代 fresh-main。

## 真实环境必须单独验收
<!-- docs-section: external -->

必须独立采集并绑定当前源码：①真实登录 Codex App Server/Provider 能力；②内部使用档真实 `glab` 及 Issue/MR/Pipeline 可用；③Linux 和 Windows→SSH Ubuntu 的**控制终端** Focus/Resize/Ctrl+C/退出/恢复；④实际机器的性能、资源和异常恢复。托管 CI 不得伪造为真实 PASS。

## 管理员独立治理
<!-- docs-section: admin -->

Stable 前管理员须验证 `main` 严格 Required Checks（含 GitHub Actions App ID 绑定）、禁止 Force Push/删除、管理员同样受保护，以及 **GitHub Immutable Releases** 已开启。没有权限或读取失败就是阻塞，不能绕过；管理员令牌不得进入公共 PR 工作流。

## 精确候选 Stable 非发布演练
<!-- docs-section: dryrun -->

Stable `publish=false` 与普通 Preview 自检不是同一资格。必须绑定候选版本/Tag、Canonical CI、真实 Linux Doctor/TTY/性能及 `release-evidence/v5` 必须字段，才可宣布 Stable 演练有效。不能编造时间戳、掩盖摘要或复用旧候选证据。

## 正式发布必须授权
<!-- docs-section: publication -->

正式 `publish=true` 之前必须审阅发布说明/日期、不可变 Releases 和主线 HEAD 的再次读取、与演练完全一致的保留产物，以及**明确的所有者授权**。发行发布和远程资产校验属于独立敏感操作，任何文档或模拟 Fixture 不能绕过。

## 交接与历史记录
<!-- docs-section: archive -->

在真实/管理员证据全部取得之前，保持 [#211](https://github.com/jiying2007/codex-tui/issues/211) 为 Open。不要强删独立分叉的历史分支、重写 v1.0.0 GitHub 发布事实或把 v1.2/v1.3 完成记录当作实际部署后的升级链。本次为 **v1.4 首次安装**，不是历史数据迁移。

