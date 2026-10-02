use super::{
    goal_status_label, planning_attention_label, saved_view_layout_label, saved_view_name, tr,
    tr_language, workflow_stage_label,
};
use crate::app::{AppState, InputMode};
use crate::i18n::UiLanguage;
use crate::planning::{SavedViewLayout, SourceKind, WorkflowStage, saved_view_group_key};
use crate::text::{fit_display, sanitize_inline, truncate_display};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BoardViewport {
    start: usize,
    row_capacity: usize,
}

fn board_viewport(total: usize, selected: usize, area_height: u16) -> BoardViewport {
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

fn scroll_offset(start: usize) -> u16 {
    start.min(u16::MAX as usize) as u16
}

fn render_scrollbar(frame: &mut Frame<'_>, area: Rect, total: usize, viewport: BoardViewport) {
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
            let filtered = app.planning_cards_for_active_view();

            for (stage_index, stage) in WorkflowStage::ALL.iter().enumerate() {
                let stage_cards = filtered
                    .iter()
                    .copied()
                    .filter(|card| card.stage == *stage)
                    .collect::<Vec<_>>();
                let selected = (stage_index == app.board_stage_index).then_some(app.board_selected);
                let viewport = board_viewport(
                    stage_cards.len(),
                    selected.unwrap_or(0),
                    columns[stage_index].height,
                );
                let lines = stage_cards
                    .iter()
                    .enumerate()
                    .map(|(index, card)| {
                        let selected =
                            stage_index == app.board_stage_index && index == app.board_selected;
                        let attention = if card.needs_you() { "!" } else { " " };
                        let pin = if card.overlay.pinned { "*" } else { " " };
                        let metadata =
                            compact_card_metadata(card, &view.visible_fields, app.language);
                        let raw = if metadata.is_empty() {
                            format!(
                                "{}{}{} {}",
                                if selected { ">" } else { " " },
                                pin,
                                attention,
                                sanitize_inline(&card.title)
                            )
                        } else {
                            format!(
                                "{}{}{} {} · {}",
                                if selected { ">" } else { " " },
                                pin,
                                attention,
                                sanitize_inline(&card.title),
                                metadata
                            )
                        };
                        let text = truncate_display(
                            &raw,
                            columns[stage_index].width.saturating_sub(2) as usize,
                        );
                        let style = if selected {
                            Style::default().add_modifier(Modifier::REVERSED)
                        } else {
                            Style::default()
                        };
                        Line::from(Span::styled(text, style))
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(
                    Paragraph::new(lines)
                        .block(Block::bordered().title(format!(
                            " {} ({}) ",
                            workflow_stage_label(*stage, app.language),
                            stage_cards.len()
                        )))
                        .wrap(Wrap { trim: false })
                        .scroll((scroll_offset(viewport.start), 0)),
                    columns[stage_index],
                );
                render_scrollbar(frame, columns[stage_index], stage_cards.len(), viewport);
            }
        }
        SavedViewLayout::Board => {
            let stage = WorkflowStage::ALL[app.board_stage_index % WorkflowStage::ALL.len()];
            let cards = app.visible_planning_cards();
            let viewport = board_viewport(cards.len(), app.board_selected, outer[0].height);
            let lines = cards
                .iter()
                .enumerate()
                .map(|(index, card)| {
                    planning_card_line(
                        card,
                        index == app.board_selected,
                        app.language,
                        &view.visible_fields,
                    )
                })
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(lines)
                    .block(Block::bordered().title(format!(
                        " {} · {} · {}/{} ",
                        saved_view_name(&view, app.language),
                        workflow_stage_label(stage, app.language),
                        app.board_stage_index + 1,
                        WorkflowStage::ALL.len()
                    )))
                    .wrap(Wrap { trim: false })
                    .scroll((scroll_offset(viewport.start), 0)),
                outer[0],
            );
            render_scrollbar(frame, outer[0], cards.len(), viewport);
        }
        SavedViewLayout::List | SavedViewLayout::ReviewQueue => {
            let cards = app.visible_planning_cards();
            let mut lines = Vec::new();
            let mut previous_group = String::new();
            let mut selected_line = 0;
            for (index, card) in cards.iter().enumerate() {
                let group = saved_view_group_key(card, view.group_by.as_deref());
                if !group.is_empty() && group != previous_group {
                    lines.push(Line::from(format!("── {group} ──")));
                    previous_group = group;
                }
                if index == app.board_selected {
                    selected_line = lines.len();
                }
                lines.push(planning_card_line(
                    card,
                    index == app.board_selected,
                    app.language,
                    &view.visible_fields,
                ));
            }
            if lines.is_empty() {
                lines.push(Line::from(tr(
                    app,
                    "No cards match this Saved View.",
                    "没有符合当前已保存视图的卡片。",
                )));
            }
            let line_count = lines.len();
            let viewport = board_viewport(line_count, selected_line, outer[0].height);
            frame.render_widget(
                Paragraph::new(lines)
                    .block(Block::bordered().title(format!(
                        " {} · {} ",
                        saved_view_name(&view, app.language),
                        saved_view_layout_label(view.layout, app.language)
                    )))
                    .wrap(Wrap { trim: false })
                    .scroll((scroll_offset(viewport.start), 0)),
                outer[0],
            );
            render_scrollbar(frame, outer[0], line_count, viewport);
        }
    }

    let input = if app.input_mode == InputMode::Search {
        format!(
            "/{} · {}",
            app.input_buffer,
            tr(app, "Enter keep · Esc cancel", "Enter 保留 · Esc 取消")
        )
    } else if app.input_mode == InputMode::ScratchTitle {
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
    } else if app.input_mode == InputMode::SavedViewName {
        format!(
            "{}> {} · {}",
            tr(app, "view name", "视图名称"),
            truncate_display(&app.input_buffer, 80),
            tr(app, "Enter save · Esc cancel", "Enter 保存 · Esc 取消")
        )
    } else if app.hot_slot_bind_pending {
        tr(
            app,
            "bind hot slot: press 1–9 · Esc cancel",
            "绑定快捷槽：按 1–9 · Esc 取消",
        )
        .into()
    } else if app.link_hot_slot_pending.is_some() {
        tr(
            app,
            "link WorkCard: press 1–9 · Esc cancel",
            "关联 WorkCard：按 1–9 · Esc 取消",
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
    frame.render_widget(Paragraph::new(input), outer[1]);
}

fn attention_text(card: &crate::planning::WorkCardProjection, language: UiLanguage) -> String {
    if card.needs_you() {
        card.attention
            .iter()
            .map(|reason| planning_attention_label(reason, language))
            .collect::<Vec<_>>()
            .join(",")
    } else if card.snoozed && !card.attention.is_empty() {
        tr_language(language, "snoozed", "已稍后提醒").into()
    } else {
        "-".into()
    }
}

fn source_text(card: &crate::planning::WorkCardProjection, language: UiLanguage) -> &'static str {
    match (card.anchor.kind.clone(), language) {
        (SourceKind::ScratchWork, UiLanguage::SimplifiedChinese) => "草稿",
        (SourceKind::CodexThread, UiLanguage::SimplifiedChinese) => "会话",
        (SourceKind::ForgeWorkItem, UiLanguage::SimplifiedChinese) => "Forge",
        (SourceKind::ScratchWork, UiLanguage::English) => "scratch",
        (SourceKind::CodexThread, UiLanguage::English) => "thread",
        (SourceKind::ForgeWorkItem, UiLanguage::English) => "forge",
        _ => tr_language(language, "link", "链接"),
    }
}

fn saved_view_field_text(
    card: &crate::planning::WorkCardProjection,
    field: &str,
    language: UiLanguage,
) -> Option<String> {
    Some(match field {
        "stage" => fit_display(workflow_stage_label(card.stage, language), 7),
        "attention" => fit_display(&attention_text(card, language), 10),
        "workspace" => fit_display(card.workspace.as_deref().unwrap_or("-"), 14),
        "source" => fit_display(source_text(card, language), 8),
        "goal" => fit_display(
            card.goal
                .as_ref()
                .map(|goal| goal_status_label(goal.status, language))
                .unwrap_or("-"),
            12,
        ),
        "priority" => fit_display(
            &card
                .overlay
                .priority
                .map(|priority| format!("p={priority}"))
                .unwrap_or_else(|| "p=-".into()),
            7,
        ),
        "branch" => fit_display(card.branch.as_deref().unwrap_or("-"), 14),
        "forge" => fit_display(
            card.forge_provider
                .map(|provider| provider.label())
                .unwrap_or("-"),
            8,
        ),
        _ => return None,
    })
}

fn compact_card_metadata(
    card: &crate::planning::WorkCardProjection,
    visible_fields: &[String],
    language: UiLanguage,
) -> String {
    visible_fields
        .iter()
        .filter(|field| field.as_str() != "stage")
        .filter_map(|field| saved_view_field_text(card, field, language))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value != "-")
        .collect::<Vec<_>>()
        .join(" · ")
}

fn planning_card_line(
    card: &crate::planning::WorkCardProjection,
    selected: bool,
    language: UiLanguage,
    visible_fields: &[String],
) -> Line<'static> {
    let metadata = visible_fields
        .iter()
        .filter_map(|field| saved_view_field_text(card, field, language))
        .collect::<Vec<_>>()
        .join(" ");
    let text = if metadata.is_empty() {
        format!(
            "{} {}",
            if selected { ">" } else { " " },
            sanitize_inline(&card.title)
        )
    } else {
        format!(
            "{} {} {}",
            if selected { ">" } else { " " },
            metadata,
            sanitize_inline(&card.title)
        )
    };
    let style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    Line::from(Span::styled(text, style))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_keeps_large_selection_visible() {
        let viewport = board_viewport(100, 73, 12);
        assert!(73 >= viewport.start);
        assert!(73 < viewport.start + viewport.row_capacity);
        assert_eq!(viewport.row_capacity, 10);
    }

    #[test]
    fn viewport_stays_zero_when_content_fits() {
        assert_eq!(
            board_viewport(5, 4, 12),
            BoardViewport {
                start: 0,
                row_capacity: 10,
            }
        );
    }
}
