use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RegistryViewport {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) total: usize,
    pub(super) matched: usize,
    pub(super) row_capacity: usize,
}

pub(super) fn registry_viewport(
    app: &AppState,
    area_height: u16,
) -> (Vec<usize>, RegistryViewport) {
    let (visible, matched) = app.visible_indices_with_match_count();
    let total = visible.len();
    let row_capacity = usize::from(area_height.saturating_sub(3));
    if total == 0 || row_capacity == 0 {
        return (
            visible,
            RegistryViewport {
                start: 0,
                end: 0,
                total,
                matched,
                row_capacity,
            },
        );
    }

    let selected_position = visible
        .iter()
        .position(|index| *index == app.selected)
        .unwrap_or(0);
    let start = selected_position
        .saturating_sub(row_capacity.saturating_sub(1))
        .min(total.saturating_sub(row_capacity));
    let end = (start + row_capacity).min(total);

    (
        visible,
        RegistryViewport {
            start,
            end,
            total,
            matched,
            row_capacity,
        },
    )
}

pub(super) fn registry_history_label(
    language: UiLanguage,
    hydrating: bool,
    has_filter: bool,
    show_all_history: bool,
) -> &'static str {
    if language.is_simplified_chinese() {
        match (hydrating, has_filter, show_all_history) {
            (true, true, _) => "搜索（历史加载中）",
            (true, false, true) => "全部历史（加载中）",
            (true, false, false) => "最近（历史加载中）",
            (false, true, _) => "搜索全部",
            (false, false, true) => "全部历史",
            (false, false, false) => "最近",
        }
    } else {
        match (hydrating, has_filter, show_all_history) {
            (true, true, _) => "SEARCH PARTIAL · HYDRATING",
            (true, false, true) => "ALL HISTORY · HYDRATING",
            (true, false, false) => "RECENT · HYDRATING",
            (false, true, _) => "SEARCH ALL",
            (false, false, true) => "ALL HISTORY",
            (false, false, false) => "RECENT",
        }
    }
}

pub(super) fn render_thread_list(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let (paragraph, viewport) = thread_list(app, area);
    frame.render_widget(paragraph, area);

    if viewport.total <= viewport.row_capacity || viewport.row_capacity == 0 || area.width == 0 {
        return;
    }

    let scrollbar_area = Rect {
        x: area.x.saturating_add(area.width.saturating_sub(1)),
        y: area.y.saturating_add(2),
        width: 1,
        height: area.height.saturating_sub(3),
    };
    if scrollbar_area.height == 0 {
        return;
    }

    let mut scrollbar_state = ScrollbarState::new(viewport.total)
        .position(viewport.start)
        .viewport_content_length(viewport.row_capacity);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_symbol("█")
            .track_symbol(Some("│")),
        scrollbar_area,
        &mut scrollbar_state,
    );
}

