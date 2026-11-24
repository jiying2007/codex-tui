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
    widgets::{Block, Clear, List, ListState, Paragraph, Wrap},
};

pub(super) fn render_command_palette(frame: &mut Frame<'_>, app: &AppState) {
    let matches = app.command_palette_matches();
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("{}> ", tr(app, "Query", "搜索")),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(if app.command_palette_query.is_empty() {
            tr(app, "<type to show all commands>", "<输入以搜索全部命令>").to_string()
        } else {
            sanitize_inline(&app.command_palette_query)
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
                spans.push(Span::styled(sanitize_inline(&character.to_string()), style));
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
    let height = lines.len().saturating_add(2).clamp(7, 26) as u16;
    let area = centered_fixed(86, height, frame.area());
    let selected =
        (app.command_palette_selected < matches.len()).then(|| app.command_palette_selected + 1);
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(
        List::new(lines).block(Block::bordered().title(tr(app, " Command Palette ", " 命令面板 "))),
        area,
        &mut ListState::default().with_selected(selected),
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
    let height = lines.len().saturating_add(2).clamp(6, 24) as u16;
    let area = centered_fixed(64, height, frame.area());
    let selected = (app.context_selected < choices.len()).then_some(app.context_selected);
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(
        List::new(lines).block(Block::bordered().title(tr(app, " Actions ", " 操作 "))),
        area,
        &mut ListState::default().with_selected(selected),
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
        InputMode::Note => (
            tr(app, " Local note ", " 本地备注 "),
            tr(app, "Enter save · Esc cancel", "Enter 保存 · Esc 取消"),
        ),
        InputMode::WorkrefLink => (
            tr(
                app,
                " Link session to existing work item ",
                " 将会话关联到已有事项 ",
            ),
            if app.workref_link_pending {
                tr(
                    app,
                    "SQLite write accepted · waiting for commit receipt; cannot cancel",
                    "SQLite 写入已接收 · 等待提交回执；当前不能取消",
                )
            } else {
                tr(
                    app,
                    "Enter an existing local key · alias is derived from repository · Esc cancel",
                    "输入已有事项 Key · 会话别名按仓库自动生成 · Esc 取消",
                )
            },
        ),
        InputMode::Snooze => (
            tr(app, " Snooze ", " 稍后提醒 "),
            tr(
                app,
                "15m / 1h / 1d · Enter apply · Esc cancel",
                "15m / 1h / 1d · Enter 应用 · Esc 取消",
            ),
        ),
        _ => return,
    };
    let mut lines = vec![
        Line::from(sanitize_inline(&app.input_buffer)),
        Line::from(""),
        Line::from(hint),
    ];
    if app.input_mode == InputMode::WorkrefLink {
        for (key, item) in app.workref_link_items() {
            lines.push(Line::from(sanitize_inline(&format!(
                "{} · {} · {} / {}",
                key, item.provider, item.space, item.external_id
            ))));
        }
        if app.workref_ledger.items.len() > 5 {
            lines.push(Line::from(tr(
                app,
                "… enter the exact key to select another item",
                "… 其他事项请输入准确 Key",
            )));
        }
    }
    let height = lines.len().saturating_add(2).clamp(7, 14) as u16;
    let area = centered_fixed(72, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
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
