use super::*;
mod coverage;
mod groups;
use coverage::registry_inventory_label;
use groups::display_group_title;

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
    let group_info = app.registry_group_projection();
    let attention_count = visible
        .iter()
        .filter(|index| app.thread_needs_attention(**index))
        .count();
    let unlocated = app.unlocated_pending_request_count();
    // Show unresolved live requests before the recent/history scope even on SSH.
    // They are NOT synthetic thread rows and cannot authorize a resume/approval.
    let unlocated_label = match (unlocated, app.language.is_simplified_chinese()) {
        (0, _) => String::new(),
        (count, true) => format!("!{count} 请求待定位 · "),
        (1, false) => "!1 request unlocated · ".into(),
        (count, false) => format!("!{count} requests unlocated · "),
    };
    let mut lines = Vec::with_capacity(viewport.row_capacity.saturating_add(1));
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
    let history = registry_inventory_label(app);
    let matched = viewport.matched;
    // Keep default chrome about the next action, not filesystem diagnostics.
    // Detailed locality and upstream provenance remain in the selected panel/Doctor.
    let summary = if app.language.is_simplified_chinese() {
        format!(
            "{unlocated_label}待处理 {attention_count} · {scope}{history} · 显示 {}/{} · 共 {} · {range}{text_filter}",
            visible.len(),
            matched,
            app.threads.len()
        )
    } else {
        format!(
            "{unlocated_label}{attention_count} need attention · {scope}{history} · showing {}/{} · {} total · {range}{text_filter}",
            visible.len(),
            matched,
            app.threads.len()
        )
    };
    lines.push(Line::from(summary));
    if visible.is_empty() && !app.threads.is_empty() {
        let hint = tr(app, "No matches", "无匹配");
        lines.push(Line::from(format!("{hint} · / l g ?")));
    }

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
        let observed = app.session_attention_reasons(thread);
        let snoozed_attention =
            (!observed.is_empty() || pending_interactive) && app.session_attention_snoozed(thread);
        let attention = if needs_attention {
            let mut reasons = thread
                .attention
                .iter()
                .map(|reason| attention_reason_label(reason, app.language))
                .collect::<Vec<_>>();
            for reason in observed.iter().filter(|reason| {
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
                display_group_title(app, index, &group_info)
            ),
            LayoutMode::Standard | LayoutMode::Wide => format!(
                "{prefix}{pin}{collision}{locality} {} {} {} {}",
                fit_display(runtime_status_label(&thread.runtime, app.language), 8),
                fit_display(&thread.workspace, 18),
                fit_display(&attention, 10),
                display_group_title(app, index, &group_info)
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
        tr(app, "Space / r w Alt+T ?", "Space / r w Alt+T ?")
    } else if width < 80 {
        tr(
            app,
            "Space needs-you / search r w Alt+T 1–9 ?",
            "Space 待办 / 搜索 r w Alt+T 1–9 快切 ?",
        )
    } else {
        tr(
            app,
            "Space next / search Enter open r Review w Workspace Alt+T terminal 1–9 jump ?",
            "Space 待办 / 搜索 Enter 打开 r 评审 w 工作区 Alt+T 终端 1–9 快切 ?",
        )
    };
    Line::from(truncate_display(hint, usize::from(width)))
}

#[cfg(test)]
mod tests;