fn thread_list(app: &AppState, area: Rect) -> (Paragraph<'static>, RegistryViewport) {
    let (visible, viewport) = registry_viewport(app, area.height);
    let attention_count = visible
        .iter()
        .filter(|index| app.thread_needs_attention(**index))
        .count();
    let mut lines = Vec::with_capacity(viewport.row_capacity.saturating_add(1));
    let (local_count, foreign_count, stale_count, unprobed_count) =
        visible
            .iter()
            .fold((0_usize, 0_usize, 0_usize, 0_usize), |mut counts, index| {
                match app.cwd_locality_for_display(&app.threads[*index].metadata.cwd) {
                    Some(CwdLocality::LocalDirectory) => counts.0 += 1,
                    Some(CwdLocality::ForeignWindows | CwdLocality::ForeignUnix) => counts.1 += 1,
                    Some(CwdLocality::NativeMissing) => counts.2 += 1,
                    Some(CwdLocality::Relative | CwdLocality::Empty) => {}
                    None => counts.3 += 1,
                }
                counts
            });
    let range = if viewport.total == 0 {
        tr(app, "rows 0/0", "行 0/0").to_string()
    } else if app.language.is_simplified_chinese() {
        format!(
            "行 {}-{}/{}",
            viewport.start + 1,
            viewport.end,
            viewport.total
        )
    } else {
        format!(
            "rows {}-{}/{}",
            viewport.start + 1,
            viewport.end,
            viewport.total
        )
    };
    let scope = if app.language.is_simplified_chinese() {
        match (app.host_local_only, app.repo_backed_only) {
            (true, true) => "仅本机+仓库 · ",
            (true, false) => "仅本机 · ",
            (false, true) => "仅仓库 · ",
            (false, false) => "",
        }
    } else {
        match (app.host_local_only, app.repo_backed_only) {
            (true, true) => "LOCAL+REPO ONLY · ",
            (true, false) => "LOCAL ONLY · ",
            (false, true) => "REPO ONLY · ",
            (false, false) => "",
        }
    };
    let text_filter = if app.filter.is_empty() {
        String::new()
    } else if app.language.is_simplified_chinese() {
        format!(" · 筛选: {}", app.filter)
    } else {
        format!(" · filter: {}", app.filter)
    };
    let history = registry_history_label(
        app.language,
        app.backend_status.connected && !app.backend_status.registry_complete,
        !app.filter.is_empty(),
        app.show_all_history,
    );
    let matched = viewport.matched;
    let summary = if app.language.is_simplified_chinese() {
        format!(
            "待处理 {attention_count} · 未探测 {unprobed_count} · {scope}{history} · 显示 {}/{} 匹配 · 共 {} · 本机 {local_count} · 外部 {foreign_count} · 失效 {stale_count} · {range}{text_filter}",
            visible.len(),
            matched,
            app.threads.len()
        )
    } else {
        format!(
            "{attention_count} need attention · {unprobed_count} unprobed · {scope}{history} {}/{} matched · {} total · {local_count} local · {foreign_count} foreign · {stale_count} stale · {range}{text_filter}",
            visible.len(),
            matched,
            app.threads.len()
        )
    };
    lines.push(Line::from(summary));

    for index in visible[viewport.start..viewport.end].iter().copied() {
        let thread = &app.threads[index];
        let selected = index == app.selected;
        let prefix = if selected { ">" } else { " " };
        let pin = if thread.pinned { "*" } else { " " };
        let pending_interactive = app
            .pending_requests
            .iter()
            .any(|request| request.thread_id == thread.id);
        let needs_attention = app.thread_needs_attention(index);
        let card = app.work_card_for_thread(&thread.id);
        let snoozed_attention = (!thread.attention.is_empty()
            || pending_interactive
            || card.is_some_and(|card| !card.attention.is_empty()))
            && card.is_some_and(|card| card.snoozed);
        let attention = if needs_attention {
            let mut reasons = thread
                .attention
                .iter()
                .map(|reason| attention_reason_label(reason, app.language))
                .collect::<Vec<_>>();
            if let Some(card) = card {
                for reason in card.attention.iter().filter(|reason| {
                    matches!(
                        reason,
                        PlanningAttention::ConflictRisk
                            | PlanningAttention::GoalBlocked
                            | PlanningAttention::UsageLimited
                            | PlanningAttention::BudgetLimited
                            | PlanningAttention::PipelineFailed
                            | PlanningAttention::ChangeRequested
                            | PlanningAttention::ReviewUnseen
                    )
                }) {
                    let label = planning_attention_label(reason, app.language);
                    if !reasons.contains(&label) {
                        reasons.push(label);
                    }
                }
            }
            if pending_interactive {
                reasons.push(tr(app, "interactive", "交互"));
            }
            reasons.join(",")
        } else if snoozed_attention {
            tr(app, "snoozed", "稍后").into()
        } else if thread.attention.is_empty() {
            "-".into()
        } else {
            tr(app, "ack", "已处理").into()
        };
        let collision = if app.worktree_collision_count(&thread.id) > 0 {
            "!"
        } else {
            " "
        };
        let locality = match app.cwd_locality_for_display(&thread.metadata.cwd) {
            Some(CwdLocality::LocalDirectory) => "L",
            Some(CwdLocality::ForeignWindows | CwdLocality::ForeignUnix) => "F",
            Some(CwdLocality::NativeMissing) => "!",
            Some(CwdLocality::Relative | CwdLocality::Empty) | None => "?",
        };
        let text = match layout_mode(area.width) {
            LayoutMode::Compact => format!(
                "{prefix}{pin}{collision}{locality} {} {} {}",
                fit_display(
                    if needs_attention || snoozed_attention {
                        &attention
                    } else {
                        runtime_status_label(&thread.runtime, app.language)
                    },
                    8,
                ),
                fit_display(&thread.workspace, 11),
                sanitize_inline(thread.display_title())
            ),
            LayoutMode::Standard | LayoutMode::Wide => format!(
                "{prefix}{pin}{collision}{locality} {} {} {} {}",
                fit_display(runtime_status_label(&thread.runtime, app.language), 8),
                fit_display(&thread.workspace, 18),
                fit_display(&attention, 10),
                sanitize_inline(thread.display_title())
            ),
        };
        let style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(text, style)));
    }

    (
        Paragraph::new(lines).block(Block::bordered().title(registry_title(app, area.width))),
        viewport,
    )
}

pub(super) fn registry_footer(app: &AppState, width: u16) -> Line<'static> {
    // Keep the most useful actions visible before less-frequent features.
    let hint = if width < 40 {
        tr(app, "Space b r w ?", "Space b r w ?")
    } else if width < 80 {
        tr(
            app,
            "Space needs-you  1–9 jump  b r w  / search  ? help",
            "Space 待办  1–9 快切  b r w  / 搜索  ? 帮助",
        )
    } else {
        tr(
            app,
            "Space needs-you 1–9 / search ? b Board r Review w Workspace t terminal l local-only g repo-only",
            "Space 待办 1–9 / 搜索 ? b 看板 r 评审 w 工作区 t 终端 l 仅本机 g 仅仓库",
        )
    };
    Line::from(truncate_display(hint, usize::from(width)))
}

#[cfg(test)]
mod tests;
