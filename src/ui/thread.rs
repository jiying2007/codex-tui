use super::{goal_status_label, goal_summary, interactive_request_lines, tr};
use crate::app::{AppState, InputMode};
use crate::conversation::ConversationState;
use crate::domain::ThreadSummary;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use std::sync::{Arc, LazyLock, Mutex};

const MIN_WINDOW_ITEMS: usize = 64;
const MAX_WINDOW_ITEMS: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
struct CachedItemPresentation {
    item_id: String,
    text: String,
}

#[derive(Clone)]
struct CachedConversationPresentation {
    thread_id: String,
    revision: u64,
    start: usize,
    end: usize,
    items: Arc<Vec<CachedItemPresentation>>,
}

static PRESENTATION_CACHE: LazyLock<Mutex<Option<CachedConversationPresentation>>> =
    LazyLock::new(|| Mutex::new(None));

fn formatted_items(
    conversation: &ConversationState,
    start: usize,
    end: usize,
) -> Arc<Vec<CachedItemPresentation>> {
    let mut cache = PRESENTATION_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(entry) = cache.as_ref()
        && entry.thread_id == conversation.thread_id.0
        && entry.revision == conversation.presentation_revision()
        && entry.start == start
        && entry.end == end
    {
        return Arc::clone(&entry.items);
    }

    let items = Arc::new(
        conversation.items[start..end]
            .iter()
            .map(|item| {
                let status = item
                    .status
                    .as_deref()
                    .map(|value| format!(" [{value}]"))
                    .unwrap_or_default();
                CachedItemPresentation {
                    item_id: item.item_id.clone(),
                    text: format!(
                        "{:>5}{status} {}",
                        item.kind.label(),
                        item.text.replace(['\n', '\r', '\t'], " ")
                    ),
                }
            })
            .collect::<Vec<_>>(),
    );

    *cache = Some(CachedConversationPresentation {
        thread_id: conversation.thread_id.0.clone(),
        revision: conversation.presentation_revision(),
        start,
        end,
        items: Arc::clone(&items),
    });
    items
}

fn item_window(total: usize, scroll: u16, area_height: u16) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    let visible_rows = area_height.saturating_sub(2) as usize;
    let capacity = visible_rows
        .saturating_mul(4)
        .clamp(MIN_WINDOW_ITEMS, MAX_WINDOW_ITEMS);
    let start = (scroll as usize).min(total.saturating_sub(1));
    let end = start.saturating_add(capacity).min(total);
    (start, end)
}

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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(4),
            Constraint::Length(4),
            Constraint::Length(1),
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
    let effective_scroll = if pending_request.is_some() {
        0
    } else {
        ui.scroll
    };

    let mut window_start = 0;
    let mut window_end = 0;
    let mut total_items = 0;
    let mut conversation_lines = match app.conversations.get(thread_id) {
        Some(conversation) if conversation.loading => vec![Line::from(tr(
            app,
            "Loading recent Codex history…",
            "正在加载最近的 Codex 历史…",
        ))],
        Some(conversation) if conversation.error.is_some() => {
            vec![Line::from(format!(
                "{}: {}",
                tr(app, "Conversation unavailable", "会话不可用"),
                conversation.error.as_deref().unwrap_or_else(|| tr(
                    app,
                    "unknown error",
                    "未知错误"
                ))
            ))]
        }
        Some(conversation) if conversation.items.is_empty() => vec![Line::from(tr(
            app,
            "No visible items in the loaded history page.",
            "已加载的历史页中没有可见内容。",
        ))],
        Some(conversation) => {
            total_items = conversation.items.len();
            (window_start, window_end) =
                item_window(total_items, effective_scroll, chunks[1].height);
            let cached = formatted_items(conversation, window_start, window_end);
            cached
                .iter()
                .map(|item| {
                    let is_search_target =
                        app.transcript_search_active_hit
                            .as_ref()
                            .is_some_and(|hit| {
                                hit.thread_id.0 == thread_id
                                    && hit.item_id.as_deref() == Some(item.item_id.as_str())
                            });
                    if is_search_target {
                        Line::styled(
                            item.text.clone(),
                            Style::default().add_modifier(Modifier::REVERSED),
                        )
                    } else {
                        Line::from(item.text.clone())
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

    if let Some(request) = pending_request {
        let mut request_lines = interactive_request_lines(request, app.language);
        request_lines.push(Line::from(""));
        request_lines.append(&mut conversation_lines);
        conversation_lines = request_lines;
    }

    let page_hint = app
        .conversations
        .get(thread_id)
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
    frame.render_widget(
        Paragraph::new(conversation_lines)
            .block(Block::bordered().title(format!(
                " {}{}{} ",
                tr(app, "Conversation", "会话"),
                range,
                older
            )))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
    render_item_scrollbar(
        frame,
        chunks[1],
        total_items,
        window_start,
        window_end.saturating_sub(window_start),
    );

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
                "{}\n{}={} {}={}",
                if ui.draft.is_empty() {
                    tr(app, "<empty>", "<空>")
                } else {
                    &ui.draft
                },
                tr(app, "item offset", "条目偏移"),
                ui.scroll,
                tr(app, "follow", "跟随"),
                ui.follow
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
            "a composer · q queue · g goal · m worktrees · y accept · n decline · c cancel · i answer · Ctrl+C interrupt",
            "a 编辑 · q 队列 · g 目标 · m worktree · y 接受 · n 拒绝 · c 取消 · i 回答 · Ctrl+C 中断",
        )),
        chunks[3],
    );
}

#[cfg(test)]
mod tests;
