use super::{goal_status_label, goal_summary, interactive_request_lines, tr};
use crate::app::{AppState, InputMode};
use crate::domain::ThreadSummary;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
fn render_item_scrollbar(
    frame: &mut Frame<'_>,
    area: Rect,
    total: usize,
    start: usize,
    visible: usize,
) {
    if total <= visible || visible == 0 || area.width == 0 || area.height <= 2 {
        return;
    }
    let scrollbar_area = Rect {
        x: area.x.saturating_add(area.width.saturating_sub(1)),
        y: area.y.saturating_add(1),
        width: 1,
        height: area.height.saturating_sub(2),
    };
    let mut state = ScrollbarState::new(total)
        .position(start)
        .viewport_content_length(visible);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_symbol("█")
            .track_symbol(Some("│")),
        scrollbar_area,
        &mut state,
    );
}

pub(super) fn render_thread(frame: &mut Frame<'_>, app: &AppState, thread_id: &str, area: Rect) {
    if app.request_details_open() {
        request_details::render(frame, app, area);
        return;
    }
    let (header_height, composer_height) = if area.height >= 14 {
        (5, 4)
    } else if area.height >= 9 {
        (3, 2)
    } else {
        (0, 0)
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(1),
            Constraint::Length(composer_height),
            Constraint::Length(area.height.saturating_sub(1).min(1)),
        ])
        .split(area);

    let thread = app.threads.iter().find(|thread| thread.id.0 == thread_id);
    let title = thread
        .map(ThreadSummary::display_title)
        .unwrap_or_else(|| tr(app, "Unknown thread", "未知会话"));
    frame.render_widget(
        Paragraph::new(format!(
            "{title}\n{thread_id}\n{}: {}",
            tr(app, "Goal", "目标"),
            goal_summary(app, thread_id)
        ))
        .block(Block::bordered().title(tr(app, " Thread ", " 会话 "))),
        chunks[0],
    );

    let ui = app.thread_ui.get(thread_id).cloned().unwrap_or_default();
    let pending_request = app.current_pending_request();
    let body_has_border = chunks[1].width >= 3 && chunks[1].height >= 3;
    let border_size = if body_has_border { 2 } else { 0 };
    let (body_width, body_height) = (
        chunks[1].width.saturating_sub(border_size),
        chunks[1].height.saturating_sub(border_size),
    );
    // Keep the pending request visible without stealing all navigation rows.
    // Count actual wrapped request rows before sizing the history viewport.
    let mut request_lines = Vec::new();
    let request_limit = usize::from(body_height.saturating_sub(1));
    if let Some(request) = pending_request {
        for line in interactive_request_lines(request, app.language) {
            let text = line.to_string();
            for row in crate::conversation_viewport::WrappedRows::new(&text, body_width, 0)
                .take(request_limit.saturating_sub(request_lines.len()))
            {
                request_lines.push(Line::from(text[row].to_owned()));
            }
            if request_lines.len() == request_limit {
                break;
            }
        }
        if request_lines.len() < request_limit {
            request_lines.push(Line::from(""));
        }
    }
    let history_height = body_height.saturating_sub(request_lines.len() as u16);
    app.thread_viewport_size.set((body_width, history_height));
    let mut scrollbar = (0, 0, 0);

    let mut window_start = 0;
    let mut window_end = 0;
    let mut total_items = 0;
    let mut conversation_lines = match app.displayed_conversation(thread_id) {
        Some(conversation) if conversation.loading => vec![Line::from(tr(
            app,
            "Loading recent Codex history…",
            "正在加载最近的 Codex 历史…",
        ))],
        Some(conversation) if conversation.error.is_some() => {
            vec![Line::from(format!(
                "{}: {}",
                tr(app, "Conversation unavailable", "会话不可用"),
                crate::text::sanitize_inline(conversation.error.as_deref().unwrap_or_else(|| tr(
                    app,
                    "unknown error",
                    "未知错误"
                )))
            ))]
        }
        Some(conversation) if conversation.items.is_empty() => vec![Line::from(tr(
            app,
            "No visible items in the loaded history page.",
            "已加载的历史页中没有可见内容。",
        ))],
        Some(conversation) => {
            total_items = conversation.items.len();
            let view = crate::conversation_viewport::viewport(
                conversation,
                &ui,
                body_width,
                history_height,
            );
            (window_start, window_end) = (view.start, view.end);
            scrollbar = view.scrollbar();
            view.visible()
                .into_iter()
                .map(|(turn_id, item_id, text)| {
                    let is_search_target =
                        app.transcript_search_active_hit
                            .as_ref()
                            .is_some_and(|hit| {
                                hit.thread_id.0 == thread_id
                                    && hit.item_id.as_deref() == Some(item_id)
                                    && hit.turn_id.as_deref().is_none_or(|turn| turn == turn_id)
                            });
                    if is_search_target {
                        Line::styled(
                            text.to_owned(),
                            Style::default().add_modifier(Modifier::REVERSED),
                        )
                    } else {
                        Line::from(text.to_owned())
                    }
                })
                .collect()
        }
        None => vec![Line::from(tr(
            app,
            "Conversation has not been loaded yet.",
            "会话尚未加载。",
        ))],
    };

    if pending_request.is_some() {
        request_lines.append(&mut conversation_lines);
        conversation_lines = request_lines;
    }

    let page_hint = app
        .displayed_conversation(thread_id)
        .is_some_and(|conversation| {
            conversation.next_turn_cursor.is_some() || conversation.next_item_cursor.is_some()
        });
    let range = if total_items > 0 {
        format!(
            " · {} {}-{}/{}",
            tr(app, "items", "条目"),
            window_start + 1,
            window_end,
            total_items
        )
    } else {
        String::new()
    };
    let older = if page_hint {
        tr(app, " · older history available", " · 可加载更早历史")
    } else {
        ""
    };
    let paused = if app.is_reading_snapshot(thread_id) {
        tr(
            app,
            " · paused reading · End latest",
            " · 暂停阅读 · End 最新",
        )
    } else {
        ""
    };
    let body_block = if body_has_border {
        Block::bordered().title(format!(
            " {}{}{}{} ",
            tr(app, "Conversation", "会话"),
            paused,
            range,
            older
        ))
    } else {
        Block::default()
    };
    frame.render_widget(
        Paragraph::new(conversation_lines)
            .block(body_block)
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
    render_item_scrollbar(frame, chunks[1], scrollbar.0, scrollbar.1, scrollbar.2);

    let (composer_title, composer_text) = if app.input_mode == InputMode::GoalObjective {
        (
            tr(
                app,
                " Goal objective · Enter set · Esc cancel ",
                " Goal 目标 · Enter 设置 · Esc 取消 ",
            ),
            format!("{}> {}", tr(app, "objective", "目标"), app.input_buffer),
        )
    } else if app.goal_actions_open {
        let text = app.current_goal().map_or_else(
            || {
                tr(
                    app,
                    "No Goal observed. e/Enter creates one with ACTIVE status.",
                    "尚未发现 Goal。按 e/Enter 创建并设为 ACTIVE。",
                )
                .into()
            },
            |goal| {
                if app.language.is_simplified_chinese() {
                    format!(
                        "{}\n状态={} · token={}{} · 已用={}秒",
                        goal.objective,
                        goal_status_label(goal.status, app.language),
                        goal.tokens_used,
                        goal.token_budget
                            .map(|budget| format!("/{budget}"))
                            .unwrap_or_default(),
                        goal.time_used_seconds
                    )
                } else {
                    format!(
                        "{}\nstatus={} · tokens={}{} · elapsed={}s",
                        goal.objective,
                        goal_status_label(goal.status, app.language),
                        goal.tokens_used,
                        goal.token_budget
                            .map(|budget| format!("/{budget}"))
                            .unwrap_or_default(),
                        goal.time_used_seconds
                    )
                }
            },
        );
        (
            tr(
                app,
                " Goal actions · e objective · p pause · r resume · c clear · Esc close ",
                " Goal 操作 · e 目标 · p 暂停 · r 恢复 · c 清除 · Esc 关闭 ",
            ),
            text,
        )
    } else if app.input_mode == InputMode::UserInput {
        let question = app.current_user_input_question();
        let displayed_answer = if question.is_none_or(|question| question.is_secret) {
            "*".repeat(app.input_buffer.chars().count())
        } else {
            app.input_buffer.clone()
        };
        (
            tr(
                app,
                " User input · Enter next/send · Esc cancel editor ",
                " 用户输入 · Enter 下一项/发送 · Esc 取消编辑 ",
            ),
            format!(
                "{}\n{}> {}",
                question
                    .map(|question| question.question.as_str())
                    .unwrap_or_else(|| tr(app, "Question unavailable", "问题不可用")),
                tr(app, "answer", "回答"),
                displayed_answer
            ),
        )
    } else {
        (
            if app.input_mode == InputMode::Composer {
                tr(
                    app,
                    " Composer · Enter send · Esc keep draft ",
                    " 编辑消息 · Enter 发送 · Esc 保留草稿 ",
                )
            } else {
                tr(app, " Draft · a edit ", " 草稿 · a 编辑 ")
            },
            format!(
                "{}\n{}",
                if ui.draft.is_empty() {
                    tr(app, "<empty>", "<空>")
                } else {
                    &ui.draft
                },
                if app.is_reading_snapshot(thread_id) {
                    tr(
                        app,
                        "Paused reading · End for latest",
                        "暂停阅读 · End 查看最新",
                    )
                } else if ui.follow {
                    tr(
                        app,
                        "Following latest · PageUp to browse",
                        "跟随最新 · PageUp 浏览历史",
                    )
                } else {
                    tr(
                        app,
                        "Browsing history · End to follow latest",
                        "浏览历史 · End 跟随最新",
                    )
                }
            ),
        )
    };
    frame.render_widget(
        Paragraph::new(composer_text).block(Block::bordered().title(composer_title)),
        chunks[2],
    );
    frame.render_widget(
        Paragraph::new(tr(
            app,
            if pending_request.is_some() {
                "d full details · PgUp/PgDn scroll · Home start · End follow · y/n/c approval · i answer"
            } else {
                "PgUp/PgDn scroll · Home start · End follow · a composer · q queue · ? help"
            },
            if pending_request.is_some() {
                "d 完整详情 · PgUp/PgDn 滚动 · Home 开头 · End 跟随 · y/n/c 审批 · i 回答"
            } else {
                "PgUp/PgDn 滚动 · Home 开头 · End 跟随 · a 编辑 · q 队列 · ? 帮助"
            },
        )),
        chunks[3],
    );
}

mod request_details;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod request_details_tests;
