import json
from pathlib import Path
from lean_patch_support import replace, write

sections = ['overview', 'decision', 'implementation', 'qualification', 'sources']
write('docs/implementation/lean-mission-control.md', '''<!-- docs-id: lean-mission-control -->
<!-- docs-lang: en -->
# Lean Mission Control: final product and implementation plan
<!-- docs-section: overview -->

**Language / 语言:** [English](lean-mission-control.md) · [简体中文](../zh-CN/implementation/lean-mission-control.md)

Decision date: 2026-10-21. Entry commit: `d74020ecd69b19ed83d2342b3d99497b800e68d0`.
The user authorized this new development scope after a product-value review.
Package version remains 1.4.0; the v1.5 theme is not a Stable release claim.
The current product is a personal-first, local engineering control plane: reduce
finding, switching and repeated verification across Codex, Git and the forge.

<!-- docs-section: decision -->
## Final decisions

Keep cross-repository attention, project grouping, delivery links, explicit source
freshness, local notes/pins/views and bounded diagnostics. Reuse existing Registry,
Board/List and Review rather than adding another task authority or UI state machine.
Codex owns conversations, permissions, agents and queue semantics. Git owns worktrees;
the forge owns review and delivery. Conversation, Queue and Terminal Drawer remain
advanced maintenance surfaces, not the default product pitch. Do not delete a user's
working path before a tested upstream handoff can replace it.

Named targets mean one selected App Server per invocation, not concurrent federation.
This scope fixes headless target selection and truthful partial coverage. Concurrent
multi-target aggregation, automatic risk-taking approval, automatic retry of unknown
writes, a team service and a generic agent framework are explicitly excluded.
No new cloud dependency, paid service, background daemon or persistent authority.

<!-- docs-section: implementation -->
## Six implementation packages

| Package | Final behavior | Acceptance |
|---|---|---|
| L1 Attention | Recent Registry prioritizes interaction, observed blockers, review, unread, pins, active work, then recent history; active/attention items survive the recent window. Explicit search/all-history retains source order. | Exact selected IDs remain unchanged during reordering; snooze and acknowledgements have explicit semantics; narrow terminals show attention text without relying on color. |
| L2 Daily navigation | F1 Today, F2 Projects, F3 Review queue and contextual palette reuse existing views; `o` on a thread opens its known delivery URL, otherwise explains missing data. | No new task database, no write on navigation, scoped identities and URL validation preserved; existing hot slots and F6 unchanged. |
| L3 Scoped headless | Registry-derived snapshots use configured active target or `--target NAME`; unknown targets never fall back. Structured coverage marks Git/Goal/Forge not-collected. Anchor index replaces per-thread local-card scans. | Pure preflight and actual-binary negative cases; no claim that an empty scoped attention list proves all work healthy. Local-only forge/worktree commands reject target overrides. |
| L4 List viewport | Scan lightweight grouping metadata and format only visible rows; truncate a list row to one terminal row so selection/scroll coordinates agree. | Group headers, Unicode, empty view, zero height and 10k tail reachability; no claim that sorting or grouping becomes constant-time. |
| L5 Sustainable maintenance | Short daily help and paired current guide; separate obsolete implementation narratives from current behavior. Narrow non-required performance diagnostics to runtime/policy changes. | The five required checks always trigger; release checks and exact-source evidence stay intact; no old SHA PASS reused for a new candidate. |
| L6 Value measurement | Offline paired workflow measurement and strict validation; no transcript, token, repository path or prompt collection. | Report missing evidence honestly, retain failures, compare matched tasks and counterbalance order. Simulated data cannot grant real qualification or publication. |

The only numeric module ceiling authority remains `release/v1.4-plan.json`.
Touched files and new focused modules receive explicitly reviewed ceilings under
this new plan; unrelated ceilings do not move. Do not treat test count or LOC as
proof of usability, nor a lower LOC count as a reason to remove safety checks.

<!-- docs-section: qualification -->
## Acceptance and stop conditions

Compare with the tools already used: official Codex plus Git/gh/glab and the user's
terminal/editor. Measure finding a blocking task, locating its delivery evidence,
and resuming after interruption. Use anonymous task/operator IDs, pinned candidate
SHA and baseline version, paired observations, both execution orders, at least
12 pairs and at least four pairs per task. Report time, operations, switches,
completion and errors separately. Proposed decision targets are a 30% median
reduction in task time or context switches without more errors or fewer completions;
these are targets, not achieved results. The tool only summarizes operator-supplied
observations and is not an independent certifier.

No captured real data means NOT_MEASURED. Current hosted tests do not grant actual
Codex, internal GitLab, Windows-to-SSH, controlling-TTY or multi-hour qualification.
Issue #211 remains authoritative for external release prerequisites. A successful
Preview build is not a Stable dry-run. Stable readiness/publication remain false
until their existing gates and explicit approval are satisfied.

Keep full native, security and affected protocol/terminal regression on the candidate
and main. Periodic scale/soak retain diagnostic value. Only narrow a non-required
workload when trigger coverage is tested; do not remove post-merge or publication
safety merely to reduce CI duration. Measured CI savings and actual user ROI remain
unknown until observed. Freeze expansion after these packages; add federation or
remove compatibility surfaces only when actual usage justifies the change.

<!-- docs-section: sources -->
## Primary-source research used for the decisions

Reviewed 2026-10-21. Live upstream release lookup returned Codex 0.163.0, published
2026-10-15; feature availability still depends on the connected server/account.

- [Codex official CLI documentation](https://developers.openai.com/codex/cli/features) and [App Server contract](https://developers.openai.com/codex/app-server): reuse upstream conversation/permission semantics.
- [Codex 0.163.0 release](https://github.com/openai/codex/releases/tag/rust-v0.163.0) and [0.162.0 release](https://github.com/openai/codex/releases/tag/rust-v0.162.0): ongoing configuration, task and worktree improvements make a duplicate CLI a poor investment.
- [GitLab CLI](https://docs.gitlab.com/cli/): preserve authenticated self-managed forge integration rather than adding another transport without measurements.
- [Ratatui widgets](https://docs.rs/ratatui/latest/ratatui/widgets/index.html): keep view-local layout and explicit viewport state.
- [Required-check troubleshooting](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks): workflow-level path filtering must not leave required checks pending.
- [Microsoft Research: The SPACE of AI](https://www.microsoft.com/en-us/research/publication/the-space-of-ai-real-world-lessons-on-ais-impact-on-developers/): evaluate multiple workflow dimensions rather than inferring human productivity from code or test volume.

Third-party marketing, repository stars and synthetic latency are not product-value
proof. The previous review's lazygit.dev comparison is not treated as official
project evidence; no copied code, dependency or license change comes from it.
''')
write('docs/zh-CN/implementation/lean-mission-control.md', '''<!-- docs-id: lean-mission-control -->
<!-- docs-lang: zh-CN -->
# 精简任务中心：最终产品方案与实施边界
<!-- docs-section: overview -->

**Language / 语言:** [English](../../implementation/lean-mission-control.md) · [简体中文](lean-mission-control.md)

决策日期：2026-10-21。基线：`d74020ecd69b19ed83d2342b3d99497b800e68d0`。
本轮由用户在产品价值审查后明确授权新开发范围。包版本暂保持 1.4.0，
v1.5 是开发主题，不代表稳定版本已经发布。产品定位是个人优先、本地运行的
研发任务与交付控制台，减少查找、切换和重复核实，而不是增加另一套 Codex。

<!-- docs-section: decision -->
## 最终取舍

重点保留跨仓待处理、项目聚合、交付直达、来源时效、本地备注/固定/视图和有界诊断。
复用 Registry、Board/List 和 Review，不建立第二套任务库或新页面状态机。
Codex 管会话、权限、Agent 与队列；Git 管工作树；Forge 管交付与评审。
会话客户端、队列界面和终端抽屉保留为高级维护能力，在替代交接经过验证前不盲目删除。

命名 Target 表示每次连接一个明确目标，不是多个服务器同屏聚合。本轮先修正
Headless 目标选择与部分数据说明。不扩展多目标并发、自动高风险审批、未知写入自动重试、
团队服务、通用 Agent 框架；不增加云服务、付费资源、常驻守护进程或持久化权威状态。

<!-- docs-section: implementation -->
## 六个落地工作包

| 工作包 | 最终行为 | 验收标准 |
|---|---|---|
| L1 待处理 | 最近列表按交互、已观测阻塞、评审、未读、固定、运行、最近历史排序；运行和待处理不被最近窗口截掉；显式搜索/全部历史保持原有顺序。 | 重排不改变所选身份；稍后提醒与已处理语义明确；窄终端不用颜色也看得到原因。 |
| L2 日常入口 | F1 今日、F2 项目、F3 交付评审复用已有视图；在线程上按 o 打开已知交付链接，缺少数据则明确提示。 | 不新建任务库，不因导航执行写操作；保留身份与 URL 校验、原快捷槽和 F6。 |
| L3 Headless | 复用活动 Target 和 --target NAME，未知目标不得回退本地；结构化覆盖范围说明 Git/Goal/Forge 未采集；本地卡片建立一次锚点索引。 | 纯参数检查和真实程序负例；空待处理不等于所有工作正常；本地 Forge/Worktree 命令拒绝 Target 参数。 |
| L4 视口 | 只扫描轻量分组元数据，只格式化可见行；列表单项截成一个物理行，保证选择和滚动坐标一致。 | 分组、中文、空列表、零高度和万条末尾可达；不声称排序与分组变成常数开销。 |
| L5 维护减负 | 缩短日常帮助，增加成对的当前手册，标记过时实现叙述；非必需性能诊断只响应运行时/策略变化。 | 五项必需检查始终触发；发布门禁和精确来源证据保留，不继承旧 SHA 的通过。 |
| L6 收益验收 | 离线成对工作流测量与严格输入校验，不采集会话、Token、仓库路径或提示词。 | 缺少证据明确为未测量；保留失败，同类任务配对、交替执行顺序；模拟数据不得签发实用收益或发布资格。 |

数值模块上限继续只由 release/v1.4-plan.json 管理。按照本方案审阅本轮修改文件与新增聚焦模块，
无关模块上限不动。不以测试数量或代码行数代替可用性，更不能为了减少代码而删除安全保护。

<!-- docs-section: qualification -->
## 验收与停止条件

对照用户已经拥有的官方 Codex、Git/gh/glab 和终端/编辑器，完成三类实际工作：
寻找阻塞、定位交付证据、中断后恢复上下文。仅记录匿名任务/操作者编号、固定候选 SHA、
基线工具版本；至少 12 组成对记录，每类至少四组，并平衡先后顺序。
用时、操作、切换、完成率和错误分别报告。建议目标为中位任务用时或切换次数降低 30%，
同时错误不增加、完成率不下降；这是待验证目标，不是已经获得的收益。
测量工具仅汇总操作者提供的观察，不是独立认证器。

没有真实数据就保持 NOT_MEASURED。托管回归不签发真实 Codex、内网 GitLab、
Windows→SSH、控制终端或多小时使用资格。Issue #211 继续跟踪外部发布前提；
Preview 成功不等于 Stable dry-run 通过，现有稳定就绪和发布门禁继续生效。

候选与主线保留完整原生、安全及受影响协议/终端回归，周期规模和持续模拟仍用于诊断。
只在触发范围得到回归覆盖时减少非必需重复工作，不取消合并后与正式发布保护。
CI 节省与真实人时收益在测量前均未知。六包完成后停止扩张；多 Target 联邦聚合和
兼容能力删除必须由实际使用证明必要性。

<!-- docs-section: sources -->
## 本轮采用的一手资料

2026-10-21 查询上游最新稳定版为 Codex 0.163.0，发布于 2026-10-15；具体功能仍取决于服务端和账号。

- [Codex 官方 CLI 文档](https://developers.openai.com/codex/cli/features)、[App Server 契约](https://developers.openai.com/codex/app-server)：会话和权限由上游负责。
- [0.163.0](https://github.com/openai/codex/releases/tag/rust-v0.163.0)、[0.162.0](https://github.com/openai/codex/releases/tag/rust-v0.162.0)：上游配置、任务和 Worktree 能力持续完善，重复开发难形成长期优势。
- [GitLab CLI](https://docs.gitlab.com/cli/)：保留自托管实例和已有认证，避免无测量依据的新传输层。
- [Ratatui Widgets](https://docs.rs/ratatui/latest/ratatui/widgets/index.html)：明确布局与视口状态。
- [GitHub 必需检查说明](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)：路径过滤不能让必需检查永久 Pending。
- [Microsoft Research：The SPACE of AI](https://www.microsoft.com/en-us/research/publication/the-space-of-ai-real-world-lessons-on-ais-impact-on-developers/)：用多维工作流指标衡量效率，而非代码或测试数量。

第三方宣传、Star 和合成延迟不作为价值证明。此前对比引用的 lazygit.dev 不是官方项目证据，
本轮不据此导入代码、依赖或变更许可。
''')
p = Path('docs/i18n/manifest.json'); data = json.loads(p.read_text()); data['pairs'].append({'id':'lean-mission-control','en':'docs/implementation/lean-mission-control.md','zh-CN':'docs/zh-CN/implementation/lean-mission-control.md','sections':sections}); p.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
# Add a concise current route without rewriting historical release claims.
for path, text in [
    ('README.md', '\n**Daily workflow:** F1 Today · F2 Projects · F3 Review · Ctrl+K actions. Named targets select one server per invocation. [Lean product plan](docs/implementation/lean-mission-control.md) defines the current scope; real productivity remains unmeasured.\n'),
    ('README.zh-CN.md', '\n**日常路径：** F1 今日 · F2 项目 · F3 交付评审 · Ctrl+K 操作。命名 Target 每次选择一个服务端。[精简最终方案](docs/zh-CN/implementation/lean-mission-control.md) 定义当前范围；真实效率收益尚未测量。\n')]:
    p=Path(path); s=p.read_text(); marker='<!-- docs-section: overview -->'; assert marker in s; p.write_text(s.replace(marker, marker+'\n'+text,1))
