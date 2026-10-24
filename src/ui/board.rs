//! Single-row planning cards keep selection and scrollbar positions in terminal rows.
use super::*;
use crate::planning::{apply_saved_view, saved_view_group_key};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BoardViewport {
    pub(super) start: usize,
    pub(super) row_capacity: usize,
}

pub(super) fn board_viewport(total: usize, selected: usize, area_height: u16) -> BoardViewport {
    let row_capacity = area_height.saturating_sub(2) as usize;
    if total == 0 || row_capacity == 0 {
        return BoardViewport {
            start: 0,
            row_capacity,
        };
    }
    let selected = selected.min(total.saturating_sub(1));
    let max_start = total.saturating_sub(row_capacity);
    let start = selected.saturating_sub(row_capacity / 2).min(max_start);
    BoardViewport {
        start,
        row_capacity,
    }
}

fn render_board_scrollbar(
    frame: &mut Frame<'_>,
    area: Rect,
    total: usize,
    viewport: BoardViewport,
) {
    if total <= viewport.row_capacity || viewport.row_capacity == 0 || area.width == 0 {
        return;
    }
    let scrollbar_area = Rect {
        x: area.x.saturating_add(area.width.saturating_sub(1)),
        y: area.y.saturating_add(1),
        width: 1,
        height: area.height.saturating_sub(2),
    };
    if scrollbar_area.height == 0 {
        return;
    }
    let mut state = ScrollbarState::new(total)
        .position(viewport.start)
        .viewport_content_length(viewport.row_capacity);
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

pub(super) fn render_board(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(2)])
        .split(area);
    let view = app.active_saved_view();

    match view.layout {
        SavedViewLayout::Board if area.width >= 120 => {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Ratio(1, 5); 5])
                .split(outer[0]);
            let filtered = apply_saved_view(&app.work_cards, &view);

            for (stage_index, stage) in WorkflowStage::ALL.iter().enumerate() {
                let stage_cards = filtered
                    .iter()
                    .copied()
                    .filter(|card| card.stage == *stage)
                    .collect::<Vec<_>>();
                let selected = if stage_index == app.board_stage_index {
                    app.board_selected
                } else {
                    0
                };
                let viewport =
                    board_viewport(stage_cards.len(), selected, columns[stage_index].height);
                let lines = stage_cards
                    .iter()
                    .enumerate()
                    .skip(viewport.start)
                    .take(viewport.row_capacity)
                    .map(|(index, card)| {
                        let selected =
                            stage_index == app.board_stage_index && index == app.board_selected;
                        planning_card_line(
                            card,
                            selected,
                            app.language,
                            &view.visible_fields,
                            columns[stage_index].width.saturating_sub(2),
                        )
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(
                    Paragraph::new(lines).block(Block::bordered().title(format!(
                        " {} ({}) ",
                        workflow_stage_label(*stage, app.language),
                        stage_cards.len()
                    ))),
                    columns[stage_index],
                );
                render_board_scrollbar(frame, columns[stage_index], stage_cards.len(), viewport);
            }
        }
        SavedViewLayout::Board => {
            let stage = WorkflowStage::ALL[app.board_stage_index % WorkflowStage::ALL.len()];
            let cards = app.visible_planning_cards();
            let viewport = board_viewport(cards.len(), app.board_selected, outer[0].height);
            let lines = cards
                .iter()
                .enumerate()
                .skip(viewport.start)
                .take(viewport.row_capacity)
                .map(|(index, card)| {
                    planning_card_line(
                        card,
                        index == app.board_selected,
                        app.language,
                        &view.visible_fields,
                        outer[0].width.saturating_sub(2),
                    )
                })
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(lines).block(Block::bordered().title(format!(
                    " {} · {} · {}/{} ",
                    sanitize_inline(saved_view_name(&view, app.language)),
                    workflow_stage_label(stage, app.language),
                    app.board_stage_index + 1,
                    WorkflowStage::ALL.len()
                ))),
                outer[0],
            );
            render_board_scrollbar(frame, outer[0], cards.len(), viewport);
        }
        SavedViewLayout::List | SavedViewLayout::ReviewQueue => {
            let cards = app.visible_planning_cards();
            // Count card/group rows before formatting; offscreen cards never build
            // their display strings, links or metadata on each frame.
            let mut rows = Vec::with_capacity(cards.len());
            let mut previous_group = String::new();
            let mut selected_line = 0_usize;
            for (index, card) in cards.iter().enumerate() {
                let group = saved_view_group_key(card, view.group_by.as_deref());
                if !group.is_empty() && group != previous_group {
                    rows.push((index, true));
                    previous_group = group;
                }
                if index == app.board_selected {
                    selected_line = rows.len();
                }
                rows.push((index, false));
            }
            let total = rows.len();
            let viewport = board_viewport(total, selected_line, outer[0].height);
            let width = outer[0].width.saturating_sub(2);
            let mut visible_lines = rows
                .into_iter()
                .skip(viewport.start)
                .take(viewport.row_capacity)
                .map(|(index, is_group)| {
                    let card = cards[index];
                    if is_group {
                        let group = saved_view_group_key(card, view.group_by.as_deref());
                        Line::from(truncate_display(&format!("── {group} ──"), width as usize))
                    } else {
                        planning_card_line(
                            card,
                            index == app.board_selected,
                            app.language,
                            &view.visible_fields,
                            width,
                        )
                    }
                })
                .collect::<Vec<_>>();
            if cards.is_empty() {
                visible_lines.push(Line::from(tr(
                    app,
                    "No cards match this Saved View.",
                    "没有符合当前已保存视图的卡片。",
                )));
            }
            frame.render_widget(
                Paragraph::new(visible_lines).block(Block::bordered().title(format!(
                    " {} · {} ",
                    sanitize_inline(saved_view_name(&view, app.language)),
                    saved_view_layout_label(view.layout, app.language)
                ))),
                outer[0],
            );
            render_board_scrollbar(frame, outer[0], total, viewport);
        }
    }

    let input = if app.input_mode == InputMode::ScratchTitle {
        format!(
            "{}> {} · {}",
            tr(app, "new scratch", "新建 Scratch"),
            app.input_buffer,
            tr(app, "Enter create · Esc cancel", "Enter 创建 · Esc 取消")
        )
    } else if app.input_mode == InputMode::Snooze {
        format!(
            "{}> {} · 15m / 1h / 1d · {}",
            tr(app, "snooze", "稍后提醒"),
            app.input_buffer,
            tr(app, "Enter apply · Esc cancel", "Enter 应用 · Esc 取消")
        )
    } else if app.input_mode == InputMode::Note {
        format!(
            "{}> {} · {}",
            tr(app, "note", "备注"),
            truncate_display(&app.input_buffer, 80),
            tr(app, "Enter save · Esc cancel", "Enter 保存 · Esc 取消")
        )
    } else if app.input_mode == InputMode::SavedViewField {
        format!(
            "{}> {} · {}",
            tr(app, "view field", "视图字段"),
            truncate_display(&app.input_buffer, 80),
            tr(
                app,
                "Enter apply field · Esc cancel",
                "Enter 应用字段 · Esc 取消"
            )
        )
    } else if app.hot_slot_bind_pending.is_some() {
        tr(
            app,
            "bind hot slot: press 1–9 · Esc cancels other input only",
            "绑定快捷槽：按 1–9 · Esc 仅取消其他输入",
        )
        .into()
    } else if let Some(error) = &app.planning_store_error {
        format!(
            "{} · {}",
            tr(app, "LOCAL STORE DEGRADED", "本地存储已降级"),
            truncate_display(error, 80)
        )
    } else if app.language.is_simplified_chinese() {
        format!(
            "h/l 阶段 · j/k 项目 · Tab 视图 · Enter 打开 · Space 待处理 · s 稍后提醒 · = 绑定 · 1–9 跳转 · n Scratch · 视图 {}/{}",
            app.planning_view_index + 1,
            app.planning_views().len()
        )
    } else {
        format!(
            "h/l stage · j/k item · Tab view · Enter open · Space attention · s snooze · = bind · 1–9 jump · n scratch · view {}/{}",
            app.planning_view_index + 1,
            app.planning_views().len()
        )
    };
    frame.render_widget(Paragraph::new(sanitize_inline(&input)), outer[1]);
}

