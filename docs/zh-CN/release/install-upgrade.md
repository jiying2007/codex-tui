<!-- docs-id: install-upgrade -->
<!-- docs-lang: zh-CN -->
# 安装、验包、首次部署与升级边界
<!-- docs-section: overview -->

**语言 / Language:** [English](../../release/install-upgrade.md) · [简体中文](install-upgrade.md)

本说明对应 codex-tui 的原生归档包安装，不提供后台安装服务或自动更新器。`v1.4.0` 当前是首次部署候选，不是已发布的 v1.4 Stable。历史 v1.0.0 GitHub Release 已公开，但尚未实际应用。

## 校验 Release Bundle
<!-- docs-section: verify -->

可信 Bundle 应包含 Linux/macOS/Windows 的原生归档、`SHA256SUMS`、`RELEASE_NOTES.md`、`release-verification.json` 和对应 `STABLE-CRITERIA.json`。稳定渠道还需要有效的真实环境证据。安装前确认目标 Commit/Tag 与 Bundle 来源，校验要安装的归档校验和后再解压，切勿执行来历不明的二进制。

每个平台包包含二进制、README、CHANGELOG、INSTALL-UPGRADE、TEAM-QUICKSTART、Apache LICENSE、第三方 Notices 和 Release Metadata。**v1.4 首次部署候选包**还必须保留并校验英文/简体中文版 README、安装、快速入门（含根目录 `README.zh-CN.md`、`INSTALL-UPGRADE.zh-CN.md`、`TEAM-QUICKSTART.zh-CN.md`），以及 `docs/i18n/manifest.json` 指定的所有当前双语操作/治理页面（以原仓库相对路径存放）。缺任何配对页面、章节或语言声明将拒绝归档；历史 v1.0 包的原始英文布局不会被追溯修改。详见英文 [归档约定](../../release/install-upgrade.md)。

随包的**当前中英文指南**可离线浏览；若链接指向未打包的历史设计、
实现或源码，打包器会把该相对链接转换为绑定**精确源码 SHA** 的
GitHub 只读地址。历史链接需要联网，但当前双语指南和语言切换不需要。
归档校验会拒绝残留的无效本地 Markdown 链接，避免安装包内出现死链。

## 二进制、来源与归档身份
<!-- docs-section: identity -->

`RELEASE-METADATA.json` 使用 `codex-tui/release-artifact/v2`，绑定 `binarySha256`、40 位源码 SHA、平台/Host Triple 与 Rust/Cargo 构建工具身份。验证器拒绝重复、大小写冲突、越界、链接及特殊类型文件；成员和解压总大小受限制。

编译工具及镜像环境并非字节级完全可复现，元数据只证明被验证产物的身份。不要根据文件名或日志推测通过。

## Linux ABI 范围
<!-- docs-section: abi -->

x86-64 GNU Linux 发行包在 Ubuntu 20.04 / glibc **2.31** 基线构建及校验。该数值是 ABI 下限，不意味着推荐继续使用未维护的操作系统；应采用有安全维护支持的发行版。Musl/Alpine 不是这个 GNU 构建的承诺目标，Python3.8 脚本与 Git 工具兼容分别检查。

## 首次安装步骤
<!-- docs-section: install -->

1. 获取与当前平台匹配、来自受信构建的压缩包。
2. 对照 `SHA256SUMS` 校验；解压并把 `codex-tui`（Windows 是 `codex-tui.exe`）放入 PATH。
3. 安装/确认 `git`、真实场景 `codex`；需要 Forge 时另配置 `glab` 或 `gh`。
4. 依次执行：

~~~bash
codex-tui --version
codex-tui doctor compat
codex-tui doctor compat --json
codex-tui doctor codex
codex-tui doctor terminal
~~~

进入有 Forge Remote 的仓库后运行 `codex-tui doctor forge`。无需 Nerd Font。

## v1.4 是首次实际部署
<!-- docs-section: first-deploy -->

v1.0–v1.3 没有实际部署历史，因此**不支持从这些版本就地升级**。使用新建的应用状态目录和 TOML 配置；SQLite 从 v4 直接初始化。历史 `state-v1.json` 不导入也不删除，未部署的 SQLite v1–v3 以及未知未来 schema 会被安全拒绝，不会秘密改写为可用状态。保存旧文件做审计，不能把拒绝当作“无需数据风险”。

初次部署仍须真实 Codex、内部 GitLab、Linux 控制终端和管理员发布治理验收；自动化托管测试不等价于真实场景。

## 未来已部署版本升级（非 v1.0–v1.3）
<!-- docs-section: upgrade -->

只有**未来真正部署过 v1.4 或更高版本**，且经过相应存储兼容性与回滚审核时，才可采用以下流程：停止运行中的 TUI 和 Drawer 子进程 → 以 SQLite 在线备份方式保留有效状态 → 更换相应二进制 → 运行 `doctor store`、`doctor compat` → 验证 Registry/仓库 → 最后再清理旧二进制。不得多个进程同时搬移或复制仍处于 WAL 写入中的库。

## 失败时回滚
<!-- docs-section: rollback -->

新版本诊断失败：先停进程，保留当前数据库/WAL/SHM 与诊断包；恢复上一安全二进制并比较来源/元数据。不要自动删除状态，也不能把失败后运行了旧程序当成一次“成功升级”。

## Preview 与 Stable 边界
<!-- docs-section: preview -->

预览 Tag：`vX.Y.Z-preview.N`；Stable Tag：`vX.Y.Z`。当前 v1.4 的自动化运行只是 `publish=false` 的非发布资格演练。

Stable 需要精确 SHA CI、Linux 真正 Doctor/TTY/性能证据、不可变 GitHub Releases 和具有 GitHub Actions App 身份绑定的严格主线保护、管理员审阅及明确发布授权。必须先通过同一候选的 Stable `publish=false` 验收，之后才可以由授权者触发正式发布。任何缺少的授权均 fail-closed。

## 开源许可
<!-- docs-section: license -->

`Cargo.toml` 中 `license = "Apache-2.0"` 与根目录 `LICENSE` 均为发布输入。归档校验会验证许可证和第三方 notices，不允许因为文档双语化而删除。

## SQLite 一致性备份与恢复
<!-- docs-section: backup -->

SQLite 使用在线备份 API 包括已提交 WAL 内容，备份目的地经过暂存、完整性验证、同步后发布；已有文件不会被覆盖。恢复是**离线操作**：先停止所有 codex-tui 进程，保留库及 WAL/SHM 原始镜像，再按程序校验恢复。不能使用复制仍打开的主数据库文件作为可靠快照。

