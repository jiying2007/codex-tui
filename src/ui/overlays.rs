use super::{context_choice_label, tr};
use crate::{
    app::{AppState, InputMode},
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
    let matches = app.command_palette_matches();
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("{}> ", tr(app, "Query", "搜索")),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(if app.command_palette_query.is_empty() {
            tr(app, "<type to filter>", "<输入以筛选>").to_string()
        } else {
            app.command_palette_query.clone()
        }),
    ])];

    if matches.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(tr(
            app,
            "No matching commands.",
            "没有匹配的命令。",
        )));
    } else {
        for (index, matched) in matches.iter().enumerate() {
            let selected = index == app.command_palette_selected;
            let base = if selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            let mut spans = vec![Span::styled(if selected { "> " } else { "  " }, base)];
            let matched_indices = matched
                .matched_char_indices
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>();
            for (char_index, character) in matched.label.chars().enumerate() {
                let style = if matched_indices.contains(&char_index) {
                    base.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                } else {
                    base
                };
                spans.push(Span::styled(character.to_string(), style));
            }
            lines.push(Line::from(spans));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(tr(
        app,
        "type/paste fuzzy query · ↑/↓ move · Enter execute · Backspace edit · Esc close · Ctrl+K toggle",
        "输入/粘贴模糊搜索 · ↑/↓ 移动 · Enter 执行 · Backspace 编辑 · Esc 关闭 · Ctrl+K 切换",
    )));
    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(20)
        .clamp(7, 26);
    let area = centered_fixed(86, height, frame.area());
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
        InputMode::TranscriptSearch => (
            tr(app, " Search transcript history ", " 搜索会话全文 "),
            tr(
                app,
                "Enter search · Esc cancel · App Server preferred, local index fallback",
                "Enter 搜索 · Esc 取消 · 优先 App Server，本地索引回退",
            ),
        ),
        InputMode::ThreadQueueAdd => (
            tr(app, " Add queued prompt ", " 添加队列提示词 "),
            tr(
                app,
                "Enter submit to Codex queue · Esc cancel",
                "Enter 提交到 Codex 队列 · Esc 取消",
            ),
        ),
        InputMode::ThreadQueueEdit => (
            tr(app, " Edit queued prompt ", " 编辑队列提示词 "),
            tr(
                app,
                "Enter replace text input · Esc cancel",
                "Enter 替换文本输入 · Esc 取消",
            ),
        ),
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
        InputMode::SavedViewField => (
            tr(app, " Edit Saved View field ", " 编辑已保存视图字段 "),
            tr(
                app,
                "Enter apply field · Esc cancel",
                "Enter 应用字段 · Esc 取消",
            ),
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
                " Create forge change request ",
                " 创建 Forge 变更请求 ",
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
                " Comment on forge change request ",
                " 评论 Forge 变更请求 ",
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

pub(super) fn render_transcript_search(frame: &mut Frame<'_>, app: &AppState) {
    if !app.transcript_search_open {
        return;
    }

    let source = app.transcript_search_source_label();
    let mut lines = vec![Line::from(format!(
        "{}: {} · {}: {}{}",
        tr(app, "Query", "查询"),
        sanitize_inline(&app.transcript_search_query),
        tr(app, "Source", "来源"),
        source,
        if app.transcript_search_loading {
            tr(app, " · upgrading…", " · 正在升级到服务端结果…")
        } else {
            ""
        }
    ))];

    if let Some(error) = app.transcript_search_error.as_deref() {
        lines.push(Line::from(format!(
            "{}: {}",
            tr(app, "Server note", "服务端提示"),
            truncate_display(&sanitize_inline(error), 92)
        )));
    }

    match app.transcript_search_results.as_ref() {
        Some(results) if results.hits.is_empty() => {
            lines.push(Line::from(""));
            lines.push(Line::from(tr(
                app,
                "No transcript matches in the available search authority.",
                "当前可用搜索来源中没有匹配的会话内容。",
            )));
        }
        Some(results) => {
            lines.push(Line::from(""));
            let window_start = app
                .transcript_search_selected
                .saturating_sub(8)
                .min(results.hits.len().saturating_sub(18));
            for (index, hit) in results.hits.iter().enumerate().skip(window_start).take(18) {
                let selected = index == app.transcript_search_selected;
                let style = if selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                let thread_title = app
                    .threads
                    .iter()
                    .find(|thread| thread.id == hit.thread_id)
                    .map(|thread| thread.display_title())
                    .unwrap_or_else(|| hit.thread_id.0.as_str());
                let location = match (&hit.turn_id, &hit.item_id) {
                    (Some(turn), Some(item)) => format!(
                        "{} / {}",
                        truncate_display(turn, 16),
                        truncate_display(item, 16)
                    ),
                    _ => tr(app, "thread match", "会话匹配").to_string(),
                };
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} {} · {} · {}",
                        if selected { ">" } else { " " },
                        truncate_display(thread_title, 32),
                        location,
                        truncate_display(&sanitize_inline(&hit.snippet), 72)
                    ),
                    style,
                )));
            }
            if results.hits.len() > 18 {
                let first = window_start + 1;
                let last = (window_start + 18).min(results.hits.len());
                lines.push(Line::from(if app.language.is_simplified_chinese() {
                    format!("  显示 {first}-{last} / {} 条结果", results.hits.len())
                } else {
                    format!(
                        "  showing {first}-{last} / {} result(s)",
                        results.hits.len()
                    )
                }));
            }
        }
        None => {
            lines.push(Line::from(""));
            lines.push(Line::from(tr(
                app,
                "Searching local transcript index and App Server…",
                "正在搜索本地会话索引和 App Server…",
            )));
        }
    }

    lines.extend([
        Line::from(""),
        Line::from(tr(
            app,
            "j/k move · Enter open exact result · Ctrl+F new search · Esc close",
            "j/k 移动 · Enter 打开精确结果 · Ctrl+F 重新搜索 · Esc 关闭",
        )),
    ]);

    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(24)
        .clamp(9, 26);
    let area = centered_fixed(96, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(tr(app, " Transcript Search ", " 会话全文搜索 ")))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(super) fn render_saved_view_editor(frame: &mut Frame<'_>, app: &AppState) {
    let Some(editor) = app.saved_view_editor.as_ref() else {
        return;
    };
    let draft = &editor.draft;
    let field_value = |index: usize| -> String {
        match crate::saved_view_editor::SavedViewEditorField::ALL[index] {
            crate::saved_view_editor::SavedViewEditorField::Name => draft.name.clone(),
            crate::saved_view_editor::SavedViewEditorField::SourceScope => {
                draft.source_scope.clone()
            }
            crate::saved_view_editor::SavedViewEditorField::Filter => {
                if draft.filter.is_empty() {
                    "<none>".into()
                } else {
                    draft.filter.clone()
                }
            }
            crate::saved_view_editor::SavedViewEditorField::GroupBy => {
                draft.group_by.clone().unwrap_or_else(|| "<none>".into())
            }
            crate::saved_view_editor::SavedViewEditorField::OrderBy => {
                draft.order_by.clone().unwrap_or_else(|| "<none>".into())
            }
            crate::saved_view_editor::SavedViewEditorField::Layout => {
                draft.layout.label().to_string()
            }
            crate::saved_view_editor::SavedViewEditorField::VisibleFields => {
                draft.visible_fields.join(",")
            }
        }
    };

    let mut lines = vec![Line::from(if editor.creating {
        tr(app, "New custom Saved View", "新建自定义已保存视图")
    } else {
        tr(app, "Edit custom Saved View", "编辑自定义已保存视图")
    })];
    lines.push(Line::from(""));

    for (index, field) in crate::saved_view_editor::SavedViewEditorField::ALL
        .iter()
        .enumerate()
    {
        let selected = index == editor.selected_field;
        let style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!(
                "{} {:<14} {}",
                if selected { ">" } else { " " },
                field.label(),
                truncate_display(&sanitize_inline(&field_value(index)), 70)
            ),
            style,
        )));
    }

    if let Some(error) = app.saved_view_editor_error.as_deref() {
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "{}: {}",
            tr(app, "Validation", "校验"),
            truncate_display(&sanitize_inline(error), 92)
        )));
    }

    lines.extend([
        Line::from(""),
        Line::from(tr(
            app,
            "j/k field · h/l cycle enum · Enter edit text field · s save · Esc cancel",
            "j/k 选择字段 · h/l 切换枚举 · Enter 编辑文本字段 · s 保存 · Esc 取消",
        )),
        Line::from(tr(
            app,
            "built-in views are immutable; Save as creates a local custom copy",
            "内置视图不可修改；另存为会创建本地自定义副本",
        )),
    ]);

    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(18)
        .clamp(12, 22);
    let area = centered_fixed(100, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(tr(app, " Saved View Editor ", " 已保存视图编辑器 ")))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(super) fn render_thread_queue(frame: &mut Frame<'_>, app: &AppState) {
    if !app.thread_queue_open {
        return;
    }

    let mut lines = Vec::new();
    if app.thread_queue_loading {
        lines.push(Line::from(tr(
            app,
            "Refreshing from Codex App Server…",
            "正在从 Codex App Server 刷新…",
        )));
    }
    if let Some(error) = app.thread_queue_error.as_deref() {
        lines.push(Line::from(format!(
            "{}: {}",
            tr(app, "Queue note", "队列提示"),
            truncate_display(&sanitize_inline(error), 92)
        )));
    }

    match app.thread_queue_snapshot.as_ref() {
        Some(snapshot) if snapshot.submissions.is_empty() => {
            lines.push(Line::from(""));
            lines.push(Line::from(tr(app, "Queue is empty.", "队列为空。")));
        }
        Some(snapshot) => {
            let start = app
                .thread_queue_selected
                .saturating_sub(8)
                .min(snapshot.submissions.len().saturating_sub(18));
            for (index, submission) in snapshot.submissions.iter().enumerate().skip(start).take(18)
            {
                let selected = index == app.thread_queue_selected;
                let style = if selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                let mode = if submission.editable_text.is_some() {
                    tr(app, "text", "文本")
                } else {
                    tr(app, "mixed", "混合")
                };
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} {:>2}. [{}] {}",
                        if selected { ">" } else { " " },
                        index + 1,
                        mode,
                        truncate_display(&sanitize_inline(&submission.summary), 78)
                    ),
                    style,
                )));
            }
            if snapshot.submissions.len() > 18 {
                lines.push(Line::from(format!(
                    "{} {}/{}",
                    tr(app, "Selected", "已选"),
                    app.thread_queue_selected + 1,
                    snapshot.submissions.len()
                )));
            }
        }
        None => {
            lines.push(Line::from(""));
            lines.push(Line::from(tr(
                app,
                "Queue has not been loaded yet.",
                "队列尚未加载。",
            )));
        }
    }

    lines.push(Line::from(""));
    if let Some(pending) = app.pending_thread_queue_mutation.as_ref() {
        lines.push(Line::from(if app.language.is_simplified_chinese() {
            format!("确认 {}？y 执行 · c/Esc 取消", pending.label())
        } else {
            format!("Confirm {}? y execute · c/Esc cancel", pending.label())
        }));
    } else {
        lines.push(Line::from(tr(
            app,
            "j/k move · n add · e edit text · [/] reorder · s start · x delete · r refresh · q/Esc close",
            "j/k 移动 · n 添加 · e 编辑文本 · [/] 重排 · s 启动 · x 删除 · r 刷新 · q/Esc 关闭",
        )));
        lines.push(Line::from(tr(
            app,
            "start/delete require confirmation; mixed-input items are intentionally not text-editable",
            "启动/删除需要确认；混合输入队列项不会按文本方式编辑",
        )));
    }

    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(24)
        .clamp(9, 27);
    let area = centered_fixed(100, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(tr(app, " Thread Queue ", " 会话队列 ")))
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
            "CONFIRM REQUIRED — no forge mutation has executed yet.",
            "需要确认 — 尚未执行任何 Forge 变更。",
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