fn planning_card_line(
    card: &crate::planning::WorkCardProjection,
    selected: bool,
    language: UiLanguage,
    visible_fields: &[String],
    columns: u16,
) -> Line<'static> {
    let values = visible_fields
        .iter()
        .filter_map(|field| planning_card_field(card, field, language))
        .collect::<Vec<_>>();
    let metadata = values.join(" · ");
    let text = if metadata.is_empty() {
        format!(
            "{} {}",
            if selected { ">" } else { " " },
            sanitize_inline(&card.title)
        )
    } else {
        format!(
            "{} {} · {}",
            if selected { ">" } else { " " },
            sanitize_inline(&card.title),
            metadata
        )
    };
    let style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    Line::from(Span::styled(
        truncate_display(&text, columns as usize),
        style,
    ))
}

pub(super) fn planning_card_field(
    card: &crate::planning::WorkCardProjection,
    field: &str,
    language: UiLanguage,
) -> Option<String> {
    match field {
        "stage" => Some(workflow_stage_label(card.stage, language).to_string()),
        "attention" => Some(if card.needs_you() {
            card.attention
                .iter()
                .map(|reason| planning_attention_label(reason, language))
                .collect::<Vec<_>>()
                .join(",")
        } else if card.snoozed && !card.attention.is_empty() {
            tr_language(language, "snoozed", "已稍后提醒").into()
        } else {
            "-".into()
        }),
        "goal" => Some(
            card.goal
                .as_ref()
                .map(|goal| goal_status_label(goal.status, language).to_string())
                .unwrap_or_else(|| "-".into()),
        ),
        "source" => Some(
            match (card.anchor.kind.clone(), language) {
                (crate::planning::SourceKind::ScratchWork, UiLanguage::SimplifiedChinese) => "草稿",
                (crate::planning::SourceKind::CodexThread, UiLanguage::SimplifiedChinese) => "会话",
                (crate::planning::SourceKind::ForgeWorkItem, UiLanguage::SimplifiedChinese) => {
                    "Forge"
                }
                (crate::planning::SourceKind::ScratchWork, UiLanguage::English) => "scratch",
                (crate::planning::SourceKind::CodexThread, UiLanguage::English) => "thread",
                (crate::planning::SourceKind::ForgeWorkItem, UiLanguage::English) => "forge",
                _ => tr_language(language, "link", "链接"),
            }
            .into(),
        ),
        "workspace" => Some(
            card.workspace
                .as_deref()
                .map(sanitize_inline)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "-".into()),
        ),
        "branch" => Some(
            card.branch
                .as_deref()
                .map(sanitize_inline)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "-".into()),
        ),
        "forge" => Some(
            card.forge_provider
                .map(|provider| provider.label().to_string())
                .unwrap_or_else(|| "-".into()),
        ),
        "change-request" => Some(
            card.change_request_state
                .as_deref()
                .map(|state| {
                    let state = sanitize_inline(state);
                    if card.change_request_draft {
                        format!("{state}/draft")
                    } else {
                        state
                    }
                })
                .unwrap_or_else(|| "-".into()),
        ),
        "relationships" => Some(if card.links.is_empty() {
            "-".into()
        } else {
            card.links
                .iter()
                .map(|link| link.role.label())
                .collect::<Vec<_>>()
                .join(",")
        }),
        _ => None,
    }
}
