<!-- docs-id: device-matrix -->
<!-- docs-lang: zh-CN -->
# 设备集成验收记录——尚未执行
<!-- docs-section: overview -->

**Language / 语言:** [English](../../guides/device-matrix.md)

复制到私有目录，不提交凭据或原始任务。执行前选择 development-integration 或 final-candidate，记录源码 SHA、二进制摘要、设备／SSH／Codex／Provider 版本、配置哈希、时区、操作者及独立复核者。状态：NOT_RUN、PASS、FAIL、BLOCKED 或独立批准的 N/A。不得自动完成。

[操作手册](device-validation.md)

## 执行矩阵
<!-- docs-section: matrix -->

| ID | 验收内容 | 状态 | 实际结果／证据 | 复核者 |
|---|---|---|---|---|
| E01 | 源码、远端main与工作区 | NOT_RUN | | |
| E02 | 架构/ABI、Python/Rust/Git/CLI版本 | NOT_RUN | | |
| E03 | 真实TTY、UTF-8、窗口尺寸 | NOT_RUN | | |
| E04 | 实际程序sourceSha和二进制哈希 | NOT_RUN | | |
| A01 | 本机发布工具/观察器测试 | NOT_RUN | | |
| A02 | 受控原生九套/108项（不是实服资格） | NOT_RUN | | |
| C01 | 两仓真实会话与Cwd/来源正确 | NOT_RUN | | |
| C02 | TUI真实推理任务完成且结果正确 | NOT_RUN | | |
| C03 | 实际审批允许与拒绝 | NOT_RUN | | |
| C04 | 真正request_user_input交互 | NOT_RUN | | |
| C05 | 活跃任务跨Board/搜索/返回 | NOT_RUN | | |
| C06 | 实际任务取消、不重放 | NOT_RUN | | |
| C07 | 草稿/Saved View/备注跨重启 | NOT_RUN | | |
| C08 | 导航、中文、滚动、窄窗口 | NOT_RUN | | |
| C09 | 日常命名targets（非必需可审阅N/A） | NOT_RUN | | |
| T01 | Drawer正确Cwd与输入回显 | NOT_RUN | | |
| T02 | 实际宽高resize | NOT_RUN | | |
| T03 | sleep期间Ctrl+C后Shell仍可用 | NOT_RUN | | |
| T04 | F6、重新聚焦、T关闭 | NOT_RUN | | |
| T05 | 正常退出父终端恢复 | NOT_RUN | | |
| T06 | 可捕获信号异常退出恢复 | NOT_RUN | | |
| T07 | 真实SSH断连/重连，独立TTY收据 | NOT_RUN | | |
| T08 | 文档要求的panic/错误退出范围（不虚构） | NOT_RUN | | |
| G01 | Git状态/diff/分支 | NOT_RUN | | |
| G02 | 受管Worktree创建、路径身份 | NOT_RUN | | |
| G03 | 活跃/脏树删除拒绝与取消 | NOT_RUN | | |
| G04 | 合法删除后Git和文件系统均一致 | NOT_RUN | | |
| G05 | 编辑器路径边界 | NOT_RUN | | |
| G06 | 独立Codex与同仓多会话并发 | NOT_RUN | | |
| F01 | 内网GitLab三项只读能力+有效收据 | NOT_RUN | | |
| F02 | 测试评论取消／确认恰好一次 | NOT_RUN | | |
| F03 | 审批HEAD绑定，修订变化拒绝 | NOT_RUN | | |
| F04 | 专用测试MR合法合并 | NOT_RUN | | |
| F05 | 无权限/失败/结果未知不重放 | NOT_RUN | | |
| F06 | GitHub同类场景（按声明范围） | NOT_RUN | | |
| S01 | Store健康、当前schema、单写者 | NOT_RUN | | |
| S02 | 一致性备份包含已提交状态 | NOT_RUN | | |
| S03 | 新隔离Store恢复、数据可核对 | NOT_RUN | | |
| L01 | 约定真实使用时长/任务覆盖 | NOT_RUN | | |
| L02 | root与进程家族资源趋势分别复核 | NOT_RUN | | |
| L03 | 任务结束后的资源/活动趋稳 | NOT_RUN | | |
| L04 | 实际恢复/残留进程/状态完整 | NOT_RUN | | |
| L05 | 实际人时/步骤/token（可取得项） | NOT_RUN | | |
| Q01 | 最终真实compat/终端/GitLab证据 | NOT_RUN | | |
| Q02 | 最终候选Linux资格链、构建后哈希复核 | NOT_RUN | | |
| Q03 | 私有归档校验与公开摘要脱敏 | NOT_RUN | | |

## 独立结论
<!-- docs-section: conclusion -->

设备集成：NOT_RUN。资格复核：NOT_RUN。实际安装／回退：NOT_RUN。管理员／演练／发布授权：NOT_RUN。记录声明范围、阻断项、未观察项目及后续责任。保留失败；源码或程序变化后重新验证受影响范围。设备通过不授予 Stable 发布权。