p=Path('README.md'); s=p.read_text(); s=s.replace('cross-target registry', 'selected-target registry').replace('cross-target-codex-registry', 'selected-target-codex-registry'); p.write_text(s)
# Pair-wise daily quickstart update: preserve all advanced shortcuts but teach the primary path first.
for path, text in [
    ('docs/team-quickstart.md', '\nStart with **F1 Today**, **F2 Projects**, **F3 Review**, then **Ctrl+K** for contextual actions. `o` opens the selected thread delivery in your browser when identity is available. Keep existing hot slots and F6 for drawer focus; advanced conversation/queue/terminal functions remain available, not the default workflow.\n'),
    ('docs/zh-CN/team-quickstart.md', '\n优先使用 **F1 今日**、**F2 项目**、**F3 交付评审**，其他操作通过 **Ctrl+K** 查找。`o` 在身份信息可用时打开所选线程的交付链接。原快捷槽和 F6 焦点释放保留；高级会话、队列和终端能力仍可用，但不是默认学习路径。\n')]:
    p=Path(path); s=p.read_text(); marker='<!-- docs-section: daily -->'; assert marker in s; p.write_text(s.replace(marker, marker+'\n'+text,1))
for path, text in [
    ('docs/guides/cli-reference.md', '\nRegistry-derived commands (`threads`, `work`, `status`, `attention`, `board`) accept `--target NAME`; without it they use `[app_server].active`. JSON `source.coverage` explicitly marks Git/Goal/Forge as `not-collected`, so no attention items is not an all-source health verdict. `forge` and `worktrees` remain local queries and reject `--target`; fixtures cannot be combined with a target.\n'),
    ('docs/zh-CN/guides/cli-reference.md', '\nRegistry 派生命令（threads、work、status、attention、board）支持 `--target NAME`，默认使用 `[app_server].active`。JSON 的 `source.coverage` 明确将 Git/Goal/Forge 标为 `not-collected`，空待处理并不证明所有来源正常。forge 和 worktrees 仍是本地查询，拒绝 --target；模拟数据也不能与目标参数混用。\n')]:
    p=Path(path); s=p.read_text(); marker='<!-- docs-section: headless -->'; assert marker in s; p.write_text(s.replace(marker, marker+'\n'+text,1))
