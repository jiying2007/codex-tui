<!-- docs-id: home -->
<!-- docs-lang: zh-CN -->
# codex-tui — 多仓库 Codex 终端工作台
<!-- docs-section: overview -->

**语言 / Language:** [English](README.md) · [简体中文](README.zh-CN.md)

codex-tui 是面向个人开发者、可供团队复用的**本地优先 Mission Control**。它把 Codex 会话、Git 工作树和 GitLab/GitHub 交付状态投影到一个终端，不替代官方 Codex CLI、Git 或 Forge，也不是第二套 Agent 或任务服务。

**当前状态：** `v1.4.0` 是首次部署候选，**不是已发布的 v1.4 Stable**。历史 v1.0.0 已发布，但维护者确认 v1.0 和 v1.4 都未实际部署。`stableReady=false`、`publicationAllowed=false`。托管 CI 不能替代真实账号、控制终端或内部 GitLab 资格。

从 [安装说明](docs/zh-CN/release/install-upgrade.md)、[团队快速入门](docs/zh-CN/team-quickstart.md) 或 [文档导航](docs/README.zh-CN.md) 开始。历史计划、研究和证据仍保留在原路径；本页只介绍当前工作流程，不再作为历代里程碑清单。

## 开发阶段与发布状态
<!-- docs-section: status -->

v1.4 冻结新增核心功能，继续接受缺陷、安全、互操作、证据和维护修复。v1.2/v1.3 计划是历史资料，不是已部署版本的迁移义务。首次安装直接创建 SQLite v4，不导入 `state-v1.json`，不迁移未部署的 SQLite v1–v3。参见 [首次部署决策](docs/zh-CN/implementation/v1.4-first-deployment-baseline.md)。

