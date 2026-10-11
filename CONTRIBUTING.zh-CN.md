<!-- docs-id: contributing -->
<!-- docs-lang: zh-CN -->
# 贡献指南 — codex-tui
<!-- docs-section: overview -->

**Language / 语言:** [English](CONTRIBUTING.md) · [简体中文](CONTRIBUTING.zh-CN.md)

欢迎参与个人优先、Linux Tier 1 的 codex-tui。请先阅读 [中文文档目录](docs/README.zh-CN.md)。仓库采用受保护 PR 流程，任何贡献不等于授权正式 Stable 发布。

## 架构与功能范围
<!-- docs-section: scope -->

Codex 拥有对话、Agent/Queue；Git 拥有工作树；GitLab/GitHub 拥有交付；codex-tui 只管理本地投影和个人元数据。遵守 [ADR-012 中译](docs/zh-CN/adr/012-upstream-convergence.md) 和 `release/v1.5-convergence.json`；不得添加第二套 Agent、远程服务、任务数据库或终端管理平台。旧会话/Queue/Drawer 模块以维护为主。`src/app.rs` 负责 AppState 投影和辅助逻辑，`src/app/reducer.rs` 是唯一的 `app::reduce` Action→Effect 状态转换入口（原公开路径保持不变）。重构必须保留用户意图顺序、所有副作用回执和单一 v1.4 LOC Ratchet，不得建立第二套状态权威。

## 本地开发与测试
<!-- docs-section: setup -->

使用 Rust 1.88+、已提交的 `Cargo.lock`；发布脚本兼容 Python 3.8+。真实 Codex/GitLab/GitHub 验证需在对应授权环境运行。只通过特性分支提交，不覆盖已有 Release/Tag 或主线保护。

~~~bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
python scripts/docs/check_docs.py
python -m unittest discover -s scripts/release -p "test_*.py"
~~~

本地测试与托管 CI 的结论必须区分。

## 分支、提交与 PR
<!-- docs-section: branch -->

使用小范围特性分支，推荐 Conventional Commit 前缀：`fix:`、`docs:`、`refactor:`、`test:`、`ci:`，提交说明可中英双语。PR 需写清可观察问题、修改边界、安全影响、变更文件、可重复验证、单一 `release/v1.4-plan.json` LOC Ratchet 和未取得的人工证据。检查失败不得强行合并；历史分叉分支必须经过完整证明才能删除。

## 自动化与精确 SHA
<!-- docs-section: gates -->

Canonical CI 包含 Linux/macOS/Windows、Rust MSRV、`cargo fmt`/`clippy`/`test` 和 Python 治理检查。Development Qualification 与 Release Gate 不是彼此替代。托管 PTY、10k/50k 性能和长稳属于合成/托管证据；真实 Codex 账号、内部 GitLab、SSH 控制终端、管理员保护和 Stable `publish=false` 必须单独验收。

## 中英文同步与文件卫生
<!-- docs-section: docs -->

命令、配置、安全或发布边界改变时，同 PR 同步 `docs/i18n/manifest.json` 中的英文/中文活跃文档，运行 `python scripts/docs/check_docs.py`。本地链接必须有效，命令原样保留；历史英文研究/方案不自动批量翻译。使用 UTF-8/LF，不提交 SQLite、令牌、Prompt、任意生成日志或不明许可证文件；见 `.editorconfig` / `.gitattributes`。

## 安全与隐私
<!-- docs-section: security -->

Issues、测试夹具和产物不得含真实令牌、内部仓库路径、原始 CLI 错误、对话正文或生产终端转录。使用隔离的合成测试和脱敏负例；敏感漏洞按 [安全政策](SECURITY.zh-CN.md) 私下报告。

## 发布与权威
<!-- docs-section: release -->

贡献流程不触发 Stable 正式发布。v1.4 为未部署版本的首次交付基线，不升级历史 v1.0 JSON 或 SQLite v1–v3。正式发布需精确主线 CI、不可变 Release、真实环境回执和明确授权，详见 [发布门禁](docs/zh-CN/guides/release-qualification.md)。PR 全绿不等于 Stable Ready。

