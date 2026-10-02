use super::{command_palette_choice_label, context_choice_label, tr};
use crate::{
    app::{AppState, InputMode, SavedViewEditField},
    text::{sanitize_inline, truncate_display},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};

pub(super) fn render_command_palette(frame: &mut Frame<'_>, app: &AppState) {
    let choices = app.command_palette_choices();
    let lines = choices
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            let selected = index == app.command_palette_selected;
            let style = if selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            Line::from(Span::styled(
                format!(
                    "{} {}",
                    if selected { ">" } else { " " },
                    command_palette_choice_label(*choice, app.language)
                ),
                style,
            ))
        })
        .chain(std::iter::once(Line::from("")))
        .chain(std::iter::once(Line::from(tr(
            app,
            "j/k move · Enter execute · Esc close · Ctrl+K toggle",
            "j/k 移动 · Enter 执行 · Esc 关闭 · Ctrl+K 切换",
        ))))
        .collect::<Vec<_>>();
    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(20)
        .clamp(7, 24);
    let area = centered_fixed(72, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(tr(app, " Command Palette ", " 命令面板 ")))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(super) fn render_context_actions(frame: &mut Frame<'_>, app: &AppState) {
    let choices = app.context_choices();
    let lines = choices
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            let selected = index == app.context_selected;
            let style = if selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            Line::from(Span::styled(
                format!(
                    "{} {}",
                    if selected { ">" } else { " " },
                    context_choice_label(*choice, app.language)
                ),
                style,
            ))
        })
        .chain(std::iter::once(Line::from(tr(
            app,
            "j/k move · Enter execute · Esc close",
            "j/k 移动 · Enter 执行 · Esc 关闭",
        ))))
        .collect::<Vec<_>>();
    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(18)
        .clamp(6, 24);
    let area = centered_fixed(64, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(tr(app, " Actions ", " 操作 ")))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(super) fn render_local_input_overlay(frame: &mut Frame<'_>, app: &AppState) {
    let (title, hint) = match app.input_mode {
        InputMode::Note => (
            tr(app, " Local note ", " 本地备注 "),
            tr(app, "Enter save · Esc cancel", "Enter 保存 · Esc 取消"),
        ),
        InputMode::Snooze => (
            tr(app, " Snooze ", " 稍后提醒 "),
            tr(
                app,
                "15m / 1h / 1d · Enter apply · Esc cancel",
                "15m / 1h / 1d · Enter 应用 · Esc 取消",
            ),
        ),
        InputMode::SavedViewName => (
            tr(app, " Save current view ", " 保存当前视图 "),
            tr(app, "Enter save · Esc cancel", "Enter 保存 · Esc 取消"),
        ),
        InputMode::SavedViewEdit => (
            tr(app, " Edit SavedView ", " 编辑 SavedView "),
            match app.saved_view_edit_field {
                Some(SavedViewEditField::Name) => tr(
                    app,
                    "name · non-empty · Enter save · Esc cancel",
                    "名称 · 不能为空 · Enter 保存 · Esc 取消",
                ),
                Some(SavedViewEditField::Source) => {
                    "source · all | scratch | thread | forge · Enter save · Esc cancel"
                }
                Some(SavedViewEditField::Filter) => tr(
                    app,
                    "filter · SavedView query syntax · Enter save · Esc cancel",
                    "过滤器 · SavedView 查询语法 · Enter 保存 · Esc 取消",
                ),
                Some(SavedViewEditField::Layout) => {
                    "layout · list | board | review-queue · Enter save · Esc cancel"
                }
                Some(SavedViewEditField::GroupBy) => {
                    "group · none | stage | workspace | source · Enter save · Esc cancel"
                }
                Some(SavedViewEditField::OrderBy) => {
                    "order · priority | title | stage | workspace · Enter save · Esc cancel"
                }
                Some(SavedViewEditField::VisibleFields) => {
                    "fields · stage,attention,workspace,source,goal,priority,branch,forge · Enter save · Esc cancel"
                }
                None => tr(
                    app,
                    "field unavailable · Esc cancel",
                    "字段不可用 · Esc 取消",
                ),
            },
        ),
        InputMode::BatchAddTag => (
            tr(
                app,
                " Batch visible · Add tag ",
                " 批量当前可见项 · 添加标签 ",
            ),
            tr(
                app,
                "Enter creates frozen plan · Esc cancel",
                "Enter 创建冻结计划 · Esc 取消",
            ),
        ),
        InputMode::BatchRemoveTag => (
            tr(
                app,
                " Batch visible · Remove tag ",
                " 批量当前可见项 · 移除标签 ",
            ),
            tr(
                app,
                "Enter creates frozen plan · Esc cancel",
                "Enter 创建冻结计划 · Esc 取消",
            ),
        ),
        InputMode::BatchPriority => (
            tr(
                app,
                " Batch visible · Set priority ",
                " 批量当前可见项 · 设置优先级 ",
            ),
            tr(
                app,
                "integer · Enter creates frozen plan · Esc cancel",
                "整数 · Enter 创建冻结计划 · Esc 取消",
            ),
        ),
        InputMode::BatchSnooze => (
            tr(
                app,
                " Batch visible · Snooze ",
                " 批量当前可见项 · 稍后提醒 ",
            ),
            tr(
                app,
                "15m / 1h / 1d · Enter creates frozen plan · Esc cancel",
                "15m / 1h / 1d · Enter 创建冻结计划 · Esc 取消",
            ),
        ),
        InputMode::ForgeMergeRequestTitle => (
            tr(
                app,
                " Create GitLab merge request ",
                " 创建 GitLab 合并请求 ",
            ),
            tr(
                app,
                "Enter creates a plan only · Esc cancel",
                "Enter 仅创建计划 · Esc 取消",
            ),
        ),
        InputMode::ForgeComment => (
            tr(
                app,
                " Comment on GitLab merge request ",
                " 评论 GitLab 合并请求 ",
            ),
            tr(
                app,
                "Enter creates a plan only · Esc cancel",
                "Enter 仅创建计划 · Esc 取消",
            ),
        ),
        _ => return,
    };
    let area = centered_fixed(64, 7, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(app.input_buffer.clone()),
            Line::from(""),
            Line::from(hint),
        ])
        .block(Block::bordered().title(title))
        .wrap(Wrap { trim: false }),
        area,
    );
}