# Historical notes remain available, but are not presented as current instructions.
for path in ['docs/implementation/m4-personal-planning-sqlite.md','docs/implementation/v1.4-long-thread-viewport-cache.md']:
    p=Path(path); s=p.read_text(); first, rest=s.split('\n',1); p.write_text(first+'\n\n> Historical implementation record. Later first-deployment and visual-row fixes supersede the legacy import/item-offset descriptions below. See [the current product plan](lean-mission-control.md); this record is not an active migration or scrolling contract.\n'+rest)
plan = {'schema':'codex-tui/lean-mission-control-plan/v1','entryMainSha':'d74020ecd69b19ed83d2342b3d99497b800e68d0',
 'status':'implementation','theme':'v1.5-lean-mission-control-and-productivity-qualification','packageVersion':'1.4.0',
 'authority':'new-user-authorized-development-scope','numericModuleLedger':'release/v1.4-plan.json',
 'packages':['attention-first','daily-navigation-and-delivery','headless-target-coverage-and-index','list-viewport','maintenance-and-trigger-scope','paired-productivity-measurement'],
 'targetMode':'one-explicit-server-per-invocation','defaultWorkScope':'registry-local',
 'productivityEvidence':'NOT_MEASURED','stableReady':False,'publicationAllowed':False,
 'nonGoals':['concurrent-target-federation','second-agent-runtime','team-service','automatic-approval','unknown-write-retry','remove-compatibility-without-tested-handoff'],
 'guide':'docs/implementation/lean-mission-control.md'}
