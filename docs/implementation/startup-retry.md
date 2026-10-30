# First-connection retry / 首次连接重试

## Operator behavior / 操作方式

When the first App Server connection fails before any registry is installed, the error begins with an F5 hint. Return to Mission Control, close overlays, and press plain F5 once to retry the same resolved target. There is one connection attempt in flight, no automatic timer, and no target/configuration switch. Local drafts, pins, aliases and planning state are not reset.

首次 App Server 连接在安装任何 Registry 前失败时，错误提示以 F5 开头。返回任务中心并关闭弹层后，单次按下不带修饰键的 F5，重试同一个已解析目标。最多一个连接请求在途，无自动计时重试，不切换目标或重新读取配置；本地草稿、置顶、别名和规划状态不重置。

F5 is not consumed in editors, help, command menus, request details, operation confirmations or the Terminal Drawer. Modified, repeated and release events do not start an attempt. Fake mode has no resolved live target and cannot start one. Correct invalid configuration before launching; this retry is for a failed connection, not a configuration editor.

编辑、帮助、命令菜单、审批详情、操作确认或终端抽屉拥有输入时，重试不会抢键；修饰键、长按重复和松开事件不触发重试。假数据模式没有真实目标，不能启动后端。配置解析错误仍需先修正配置后再启动；此功能不是配置编辑器。

## Safety boundary / 安全边界

The first successfully installed Registry permanently disables this startup-only path for the process, even after a subsequent disconnect. It never reconnects an established session, retries a prompt, answers an approval, starts/forks a thread, mutates a queue or writes to a Forge. An uncertain write remains uncertain. Re-establishing previously connected sessions requires a separately reviewed recovery design; it is not provided here.

进程首次成功安装 Registry 后永久关闭该入口，后续即使断开也不重新启用。本路径不会恢复已建立过的会话，不重试提示词、不代答审批、不创建或派生会话、不修改队列或 Forge；结果未知的写操作仍保持未知。已连接会话的恢复需要独立审查，本次不提供。

Only completed startup tasks are awaited; slow startup does not block keyboard handling. Quit still aborts an outstanding attempt. The initial connection and explicit retries use the same existing transport validation and bootstrap limits. No persisted schema, dependency, workflow permission or publication authority changes.

仅接收已完成的启动任务，不在输入线程等待慢连接；退出仍中止在途尝试。首次连接与手动重试使用原传输校验和启动预算，不改变持久格式、依赖、工作流权限或发布授权。

## Verification / 验证

Seven unit regressions inject the spawn boundary without executing any backend. A Linux controlling-PTY fixture runs the actual TUI with an absolute isolated mock executable: first startup fails, idle does not retry, help retains F5, repeated presses cannot overlap a held bootstrap, explicit retry installs the observed thread, and F5 cannot reconnect after the established fixture closes. It checks normal exit, terminal restoration and the absence of mutation RPCs. All fixture state is disposable and no account, real Codex executable or remote endpoint is used.

七项单元回归注入启动边界，不执行后端；Linux 控制 PTY 回归运行真实 TUI，但配置绝对路径的隔离假后端：首连失败后不会自行重试，帮助页不抢键，在途连接不重叠，手动重试能安装观测到的会话，已建立连接退出后 F5 不会重连。验证正常退出、终端恢复及无变更 RPC，状态均为临时数据，不使用账号、真实 Codex 可执行文件或远程地址。

Native CI must pass on the exact candidate and the protected merged main. These tests are not Windows SSH, real-account, internal GitLab, multi-hour or human-efficiency qualification. Keep #211 open, `stableReady=false` and `publicationAllowed=false`.

精确候选与受保护合并后的主线都必须取得各自原生 CI。夹具不替代 Windows SSH、真实账号、内网 GitLab、多小时或实际效率资格；#211 保持开放，`stableReady=false`、`publicationAllowed=false`。