pub(super) fn render_forge_mutation_confirmation(frame: &mut Frame<'_>, app: &AppState) {
    let Some(plan) = &app.pending_forge_operation else {
        return;
    };

    let mut lines = vec![
        Line::from(tr(
            app,
            "CONFIRM REQUIRED — no GitLab mutation has executed yet.",
            "需要确认 — 尚未执行任何 GitLab 变更。",
        )),
        Line::from(format!(
            "{}: {}",
            tr(app, "Operation", "操作"),
            plan.kind.label()
        )),
        Line::from(format!(
            "{}: {}/{}",
            tr(app, "Project", "项目"),
            plan.host,
            plan.project_path
        )),
    ];
    if let Some(iid) = plan.change_request_iid {
        lines.push(Line::from(format!(
            "{}: !{iid}",
            tr(app, "Merge request", "合并请求")
        )));
    }
    if plan.source_branch.is_some() || plan.target_branch.is_some() {
        lines.push(Line::from(format!(
            "{}: {} -> {}",
            tr(app, "Branches", "分支"),
            plan.source_branch
                .as_deref()
                .unwrap_or_else(|| tr(app, "<none>", "<无>")),
            plan.target_branch
                .as_deref()
                .unwrap_or_else(|| tr(app, "<none>", "<无>"))
        )));
    }
    if let Some(title) = &plan.title {
        lines.push(Line::from(format!(
            "{}: {}",
            tr(app, "Title", "标题"),
            truncate_display(title, 88)
        )));
    }
    if let Some(bytes) = plan.payload_bytes {
        lines.push(Line::from(if app.language.is_simplified_chinese() {
            format!("Payload: {bytes} 字节 · 正文按设计不持久化")
        } else {
            format!("Payload: {bytes} bytes · body intentionally not persisted")
        }));
    }
    lines.push(Line::from(format!(
        "{}: {}",
        tr(app, "Expected", "预期结果"),
        truncate_display(&plan.expected_side_effect, 100)
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(tr(
        app,
        "Preconditions revalidated at execution time:",
        "执行时将重新验证前置条件:",
    )));
    lines.extend(
        plan.preconditions
            .iter()
            .take(8)
            .map(|item| Line::from(format!("  {} = {}", item.key, item.expected))),
    );
    lines.push(Line::from(""));
    lines.push(Line::from(tr(
        app,
        "y CONFIRM execute · c/Esc cancel",
        "y 确认执行 · c/Esc 取消",
    )));

    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(18)
        .clamp(10, 22);
    let area = centered_fixed(82, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(tr(app, " Forge Mutation Plan ", " Forge 变更计划 ")))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(super) fn render_local_batch_confirmation(frame: &mut Frame<'_>, app: &AppState) {
    let Some(plan) = &app.pending_local_batch else {
        return;
    };

    let mut lines = vec![
        Line::from(tr(
            app,
            "CONFIRM REQUIRED — no local write has executed yet.",
            "需要确认 — 尚未执行任何本地写入。",
        )),
        Line::from(format!(
            "{}: {}",
            tr(app, "Operation", "操作"),
            plan.action.label()
        )),
        Line::from(format!(
            "{}: {}",
            tr(app, "Frozen targets", "冻结目标"),
            plan.targets.len()
        )),
        Line::from(tr(
            app,
            "Target set will NOT be recomputed before execution.",
            "执行前不会重新计算目标集合。",
        )),
        Line::from(""),
    ];

    for target in plan.targets.iter().take(8) {
        lines.push(Line::from(format!(
            "  {} · {}",
            target.local_id,
            truncate_display(&target.title, 48)
        )));
    }
    if plan.targets.len() > 8 {
        lines.push(Line::from(if app.language.is_simplified_chinese() {
            format!("  … 另有 {} 个冻结目标", plan.targets.len() - 8)
        } else {
            format!("  … and {} more frozen target(s)", plan.targets.len() - 8)
        }));
    }

    lines.extend([
        Line::from(""),
        Line::from(tr(
            app,
            "SQLite: one transaction; any failure rolls the whole batch back.",
            "SQLite: 单事务执行；任一失败都会回滚整批。",
        )),
        Line::from(tr(
            app,
            "Authority: local overlays / ScratchWork only; no Codex/Git/Forge write.",
            "权限边界：仅本地 overlay / ScratchWork；不写 Codex/Git/Forge。",
        )),
        Line::from(tr(
            app,
            "y CONFIRM execute · c/Esc cancel",
            "y 确认执行 · c/Esc 取消",
        )),
    ]);

    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(18)
        .clamp(12, 24);
    let area = centered_fixed(86, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(tr(app, " Local Batch Plan ", " 本地批量计划 ")))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(super) fn render_launch_presets(frame: &mut Frame<'_>, app: &AppState) {
    let mut lines = Vec::new();
    if app.launch_presets.is_empty() {
        lines.push(Line::from(tr(
            app,
            "No [[launch]] presets in .codex-tui.toml.",
            ".codex-tui.toml 中没有 [[launch]] 预设。",
        )));
    } else {
        for (index, preset) in app.launch_presets.iter().enumerate() {
            let selected = index == app.launch_selected;
            let style = if selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            let argv = preset
                .argv
                .iter()
                .map(|arg| format!("{arg:?}"))
                .collect::<Vec<_>>()
                .join(" ");
            lines.push(Line::from(Span::styled(
                format!(
                    "{} {} · cwd={:?} · {}",
                    if selected { ">" } else { " " },
                    sanitize_inline(&preset.name),
                    preset.cwd.label(),
                    truncate_display(&argv, 72)
                ),
                style,
            )));
        }
    }
    lines.extend([
        Line::from(""),
        Line::from(tr(
            app,
            "j/k move · Enter create exact launch plan · Esc close",
            "j/k 移动 · Enter 创建精确启动计划 · Esc 关闭",
        )),
    ]);
    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(12)
        .clamp(7, 22);
    let area = centered_fixed(92, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(tr(
                app,
                " Repository Launch Presets ",
                " 仓库启动预设 ",
            )))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(super) fn render_launch_confirmation(frame: &mut Frame<'_>, app: &AppState) {
    let Some(plan) = &app.pending_launch_plan else {
        return;
    };
    let lines = vec![
        Line::from(tr(
            app,
            "CONFIRM REQUIRED — process has not started.",
            "需要确认 — 进程尚未启动。",
        )),
        Line::from(format!(
            "{}: {}",
            tr(app, "Preset", "预设"),
            sanitize_inline(&plan.name)
        )),
        Line::from(format!(
            "{}: {}",
            tr(app, "Config", "配置"),
            plan.config_path.display()
        )),
        Line::from(format!("Cwd: {}", plan.cwd.display())),
        Line::from(format!(
            "{}: {}",
            tr(app, "Exact argv", "精确 argv"),
            plan.command_preview()
        )),
        Line::from(""),
        Line::from(tr(
            app,
            "No shell/eval/interpolation is used.",
            "不使用 shell/eval/插值。",
        )),
        Line::from(tr(
            app,
            "External process only; this is not the embedded Terminal Drawer.",
            "仅启动外部进程；这不是内嵌终端抽屉。",
        )),
        Line::from(tr(
            app,
            "y CONFIRM start · c/Esc cancel",
            "y 确认启动 · c/Esc 取消",
        )),
    ];
    let area = centered_fixed(92, 13, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(tr(app, " Launch Preset Plan ", " 启动预设计划 ")))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn centered_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(1);
    let height = height.min(area.height.saturating_sub(2)).max(1);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}