write('release/v1.5-lean-plan.json',json.dumps(plan,ensure_ascii=False,indent=2)+'\n')
p=Path('release/v1.5-convergence.json'); data=json.loads(p.read_text()); data['nextDevelopmentTheme']='v1.5-lean-mission-control-and-productivity-qualification'; data['developmentPlan']='release/v1.5-lean-plan.json'; data['targetMode']='one-selected-target-per-invocation'; data['differentiation']=[x.replace('cross-target-codex-registry','selected-target-codex-registry') for x in data['differentiation']]; p.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
# Narrow ONLY the advisory performance workflow. Required checks and release authority are untouched.
p=Path('.github/workflows/performance-diagnostics.yml'); s=p.read_text(); start=s.index('  pull_request:\n    paths:\n')+len('  pull_request:\n    paths:\n'); end=s.index('  workflow_dispatch:',start); paths=s[start:end];
for path in ['src/attention.rs','src/ui/planning_viewport.rs','rust-toolchain.toml','release/v1.5-lean-plan.json','scripts/release/test_performance_trigger_scope.py']:
    line='      - "'+path+'"\n'
    if line not in paths: paths+=line
s=s[:start]+paths+s[end:]; s=s.replace('  push:\n    branches: [main]\n','  push:\n    branches: [main]\n    paths:\n'+paths,1); p.write_text(s)
p=Path('scripts/release/test_performance_trigger_scope.py'); s=p.read_text().replace('"""Ensure PR-only performance diagnostics skip unrelated release script churn."""','"""Non-required diagnostics use one reviewed runtime/policy scope on PR and main."""').replace('def test_main_push_always_retains_exact_sha_evidence(self):','def test_relevant_main_push_retains_exact_sha_evidence(self):');
s=s.replace('    def test_pr_skips_unrelated_release_helpers(self):', '''    def test_main_and_pr_share_scope_without_filtering_required_checks(self):
        text = PERF.read_text(encoding="utf-8")
        section = text.split("  push:\\n    branches: [main]\\n    paths:\\n", 1)[1].split("  pull_request:", 1)[0]
        paths = re.findall(r'      - "([^"\\n]+)"', section)
        self.assertEqual(paths, pr_paths())
        for name in ("ci.yml", "security.yml"):
            workflow = (ROOT / ".github/workflows" / name).read_text(encoding="utf-8")
            header = workflow.split("permissions:", 1)[0]
            self.assertNotIn("    paths:", header)
            self.assertNotIn("    paths-ignore:", header)
        self.assertNotIn("README.md", paths)
        self.assertIn("rust-toolchain.toml", paths)
        self.assertIn("src/attention.rs", paths)

    def test_pr_skips_unrelated_release_helpers(self):''')
p.write_text(s)