受保护主线 CI 和预览产物仅建立自动化工程基线。真实环境验收及发布授权继续由 [#211](https://github.com/jiying2007/codex-tui/issues/211) 跟踪。

## 核心价值与每日工作
<!-- docs-section: product -->

**找到需要处理的事项 → 打开正确上下文 → 明确操作 → 核对结果 → 返回下一项。**

| 任务 | 操作入口 |
|---|---|
| 找到下一条待处理会话 | `Space`；`/` 搜索；`j/k` 或方向键选择；`Enter` 打开 |
| 返回常用工作 | 选中任务后按 `=`、`1–9` 绑定；数字直达，`Esc` 返回一级 |
| 查看交付上下文 | `b` 看板，`Tab` 切换视图，`r` Review，`w` Workspace |
| 查找命令和帮助 | `Ctrl+K` 命令面板，`?` 帮助；方向键、`PgUp/PgDn`、`Home/End` 浏览帮助 |
| 打开最近观测事件 | 提醒可见、未过期且关联 Codex 会话时按 `!` |
| 恢复失败的首次连接 | 出现失败提示后，在空任务中心关闭弹层，单次按下无修饰的 `F5` |

任务中心按待处理、置顶、最近活动排序，刷新保留选中身份；最近窗口有界，置顶及待处理例外仍可达。看板 List/Review Queue 按工作区分组。笔记、别名、置顶、保存视图和个人规划覆盖层保存在本地，WorkCard 仍从 Codex/Git/Forge 派生。Forge 写操作必须计划、明确确认、执行并取得可信回执。

**空闲不是成功。** 执行中或等待输入的会话变为 Ready，只提示已空闲、需要核对结果，不推断任务通过。明确的 Goal 完成观测单独提示，但也不等于交付验收通过。系统错误、用量／预算限制、审批、输入及支持的交付事件只依据已观测状态，不虚构上游结果。

**静音不等于不可见。** 默认通知模式 `off` 仅关闭外部投递，仍保留界面内最近事件和关联会话回跳。单条提醒 20 秒后过期，不持久化，不抢占编辑、弹层或终端焦点；它不是通知历史或必达承诺。

`F5` 仅重试同一首次连接目标，最多一个请求在途。一旦安装过 Registry，该入口在本进程中永久关闭，后续断开也不会重新开启；提示词、审批、队列和 Forge 写操作不会重放。无效配置仍需在启动前修正。见 [首次连接重试](docs/implementation/startup-retry.md)。

上下文键位以程序帮助及失败提示为准。当前没有鼠标点击／滚轮导航，不捕获鼠标，旧 `[ui].mouse` 只解析不生效。完整行为与限制见 [操作指南](docs/zh-CN/guides/operator-guide.md) 和 [CLI 参考](docs/zh-CN/guides/cli-reference.md)。

## 架构和权威边界
<!-- docs-section: architecture -->

~~~text
Codex App Server：会话 / Agent / Queue 权威
        |
        +-- Registry / Attention / Mission Control
        +-- 派生 Board / WorkCard
        +-- Git / 工作树与 GitLab / GitHub 投影
        +-- SQLite：仅个人界面 / 规划元数据
        |
        +-- Ratatui 终端界面
~~~

不引入云服务、共享任务数据库、RBAC 或独立 Forge／终端后端。Thread View、Thread Queue 和 Terminal Drawer 是仅维护的上游重叠界面。见 [上游收敛决策](docs/zh-CN/adr/012-upstream-convergence.md)。

## 安装、构建与运行
<!-- docs-section: run -->

构建需要 **Rust 1.88+** 和 Git。真实会话需要获得授权的 Codex 安装；对应 Forge 需要认证的 `glab` 或 `gh`，不用该 Forge 时不要求安装对应客户端。安装前核验可信归档，见 [安装与验包](docs/zh-CN/release/install-upgrade.md)。

~~~bash
git clone https://github.com/jiying2007/codex-tui.git
cd codex-tui
cargo build --release --locked
./target/release/codex-tui --version
./target/release/codex-tui --help
~~~

仅采集本地元数据，不启动外部进程或连接目标：

~~~bash
./target/release/codex-tui doctor bundle --offline --output ./codex-tui-support
~~~

离线诊断仍可能创建本地 SQLite 和输出目录，不是零写入模式，也不构成真实兼容性资格。只有真实后端使用获准时，才运行 `doctor compat`、`doctor codex` 和交互程序；假数据与规模夹具不能证明真实账号通过。

**Windows SSH Ubuntu** 应在 Ubuntu 运行 Linux 程序；文件系统、Codex、配置及子进程属于 Ubuntu，不是 Windows 桌面。`t` 打开终端抽屉；终端支持时以 `F6` 或 `Ctrl+]` 释放焦点。SSH 下未显式配置 `CODEX_TUI_EDITOR`／`CODEX_TUI_BROWSER` 适配器时，默认图形程序会在创建进程前被拒绝。适配器仍运行在 TUI 主机，不提供客户端桌面桥接或应用最终打开成功的回执。

## 语言、配置、通知
<!-- docs-section: language -->

在允许执行该诊断时，按 `codex-tui doctor` 打印的路径编辑本地配置：

~~~toml
[ui]
language = "auto"      # auto | en | zh-CN
mouse = false          # 可解析，但没有鼠标导航
presentation = "normal" # normal | quiet | screen-reader

[notifications]
mode = "off"           # 外部投递：off | terminal | os；视觉提醒仍保留

[app_server]
active = "local"
~~~

自动语言依次读取 `LC_ALL`、`LC_MESSAGES`、`LANG`；`zh_CN`、`zh_SG`、`zh-Hans` 等简体环境选中文，繁体及未知环境回退英文。技术标识与上游错误保留诊断含义。Quiet／screen-reader 只合并后台重绘，不延后直接输入；模式名称不代表完整读屏器资格，也不证明资源开销降低。

将模式设为 `terminal` 才启用 BEL 投递。SSH 中 `os` 回退 BEL，**不是 Windows Toast**。Windows Terminal SSH Profile 可设置 `"bellStyle": "all"`，从 Ubuntu 用 `printf '\a'` 验证实际路径。启用的外部投递按 75 毫秒窗口合并事件，队列有界并明确报告溢出；响铃或系统投递失败不抑制已经选出的视觉提醒。退出或 SSH 断开后不继续监听，首次观察到的历史状态不会作为新事件重放。

命名的 stdio／WebSocket／Unix Socket 目标使用本地 TOML；令牌只通过 `auth_token_env` 引用，不提交密钥。见 [远程目标设计](docs/implementation/v1.3-remote-app-server-targets.md)。Headless Codex 视图每次读取一个活动／命名目标，本地元数据仍共享，跨目标相同的裸会话 ID 可能共享覆盖层；不能把它当作跨目标数据隔离，详见 CLI 参考。

## 明确不做的功能
<!-- docs-section: boundaries -->

不建设第二套 Agent 编排、Agent 通信、会话／队列权威、通用插件平台、tmux 替代品、团队数据库、通用跨 Agent API、Web 服务或财务产品。优先删除、精简或投影上游重叠能力，不重复建设；已有重叠界面只进行缺陷、兼容和安全维护。

## 验收、故障排查与发布资格
<!-- docs-section: qualification -->

从可见错误和 [排障指南](docs/zh-CN/guides/troubleshooting.md) 开始。在对应系统探测获准时，分别检查兼容性、Codex、Git、代表仓库中的 Forge、本地存储与终端。支持包排除令牌、提示词、对话及原始探测错误，但仍需隐私复核；真实任务和内部证据保持私有。

[使用体验验收](docs/zh-CN/guides/ux-qualification.md) 对比现有工具链与工作台的真实任务耗时、错误跳转、上手及提醒回跳。文档目标**不是已测收益**。托管合成计时、五分钟结构 Soak 不等于 SSH 延迟、多小时现场使用、Token 节省或完整可访问性评估。

内部 GitLab 需要认证的 [平台能力证据](docs/zh-CN/qualification/provider.md)。Stable 仍要求精确来源的 Linux 兼容／TTY／性能证据、严格主线保护、不可变 Releases、发布说明审阅、独立检查和明确授权。自动 Preview 自检使用 `publish=false`，不能替代另行执行的 Stable 演练。见 [发布资格](docs/zh-CN/guides/release-qualification.md)。

## 许可证与参与贡献
<!-- docs-section: license -->

采用 **Apache-2.0**，见 [LICENSE](LICENSE)。先阅读 [贡献指南](CONTRIBUTING.zh-CN.md)、[安全说明](SECURITY.zh-CN.md) 和 [支持说明](SUPPORT.zh-CN.md)。代码及文档修改均经过受保护 PR 和必需门禁；不改写或删除历史 Release、Tag 和证据。[文档导航](docs/README.zh-CN.md) 保留详细当前指南及历史设计入口。
