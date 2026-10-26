<!-- docs-id: cli -->
<!-- docs-lang: zh-CN -->
# CLI 速查（v1.4 开发主线）
<!-- docs-section: overview -->

**Language / 语言:** [English](../../guides/cli-reference.md) · [简体中文](cli-reference.md)

`codex-tui --help` 和各子命令的 `--help` 是权威接口。以下示例只说明命令可用，不证明真实账号已登录或 Stable 已部署。退出码：`0` 成功、`1` 非预期错误、`2` 参数错误、`3` 降级/阻塞。

## 交互工作台
<!-- docs-section: interactive -->

~~~bash
codex-tui
codex-tui --target local-alt
codex-tui --fake
codex-tui --version
codex-tui --help
~~~

无参数启动 Mission Control；命名目标需先在本地配置声明。`--fake` 不能与 `--target` 组合，不会自动代替失败的真实会话。按 `?` 查看应用内键位。

## Doctor 与安全诊断
<!-- docs-section: doctor -->

~~~bash
codex-tui doctor compat --json
codex-tui doctor codex --target local-alt
codex-tui doctor git
codex-tui doctor forge
codex-tui doctor store
codex-tui doctor presets
codex-tui doctor terminal
codex-tui doctor bundle --offline --output ./codex-tui-support
~~~

`doctor compat` 不访问远程 Forge，可能在 `glab` 缺失时仍显示 Ready；`doctor forge` 应在目标仓库执行。诊断包只保留脱敏元数据，但分享前仍需复核。

`doctor bundle --offline` 仅采集构建身份与本地 SQLite 元数据，不启动外部程序，不连接 App Server／Forge 目标。外部组件明确记为 unavailable／not-probed，readiness 保持 Blocked，且 `realCodexQualification=false`。这只是本地支持／脱敏证据，不是实服或设备资格。自动开发与发行包采集使用此模式并校验标记，原有资格门禁全部保留。不加 `--offline` 时仍执行在线探测，可能启动 Codex App Server。分享前仍需检查包内容。

## 只读无头命令
<!-- docs-section: headless -->

~~~bash
codex-tui headless threads --json
codex-tui headless threads --target local-alt --json
codex-tui headless work --json
codex-tui headless status --json
codex-tui headless attention --json
codex-tui headless board --json
codex-tui headless forge --json
codex-tui headless worktrees --json
codex-tui status
codex-tui status --target local-alt --json
codex-tui thread list
codex-tui attention list
codex-tui board list
codex-tui forge status
codex-tui worktree list
~~~

`headless threads --fixture-10k` 只是合成规模数据，没有无头写操作接口，不可将它视为真实 Forge/账号验收。

`threads`、`work`、`status`、`attention`、`board` 及其顶层别名，默认解析 `[app_server].active`，也可用 `--target NAME` 覆盖。每次调用只读取一个目标。未知目标、配置错误或连接失败会明确降级并返回 `3`，不会悄悄回退到本机或测试数据。配置不存在时仅使用内存默认值，不创建配置文件。缺失/重复的目标参数、`--target` 与 `--fake`/`--fixture-10k` 混用，以及对 `forge`/`worktrees` 使用 `--target`，均返回语法错误 `2`。

| 输出字段 | 范围 |
| --- | --- |
| Codex 视图的 `source.targetId`、`source.transport`、`source.scope` | 所选目标和传输；真实读取为 `single-target`，合成数据为 `fixture` |
| `work`/`status`/`attention`/`board` 的 `localStoreScope` | 本地元数据为 `shared-across-targets`，目标解析失败且尚未加载时为 `not-loaded`，测试数据为 `fixture` |
| Forge 的 `scope` | `current-directory` 或 `fixture` |
| Worktrees 的 `scope` | `shared-local-cache` 或 `fixture` |

本地别名、置顶与规划元数据仍以不含目标的会话 ID 为键；两个目标使用相同 ID 时，可能共享这些元数据，切换目标不会隔离它们。`threads` 只报告源清单，不应用这些覆盖层。无效目标在读取本地存储前失败；成功读取仍可能触发已有 SQLite 初始化/WAL 行为。“只读”指没有上游变更接口，不承诺本地文件系统零写入。纯文本同样显示来源、范围与明确的失败原因。

## 开发诊断、基准与长稳
<!-- docs-section: diagnostics -->

~~~bash
codex-tui release verify --channel preview --tag v1.4.0-preview.1 --commit SOURCE_SHA --json
codex-tui release benchmark --iterations 200 --json
codex-tui release render-benchmark --iterations 200 --json
codex-tui release interaction-benchmark --iterations 200 --json
codex-tui release scale --rows 50000 --warmup 5 --iterations 50 --json
codex-tui release failure-matrix --json
codex-tui soak --rows 50000 --cycles 256 --duration-seconds 300 --json
~~~

`SOURCE_SHA` 应替换为真实 40 位源码 SHA。这些仅用于诊断/验证，不能授予 Stable 发布权限；CLI 对规模参数另有上限。

## 退出码与失败语义
<!-- docs-section: exit -->

`2` 表示命令/参数错误，`3` 表示请求的 Doctor/无头能力降级或阻塞，`1` 是非预期运行错误。排查前保留相关数据；对外部写操作结果不确定的情况不得盲目重试。出现内部 URL 时只报告脱敏原因，不贴原始 CLI stderr。

## 命令权威与稳定发布
<!-- docs-section: authority -->

CLI 帮助不改变 Codex App Server、Git 或 Forge 的真实权威。首次部署前阅读 [数据基线](../implementation/v1.4-first-deployment-baseline.md) 和 [发布资格](release-qualification.md)。v1.4 当前不是已公开 Stable 渠道。

