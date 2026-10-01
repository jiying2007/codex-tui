use crate::app::{AppState, ContextChoice, InputMode, View};
use crate::conversation::{InteractiveRequest, InteractiveRequestKind};
use crate::domain::{CwdLocality, ThreadSummary, classify_cwd, display_cwd};
use crate::git::presentation_diff_lines;
use crate::i18n::{UiLanguage, pick};
use crate::planning::{SavedViewLayout, WorkflowStage, apply_saved_view, saved_view_group_key};
use crate::pty::TerminalSize;
use crate::text::{fit_display, sanitize_inline, truncate_display};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutMode {
    Compact,
    Standard,
    Wide,
}

pub const fn layout_mode(width: u16) -> LayoutMode {
    if width < 80 {
        LayoutMode::Compact
    } else if width < 120 {
        LayoutMode::Standard
    } else {
        LayoutMode::Wide
    }
}

fn tr<'a>(app: &AppState, english: &'a str, simplified_chinese: &'a str) -> &'a str {
    pick(app.language, english, simplified_chinese)
}

fn tr_language(
    language: UiLanguage,
    english: &'static str,
    simplified_chinese: &'static str,
) -> &'static str {
    pick(language, english, simplified_chinese)
}

fn workflow_stage_label(stage: WorkflowStage, language: UiLanguage) -> &'static str {
    match (stage, language) {
        (WorkflowStage::Inbox, UiLanguage::SimplifiedChinese) => "收件箱",
        (WorkflowStage::Ready, UiLanguage::SimplifiedChinese) => "就绪",
        (WorkflowStage::Working, UiLanguage::SimplifiedChinese) => "进行中",
        (WorkflowStage::Review, UiLanguage::SimplifiedChinese) => "评审",
        (WorkflowStage::Done, UiLanguage::SimplifiedChinese) => "完成",
        _ => stage.label(),
    }
}

fn context_choice_label(choice: ContextChoice, language: UiLanguage) -> &'static str {
    if language == UiLanguage::English {
        return choice.label();
    }
    match choice {
        ContextChoice::Snooze => "稍后提醒…",
        ContextChoice::EditNote => "编辑本地备注…",
        ContextChoice::Bookmark => "添加书签",
        ContextChoice::ScratchInbox => "Scratch → 收件箱",
        ContextChoice::ScratchReady => "Scratch → 就绪",
        ContextChoice::ScratchDone => "Scratch → 完成",
        ContextChoice::DeleteScratch => "删除 ScratchWork",
        ContextChoice::SaveCurrentView => "保存当前视图…",
        ContextChoice::DeleteCurrentView => "删除当前已保存视图",
        ContextChoice::BatchAddTag => "批量当前可见项 · 添加标签…",
        ContextChoice::BatchRemoveTag => "批量当前可见项 · 移除标签…",
        ContextChoice::BatchSetPriority => "批量当前可见项 · 设置优先级…",
        ContextChoice::BatchClearPriority => "批量当前可见项 · 清除优先级",
        ContextChoice::BatchMarkReady => "批量当前可见项 · 标记就绪",
        ContextChoice::BatchClearReady => "批量当前可见项 · 清除就绪",
        ContextChoice::BatchMarkDone => "批量当前可见项 · 确认完成",
        ContextChoice::BatchReopen => "批量当前可见项 · 重新打开",
        ContextChoice::BatchSnooze => "批量当前可见项 · 稍后提醒…",
        ContextChoice::BatchClearSnooze => "批量当前可见项 · 清除稍后提醒",
        ContextChoice::LaunchPreset => "启动仓库预设…",
        ContextChoice::ForgeCreateMergeRequest => "Forge · 创建合并请求…",
        ContextChoice::ForgeComment => "Forge · 评论合并请求…",
        ContextChoice::ForgeApprove => "Forge · 批准合并请求",
        ContextChoice::ForgeMerge => "Forge · 合并合并请求",
    }
}

pub fn render(frame: &mut Frame<'_>, app: &AppState) {
    let content_area = primary_view_rect(frame.area(), app.terminal_drawer_open);
    match &app.view {
        View::Registry => render_registry(frame, app, content_area),
        View::Thread(id) => render_thread(frame, app, id.0.as_str(), content_area),
        View::Review(id) => render_review(frame, app, id.0.as_str(), content_area),
        View::Workspace(id) => render_workspace(frame, app, id.0.as_str(), content_area),
        View::ManagedWorktrees(id) => {
            render_managed_worktrees(frame, app, id.0.as_str(), content_area);
        }
        View::Board => render_board(frame, app, content_area),
        View::Scratch(id) => render_scratch(frame, app, id, content_area),
    }
    if app.terminal_drawer_open {
        render_terminal_drawer(frame, app);
    }
    if app.show_help {
        render_help(frame, app.language);
    }
    if app.context_open {
        render_context_actions(frame, app);
    }
    if app.launch_menu_open {
        render_launch_presets(frame, app);
    }
    if matches!(
        app.input_mode,
        InputMode::Note
            | InputMode::Snooze
            | InputMode::SavedViewName
            | InputMode::BatchAddTag
            | InputMode::BatchRemoveTag
            | InputMode::BatchPriority
            | InputMode::BatchSnooze
            | InputMode::ForgeMergeRequestTitle
            | InputMode::ForgeComment
    ) {
        render_local_input_overlay(frame, app);
    }
    if app.pending_forge_operation.is_some() {
        render_forge_mutation_confirmation(frame, app);
    }
    if app.pending_local_batch.is_some() {
        render_local_batch_confirmation(frame, app);
    }
    if app.pending_launch_plan.is_some() {
        render_launch_confirmation(frame, app);
    }
}

pub fn primary_view_rect(area: Rect, drawer_open: bool) -> Rect {
    if !drawer_open {
        return area;
    }
    let drawer = terminal_drawer_rect(area);
    Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height.saturating_sub(drawer.height),
    }
}

pub fn terminal_drawer_rect(area: Rect) -> Rect {
    let height = (area.height.saturating_mul(2) / 5)
        .clamp(7, 22)
        .min(area.height);
    Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(height),
        width: area.width,
        height,
    }
}

pub fn terminal_drawer_pty_size(cols: u16, rows: u16) -> TerminalSize {
    let area = terminal_drawer_rect(Rect::new(0, 0, cols, rows));
    TerminalSize {
        rows: area.height.saturating_sub(2).max(1),
        cols: area.width.saturating_sub(2).max(1),
    }
}

fn render_terminal_drawer(frame: &mut Frame<'_>, app: &AppState) {
    let area = terminal_drawer_rect(frame.area());
    frame.render_widget(Clear, area);

    let snapshot = app.terminal_snapshot.as_ref();
    let status = snapshot
        .map(|snapshot| snapshot.state.label())
        .unwrap_or_else(|| "starting".into());
    let cwd = snapshot
        .map(|snapshot| snapshot.cwd.as_str())
        .unwrap_or_else(|| tr(app, "<starting>", "<启动中>"));
    let focus = if app.terminal_focused {
        tr(
            app,
            "FOCUSED · F6 release · Ctrl+] alt",
            "已聚焦 · F6 返回应用 · Ctrl+] 备用",
        )
    } else {
        tr(app, "unfocused · t focus · T close", "未聚焦 · t 聚焦 · T 关闭")
    };
    let title = format!(
        " {} · {focus} · {} · {} ",
        tr(app, "Terminal Drawer", "终端抽屉"),
        truncate_display(cwd, 44),
        truncate_display(&status, 36)
    );

    let lines = snapshot.map_or_else(
        || vec![Line::from(tr(app, "Starting platform default terminal…", "正在启动平台默认终端…"))],
        |snapshot| {
            snapshot
                .rows
                .iter()
                .map(|row| Line::from(row.clone()))
                .collect::<Vec<_>>()
        },
    );

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(title))
            .wrap(Wrap { trim: false }),
        area,
    );

    if app.terminal_focused
        && let Some(snapshot) = snapshot
    {
        let inner_x = area.x.saturating_add(1);
        let inner_y = area.y.saturating_add(1);
        let max_x = area.x.saturating_add(area.width.saturating_sub(2));
        let max_y = area.y.saturating_add(area.height.saturating_sub(2));
        let cursor_x = inner_x.saturating_add(snapshot.cursor_col).min(max_x);
        let cursor_y = inner_y.saturating_add(snapshot.cursor_row).min(max_y);
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn render_registry(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(area);

    match layout_mode(area.width) {
        LayoutMode::Compact | LayoutMode::Standard => {
            render_thread_list(frame, app, chunks[0]);
        }
        LayoutMode::Wide => {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
                .split(chunks[0]);
            render_thread_list(frame, app, columns[0]);
            frame.render_widget(detail_panel(app), columns[1]);
        }
    }

    let status_line = match app.input_mode {
        InputMode::Search => Line::from(format!(
            "/{}  · {}",
            app.input_buffer,
            tr(app, "Enter keep · Esc cancel", "Enter 保留 · Esc 取消")
        )),
        InputMode::Alias => Line::from(format!(
            "alias> {}  · {}",
            app.input_buffer,
            tr(app, "Enter save · Esc cancel", "Enter 保存 · Esc 取消")
        )),
        InputMode::Composer => Line::from(tr(
            app,
            "composer active in Thread view",
            "消息编辑器已在会话视图激活",
        )),
        InputMode::UserInput => Line::from(tr(
            app,
            "user-input answer active in Thread view",
            "用户输入回答已在会话视图激活",
        )),
        InputMode::ScratchTitle => Line::from(format!(
            "{}> {}  · {}",
            tr(app, "new scratch", "新建 Scratch"),
            app.input_buffer,
            tr(app, "Enter create · Esc cancel", "Enter 创建 · Esc 取消")
        )),
        InputMode::Snooze => Line::from(format!(
            "{}> {}  · 15m / 1h / 1d · {}",
            tr(app, "snooze", "稍后提醒"),
            app.input_buffer,
            tr(app, "Enter apply · Esc cancel", "Enter 应用 · Esc 取消")
        )),
        InputMode::Note => Line::from(format!(
            "{}> {}  · {}",
            tr(app, "note", "备注"),
            truncate_display(&app.input_buffer, 60),
            tr(app, "Enter save · Esc cancel", "Enter 保存 · Esc 取消")
        )),
        InputMode::SavedViewName => Line::from(format!(
            "{}> {}  · {}",
            tr(app, "view name", "视图名称"),
            truncate_display(&app.input_buffer, 60),
            tr(app, "Enter save · Esc cancel", "Enter 保存 · Esc 取消")
        )),
        InputMode::BatchAddTag
        | InputMode::BatchRemoveTag
        | InputMode::BatchPriority
        | InputMode::BatchSnooze => Line::from(tr(
            app,
            "batch-local input active in Board",
            "看板中的本地批量输入已激活",
        )),
        InputMode::GoalObjective => Line::from(tr(
            app,
            "Goal objective editor active in Thread view",
            "Goal 目标编辑器已在会话视图激活",
        )),
        InputMode::ForgeMergeRequestTitle | InputMode::ForgeComment => Line::from(tr(
            app,
            "forge mutation input active in Review/Workspace",
            "Forge 变更输入已在评审/工作区激活",
        )),
        InputMode::WorktreeCreateBranch
        | InputMode::WorktreeCreatePath
        | InputMode::WorktreeCreateStartPoint
        | InputMode::WorktreeDeleteBranch => Line::from(tr(
            app,
            "managed-worktree input active",
            "受管 worktree 输入已激活",
        )),
        InputMode::Normal => {
            if let Some(notice) = &app.mutation_notice {
                Line::from(format!(
                    "{} · {}",
                    tr(app, "notice", "提示"),
                    truncate_display(notice, 100)
                ))
            } else if let Some(error) = &app.backend_status.error {
                Line::from(format!(
                    "{} · {} · {}",
                    app.backend_status.source,
                    tr(app, "offline/degraded", "离线/降级"),
                    truncate_display(error, 80)
                ))
            } else if !app.backend_status.connected {
                Line::from(format!(
                    "{} · {}",
                    app.backend_status.source,
                    tr(app, "offline", "离线")
                ))
            } else {
                Line::from(registry_scope_status(app, area.width))
            }
        }
    };
    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::raw(tr(app, "j/k move  ", "j/k 移动  ")),
            Span::raw(tr(app, "t terminal  ", "t 终端  ")),
            Span::raw(tr(app, "Space attention  ", "Space 待处理  ")),
            Span::raw(tr(app, "/ search  ", "/ 搜索  ")),
            Span::raw(tr(app, "l local-only  ", "l 仅本机  ")),
            Span::raw(tr(app, "g repo-only  ", "g 仅仓库  ")),
            Span::raw(if app.show_all_history {
                tr(app, "h recent  ", "h 最近  ")
            } else {
                tr(app, "h all-history  ", "h 全部历史  ")
            }),
            Span::raw(tr(app, "p pin  ", "p 固定  ")),
            Span::raw(tr(app, "e alias  ", "e 别名  ")),
            Span::raw(tr(app, "x ack  ", "x 已处理  ")),
            Span::raw(tr(app, "s snooze  ", "s 稍后提醒  ")),
            Span::raw(tr(app, "= bind / 1–9 jump  ", "= 绑定 / 1–9 跳转  ")),
            Span::raw(tr(app, "! shared-worktree  ", "! 共享-worktree  ")),
            Span::raw(tr(app, "? help", "? 帮助")),
        ]),
        status_line,
    ]);
    frame.render_widget(footer, chunks[1]);
}

fn backend_platform_label(app: &AppState) -> &str {
    app.backend_status
        .platform
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("<unknown>")
}

fn backend_home_label(app: &AppState) -> &str {
    app.backend_status
        .codex_home
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("<unknown>")
}

fn registry_title(app: &AppState, width: u16) -> String {
    let raw = format!(
        "{} · {} · {}",
        tr(app, "Mission Control", "任务中心"),
        sanitize_inline(&app.backend_status.source),
        sanitize_inline(backend_platform_label(app))
    );
    let budget = usize::from(width.saturating_sub(4)).max(1);
    format!(" {} ", truncate_display(&raw, budget))
}

fn selected_git_status(app: &AppState) -> &'static str {
    let Some(thread) = app.selected_thread() else {
        return tr(app, "none", "无");
    };
    if !classify_cwd(&thread.metadata.cwd).terminal_usable() {
        return tr(app, "skipped", "已跳过");
    }
    match app.git_context(&thread.id) {
        None => tr(app, "not-probed", "未探测"),
        Some(context) if context.observed_at_unix_ms == 0 => tr(app, "probing", "探测中"),
        Some(context) if context.error.is_some() => tr(app, "degraded", "降级"),
        Some(context) if context.is_repository => tr(app, "repo", "仓库"),
        Some(_) => tr(app, "not-repo", "非仓库"),
    }
}

fn registry_scope_status(app: &AppState, width: u16) -> String {
    let (locality, terminal) = app
        .selected_thread()
        .map(|thread| {
            let locality = classify_cwd(&thread.metadata.cwd);
            (
                locality.label(),
                if locality.terminal_usable() {
                    tr(app, "ready", "就绪")
                } else {
                    tr(app, "blocked", "不可用")
                },
            )
        })
        .unwrap_or((tr(app, "none", "无"), tr(app, "blocked", "不可用")));
    let git = selected_git_status(app);
    let raw = if app.language.is_simplified_chinese() {
        format!(
            "已选 cwd: {locality} · 终端 {terminal} · git {git} · Codex home: {}",
            sanitize_inline(backend_home_label(app))
        )
    } else {
        format!(
            "selected cwd: {locality} · terminal {terminal} · git {git} · Codex home: {}",
            sanitize_inline(backend_home_label(app))
        )
    };
    truncate_display(&raw, usize::from(width.saturating_sub(2)).max(1))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RegistryViewport {
    start: usize,
    end: usize,
    total: usize,
    matched: usize,
    row_capacity: usize,
}

fn registry_viewport(app: &AppState, area_height: u16) -> (Vec<usize>, RegistryViewport) {
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

fn render_thread_list(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
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
    let (local_count, foreign_count, stale_count) =
        visible
            .iter()
            .fold((0_usize, 0_usize, 0_usize), |mut counts, index| {
                match classify_cwd(&app.threads[*index].metadata.cwd) {
                    CwdLocality::LocalDirectory => counts.0 += 1,
                    CwdLocality::ForeignWindows | CwdLocality::ForeignUnix => counts.1 += 1,
                    CwdLocality::NativeMissing => counts.2 += 1,
                    CwdLocality::Relative | CwdLocality::Empty => {}
                }
                counts
            });
    let range = if viewport.total == 0 {
        tr(app, "rows 0/0", "行 0/0").to_string()
    } else if app.language.is_simplified_chinese() {
        format!("行 {}-{}/{}", viewport.start + 1, viewport.end, viewport.total)
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
    let history = if app.language.is_simplified_chinese() {
        if !app.filter.is_empty() {
            "搜索全部"
        } else if app.show_all_history {
            "全部历史"
        } else {
            "最近"
        }
    } else if !app.filter.is_empty() {
        "SEARCH ALL"
    } else if app.show_all_history {
        "ALL HISTORY"
    } else {
        "RECENT"
    };
    let matched = viewport.matched;
    let summary = if app.language.is_simplified_chinese() {
        format!(
            "{scope}{history} · 显示 {}/{} 匹配 · 共 {} · 本机 {local_count} · 外部 {foreign_count} · 失效 {stale_count} · 待处理 {} · {range}{text_filter}",
            visible.len(),
            matched,
            app.threads.len(),
            attention_count
        )
    } else {
        format!(
            "{scope}{history} {}/{} matched · {} total · {local_count} local · {foreign_count} foreign · {stale_count} stale · {} need attention · {range}{text_filter}",
            visible.len(),
            matched,
            app.threads.len(),
            attention_count
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
        let attention = if app.thread_needs_attention(index) {
            let mut reasons = thread
                .attention
                .iter()
                .map(|reason| reason.label())
                .collect::<Vec<_>>();
            if pending_interactive {
                reasons.push("interactive");
            }
            reasons.join(",")
        } else if thread.attention.is_empty() {
            "-".into()
        } else {
            "ack".into()
        };
        let collision = if app.worktree_collision_count(&thread.id) > 0 {
            "!"
        } else {
            " "
        };
        let locality = match classify_cwd(&thread.metadata.cwd) {
            CwdLocality::LocalDirectory => "L",
            CwdLocality::ForeignWindows | CwdLocality::ForeignUnix => "F",
            CwdLocality::NativeMissing => "!",
            CwdLocality::Relative | CwdLocality::Empty => "?",
        };
        let text = match layout_mode(area.width) {
            LayoutMode::Compact => format!(
                "{prefix}{pin}{collision}{locality} {:7} {} {}",
                thread.runtime.label(),
                fit_display(&thread.workspace, 12),
                sanitize_inline(thread.display_title())
            ),
            LayoutMode::Standard | LayoutMode::Wide => format!(
                "{prefix}{pin}{collision}{locality} {:7} {} {} {}",
                thread.runtime.label(),
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

fn detail_panel(app: &AppState) -> Paragraph<'static> {
    let mut lines = if let Some(thread) = app.selected_thread() {
        let attention = if thread.attention.is_empty() {
            "none".into()
        } else {
            thread
                .attention
                .iter()
                .map(|reason| reason.label())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut lines = vec![
            Line::from(format!("Thread: {}", thread.id)),
            Line::from(format!("Workspace: {}", sanitize_inline(&thread.workspace))),
            Line::from(format!(
                "Runtime: {} · attention: {attention}",
                thread.runtime.label()
            )),
            Line::from(format!(
                "Model: {}",
                thread.metadata.model.as_deref().unwrap_or("unknown")
            )),
            Line::from(format!(
                "Cwd [{}]: {}",
                classify_cwd(&thread.metadata.cwd).label(),
                sanitize_inline(display_cwd(&thread.metadata.cwd))
            )),
            Line::from(format!(
                "Planning: {}",
                app.work_card_for_thread(&thread.id)
                    .map(|card| card.stage.label())
                    .unwrap_or("<unprojected>")
            )),
            Line::from(format!("Goal: {}", goal_summary(app, &thread.id.0))),
        ];
        if let Some(note) = app
            .work_card_for_thread(&thread.id)
            .and_then(|card| card.overlay.note.as_deref())
            .filter(|note| !note.trim().is_empty())
        {
            lines.push(Line::from(format!(
                "Note: {}",
                truncate_display(&sanitize_inline(note), 54)
            )));
        }
        lines
    } else {
        vec![Line::from("No thread selected")]
    };

    if let Some(thread) = app.selected_thread() {
        lines.push(Line::from(""));
        let locality = classify_cwd(&thread.metadata.cwd);
        if !locality.terminal_usable() {
            lines.push(Line::from(format!(
                "Git: skipped · cwd {} on this host",
                locality.label()
            )));
        } else {
            match app.git_context(&thread.id) {
                None => lines.push(Line::from("Git: not probed")),
                Some(context) if context.observed_at_unix_ms == 0 => {
                    lines.push(Line::from("Git: probing…"));
                }
                Some(context) if context.error.is_some() => {
                    lines.push(Line::from(format!(
                        "Git: degraded · {}",
                        context.error.as_deref().unwrap_or("unknown error")
                    )));
                }
                Some(context) if !context.is_repository => {
                    lines.push(Line::from("Git: not a repository"));
                }
                Some(context) => {
                    let branch = context
                        .branch
                        .as_deref()
                        .or(context.head.as_deref())
                        .unwrap_or("unknown");
                    lines.push(Line::from(format!("Git: {branch}")));
                    lines.push(Line::from(format!(
                        "Dirty: {} · files={} · +{} -{}",
                        context.dirty,
                        context.changes.len(),
                        context.ahead,
                        context.behind
                    )));
                    if let Some(worktree) = &context.worktree {
                        lines.push(Line::from(format!(
                            "Worktree: {}",
                            truncate_display(&worktree.canonical_path, 42)
                        )));
                    }
                    if let Some(repo) = &context.repo {
                        lines.push(Line::from(format!(
                            "Repo: {}",
                            truncate_display(&repo.primary_root, 42)
                        )));
                    }
                    let collisions = app.worktree_collision_count(&thread.id);
                    if collisions > 0 {
                        lines.push(Line::from(format!(
                            "WARNING: shared mutable checkout with {collisions} active thread(s)"
                        )));
                    }
                }
            }
        }
    }

    if let Some(thread) = app.selected_thread() {
        lines.push(Line::from(""));
        lines.extend(forge_context_lines(app, &thread.id, false));
    }

    Paragraph::new(lines)
        .block(Block::bordered().title(" Selected "))
        .wrap(Wrap { trim: false })
}

fn forge_review_label(app: &AppState, thread_id: &str) -> String {
    let Some(review) = app
        .forge_observations
        .get(thread_id)
        .and_then(|observation| observation.review.as_ref())
    else {
        return String::new();
    };

    let approvals = if review.approvals_available {
        match (review.approvals_required, review.approvals_left) {
            (Some(required), Some(left)) => {
                format!(" · approvals {} left/{required}", left)
            }
            _ => format!(" · approvals {}", review.approved_by_count),
        }
    } else {
        " · approvals n/a".into()
    };
    let changes_requested = if review.changes_requested_by_count > 0 {
        format!(" · changes requested {}", review.changes_requested_by_count)
    } else {
        String::new()
    };
    let discussions = if review.discussions_available {
        format!(" · unresolved {}", review.unresolved_discussions)
    } else {
        " · discussions n/a".into()
    };
    format!(
        " · CR {}{}{}{}",
        review.change_request_iid, approvals, changes_requested, discussions
    )
}

fn forge_context_lines(
    app: &AppState,
    thread_id: &crate::domain::ThreadId,
    diagnostic_hint: bool,
) -> Vec<Line<'static>> {
    let Some(observation) = app.forge_observation(thread_id) else {
        return if diagnostic_hint {
            vec![Line::from("Forge: not probed")]
        } else {
            vec![]
        };
    };

    if observation.observed_at_unix_ms == 0 {
        return if diagnostic_hint {
            vec![Line::from("Forge: probing…")]
        } else {
            vec![]
        };
    }

    let Some(identity) = &observation.identity else {
        return if diagnostic_hint {
            vec![Line::from(
                "Forge: unavailable · run codex-tui doctor forge for details",
            )]
        } else {
            vec![]
        };
    };

    let mut lines = vec![Line::from(format!(
        "Forge: {} · {}/{} · {}",
        identity.provider.label(),
        identity.host,
        identity.path_with_namespace,
        observation
            .freshness_at(crate::operation::now_unix_ms())
            .label()
    ))];

    let branch = app
        .git_context(thread_id)
        .and_then(|context| context.branch.as_deref());
    if let Some(branch) = branch {
        if let Some(change) = observation.change_request_for_branch(branch) {
            lines.push(Line::from(format!(
                "CR: {} · {}{} · {}",
                change.iid,
                if change.draft { "draft · " } else { "" },
                change.state,
                truncate_display(&change.title, 58)
            )));
        } else {
            lines.push(Line::from(format!("CR: none for branch {branch}")));
        }
        if let Some(pipeline) = observation.pipeline_for_branch(branch) {
            lines.push(Line::from(format!(
                "Pipeline: #{} · {}",
                pipeline.id, pipeline.status
            )));
        } else {
            lines.push(Line::from("Pipeline: none for current branch"));
        }
    } else {
        lines.push(Line::from("CR/Pipeline: current branch unavailable"));
    }

    if let Some(notice) = &app.mutation_notice {
        lines.push(Line::from(format!(
            "Mutation: {}",
            truncate_display(notice, 90)
        )));
    }

    lines
}

fn goal_summary(app: &AppState, thread_id: &str) -> String {
    if let Some(goal) = app.goals.get(thread_id) {
        let budget = goal
            .token_budget
            .map(|budget| format!("{}/{budget}", goal.tokens_used))
            .unwrap_or_else(|| goal.tokens_used.to_string());
        return format!(
            "{} · {} · tokens={} · {}s",
            goal.status.label(),
            truncate_display(&goal.objective, 42),
            budget,
            goal.time_used_seconds
        );
    }
    if app
        .backend_status
        .optional_capabilities_missing
        .iter()
        .any(|capability| capability == "thread/goal/get")
    {
        return "unavailable on this App Server".into();
    }
    if app.goal_checked.contains(thread_id) {
        return "none".into();
    }
    "probing…".into()
}

fn render_thread(frame: &mut Frame<'_>, app: &AppState, thread_id: &str, area: Rect) {
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
        .unwrap_or("Unknown thread");
    frame.render_widget(
        Paragraph::new(format!(
            "{title}\n{thread_id}\nGoal: {}",
            goal_summary(app, thread_id)
        ))
        .block(Block::bordered().title(" Thread ")),
        chunks[0],
    );

    let ui = app.thread_ui.get(thread_id).cloned().unwrap_or_default();
    let mut conversation_lines = match app.conversations.get(thread_id) {
        Some(conversation) if conversation.loading => {
            vec![Line::from("Loading recent Codex history…")]
        }
        Some(conversation) if conversation.error.is_some() => {
            vec![Line::from(format!(
                "Conversation unavailable: {}",
                conversation.error.as_deref().unwrap_or("unknown error")
            ))]
        }
        Some(conversation) if conversation.items.is_empty() => {
            vec![Line::from("No visible items in the loaded history page.")]
        }
        Some(conversation) => conversation
            .items
            .iter()
            .map(|item| {
                let status = item
                    .status
                    .as_deref()
                    .map(|value| format!(" [{value}]"))
                    .unwrap_or_default();
                Line::from(format!(
                    "{:>5}{status} {}",
                    item.kind.label(),
                    item.text.replace('\n', " ")
                ))
            })
            .collect(),
        None => vec![Line::from("Conversation has not been loaded yet.")],
    };
    if let Some(request) = app.current_pending_request() {
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
    frame.render_widget(
        Paragraph::new(conversation_lines)
            .block(Block::bordered().title(if page_hint {
                " Conversation · older history available "
            } else {
                " Conversation "
            }))
            .wrap(Wrap { trim: false })
            .scroll((
                if app.current_pending_request().is_some() {
                    0
                } else {
                    ui.scroll
                },
                0,
            )),
        chunks[1],
    );

    let (composer_title, composer_text) = if app.input_mode == InputMode::GoalObjective {
        (
            " Goal objective · Enter set · Esc cancel ",
            format!("objective> {}", app.input_buffer),
        )
    } else if app.goal_actions_open {
        let text = app.current_goal().map_or_else(
            || "No Goal observed. e/Enter creates one with ACTIVE status.".into(),
            |goal| {
                format!(
                    "{}\nstatus={} · tokens={}{} · elapsed={}s",
                    goal.objective,
                    goal.status.label(),
                    goal.tokens_used,
                    goal.token_budget
                        .map(|budget| format!("/{budget}"))
                        .unwrap_or_default(),
                    goal.time_used_seconds
                )
            },
        );
        (
            " Goal actions · e objective · p pause · r resume · c clear · Esc close ",
            text,
        )
    } else if app.input_mode == InputMode::UserInput {
        let question = app.current_user_input_question();
        let displayed_answer = if question.is_some_and(|question| question.is_secret) {
            "*".repeat(app.input_buffer.chars().count())
        } else {
            app.input_buffer.clone()
        };
        (
            " User input · Enter next/send · Esc cancel editor ",
            format!(
                "{}\nanswer> {}",
                question
                    .map(|question| question.question.as_str())
                    .unwrap_or("Question unavailable"),
                displayed_answer
            ),
        )
    } else {
        (
            if app.input_mode == InputMode::Composer {
                " Composer · Enter send · Esc keep draft "
            } else {
                " Draft · a edit "
            },
            format!(
                "{}\nscroll={} follow={}",
                if ui.draft.is_empty() {
                    "<empty>"
                } else {
                    &ui.draft
                },
                ui.scroll,
                ui.follow
            ),
        )
    };
    let composer = Paragraph::new(composer_text).block(Block::bordered().title(composer_title));
    frame.render_widget(composer, chunks[2]);
    frame.render_widget(
        Paragraph::new(
            "a composer · g goal · m worktrees · y accept · n decline · c cancel · i answer · Ctrl+C interrupt",
        ),
        chunks[3],
    );
}

fn render_board(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
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
                let lines = stage_cards
                    .iter()
                    .enumerate()
                    .map(|(index, card)| {
                        let selected =
                            stage_index == app.board_stage_index && index == app.board_selected;
                        let attention = if card.needs_you() { "!" } else { " " };
                        let pin = if card.overlay.pinned { "*" } else { " " };
                        let goal = card
                            .goal
                            .as_ref()
                            .map(|goal| format!(" [{}]", goal.status.label()))
                            .unwrap_or_default();
                        let text = format!(
                            "{}{}{} {}{}",
                            if selected { ">" } else { " " },
                            pin,
                            attention,
                            truncate_display(&sanitize_inline(&card.title), 20),
                            goal
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
                            stage.label(),
                            stage_cards.len()
                        )))
                        .wrap(Wrap { trim: false }),
                    columns[stage_index],
                );
            }
        }
        SavedViewLayout::Board => {
            let stage = WorkflowStage::ALL[app.board_stage_index % WorkflowStage::ALL.len()];
            let lines = app
                .visible_planning_cards()
                .iter()
                .enumerate()
                .map(|(index, card)| planning_card_line(card, index == app.board_selected))
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(lines)
                    .block(Block::bordered().title(format!(
                        " {} · {} · {}/{} ",
                        view.name,
                        stage.label(),
                        app.board_stage_index + 1,
                        WorkflowStage::ALL.len()
                    )))
                    .wrap(Wrap { trim: false }),
                outer[0],
            );
        }
        SavedViewLayout::List | SavedViewLayout::ReviewQueue => {
            let cards = app.visible_planning_cards();
            let mut lines = Vec::new();
            let mut previous_group = String::new();
            for (index, card) in cards.iter().enumerate() {
                let group = saved_view_group_key(card, view.group_by.as_deref());
                if !group.is_empty() && group != previous_group {
                    lines.push(Line::from(format!("── {group} ──")));
                    previous_group = group;
                }
                lines.push(planning_card_line(card, index == app.board_selected));
            }
            if lines.is_empty() {
                lines.push(Line::from("No cards match this Saved View."));
            }
            frame.render_widget(
                Paragraph::new(lines)
                    .block(Block::bordered().title(format!(
                        " {} · {} ",
                        view.name,
                        view.layout.label()
                    )))
                    .wrap(Wrap { trim: false }),
                outer[0],
            );
        }
    }

    let input = if app.input_mode == InputMode::ScratchTitle {
        format!(
            "new scratch> {} · Enter create · Esc cancel",
            app.input_buffer
        )
    } else if app.input_mode == InputMode::Snooze {
        format!(
            "snooze> {} · 15m / 1h / 1d · Enter apply · Esc cancel",
            app.input_buffer
        )
    } else if app.input_mode == InputMode::Note {
        format!(
            "note> {} · Enter save · Esc cancel",
            truncate_display(&app.input_buffer, 80)
        )
    } else if app.input_mode == InputMode::SavedViewName {
        format!(
            "view name> {} · Enter save · Esc cancel",
            truncate_display(&app.input_buffer, 80)
        )
    } else if app.hot_slot_bind_pending {
        "bind hot slot: press 1–9 · Esc cancels other input only".into()
    } else if let Some(error) = &app.planning_store_error {
        format!("LOCAL STORE DEGRADED · {}", truncate_display(error, 80))
    } else {
        format!(
            "h/l stage · j/k item · Tab view · Enter open · Space attention · s snooze · = bind · 1–9 jump · n scratch · view {}/{}",
            app.planning_view_index + 1,
            app.planning_views().len()
        )
    };
    frame.render_widget(Paragraph::new(input), outer[1]);
}

fn planning_card_line(card: &crate::planning::WorkCardProjection, selected: bool) -> Line<'static> {
    let attention = if card.needs_you() {
        card.attention
            .iter()
            .map(crate::planning::PlanningAttention::label)
            .collect::<Vec<_>>()
            .join(",")
    } else if card.snoozed && !card.attention.is_empty() {
        "snoozed".into()
    } else {
        "-".into()
    };
    let source = match card.anchor.kind {
        crate::planning::SourceKind::ScratchWork => "scratch",
        crate::planning::SourceKind::CodexThread => "thread",
        crate::planning::SourceKind::ForgeWorkItem => "forge",
        _ => "link",
    };
    let goal = card
        .goal
        .as_ref()
        .map(|goal| goal.status.label())
        .unwrap_or("-");
    let text = format!(
        "{} {} {} {} {} {}",
        if selected { ">" } else { " " },
        fit_display(card.stage.label(), 7),
        fit_display(&attention, 10),
        fit_display(goal, 12),
        fit_display(source, 8),
        sanitize_inline(&card.title)
    );
    let style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    Line::from(Span::styled(text, style))
}

fn render_scratch(frame: &mut Frame<'_>, app: &AppState, scratch_id: &str, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(1)])
        .split(area);
    let lines = if let Some(scratch) = app
        .planning_snapshot
        .scratch
        .iter()
        .find(|scratch| scratch.id == scratch_id)
    {
        vec![
            Line::from(format!("Scratch: {}", scratch.id)),
            Line::from(format!("Title: {}", sanitize_inline(&scratch.title))),
            Line::from(format!(
                "State: {:?} · priority={}",
                scratch.state,
                scratch
                    .priority
                    .map_or_else(|| "none".into(), |value| value.to_string())
            )),
            Line::from(format!(
                "Workspace: {}",
                sanitize_inline(scratch.workspace.as_deref().unwrap_or("<none>"))
            )),
            Line::from(format!(
                "Note: {}",
                sanitize_inline(scratch.note.as_deref().unwrap_or("<empty>"))
            )),
            Line::from(""),
            Line::from(
                "Local ScratchWork only. It is not a Codex thread, Git work item, or forge issue.",
            ),
        ]
    } else {
        vec![Line::from("ScratchWork no longer exists.")]
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" ScratchWork "))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );
    frame.render_widget(Paragraph::new("Esc back to Board"), chunks[1]);
}

fn render_workspace(frame: &mut Frame<'_>, app: &AppState, thread_id: &str, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(1)])
        .split(area);

    let thread = app.threads.iter().find(|thread| thread.id.0 == thread_id);
    let Some(thread) = thread else {
        frame.render_widget(
            Paragraph::new("Thread no longer exists.")
                .block(Block::bordered().title(" Workspace ")),
            chunks[0],
        );
        return;
    };

    let mut lines = vec![
        Line::from(format!("Thread: {}", thread.id)),
        Line::from(format!("Cwd: {}", thread.metadata.cwd)),
    ];

    match app.git_context(&thread.id) {
        None => lines.push(Line::from("Git: not probed")),
        Some(context) if context.observed_at_unix_ms == 0 => {
            lines.push(Line::from("Git: probing…"));
        }
        Some(context) if context.error.is_some() => {
            lines.push(Line::from(format!(
                "Git: degraded · {}",
                context.error.as_deref().unwrap_or("unknown error")
            )));
        }
        Some(context) if !context.is_repository => {
            lines.push(Line::from("Git: not a repository"));
        }
        Some(context) => {
            if let Some(repo) = &context.repo {
                lines.push(Line::from(format!("Repo root: {}", repo.primary_root)));
                lines.push(Line::from(format!(
                    "Git common dir: {}",
                    repo.git_common_dir
                )));
            }
            if let Some(worktree) = &context.worktree {
                lines.push(Line::from(format!("Worktree: {}", worktree.canonical_path)));
            }
            lines.push(Line::from(format!(
                "Branch: {}",
                context
                    .branch
                    .as_deref()
                    .or(context.head.as_deref())
                    .unwrap_or("<unknown>")
            )));
            lines.push(Line::from(format!(
                "Upstream: {} · ahead={} behind={}",
                context.upstream.as_deref().unwrap_or("<none>"),
                context.ahead,
                context.behind
            )));
            lines.push(Line::from(format!(
                "Dirty: {} · changed files={}",
                context.dirty,
                context.changes.len()
            )));
            let collisions = app.worktree_collision_count(&thread.id);
            if collisions > 0 {
                lines.push(Line::from(format!(
                    "WARNING: shared mutable checkout with {collisions} active thread(s)"
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from("Changed files:"));
            lines.extend(context.changes.iter().take(100).map(|change| {
                Line::from(format!(
                    "  {:2} {}",
                    change.status_label(),
                    sanitize_inline(&change.path)
                ))
            }));
            if context.changes.len() > 100 {
                lines.push(Line::from(format!(
                    "  … {} additional change(s)",
                    context.changes.len() - 100
                )));
            }
        }
    }

    lines.push(Line::from(""));
    lines.extend(forge_context_lines(app, &thread.id, true));

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Workspace · Git + Forge "))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new("r review · m managed worktrees · . actions · Esc back"),
        chunks[1],
    );
}

fn render_managed_worktrees(frame: &mut Frame<'_>, app: &AppState, thread_id: &str, area: Rect) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(2)])
        .split(area);

    let thread = app.threads.iter().find(|thread| thread.id.0 == thread_id);
    let context = thread.and_then(|thread| app.git_context(&thread.id));
    let repo = context.and_then(|context| context.repo.as_ref());
    let worktrees = app.visible_managed_worktrees();

    let mut lines = Vec::new();
    lines.push(Line::from(format!(
        "Repository: {}",
        repo.map(|repo| repo.primary_root.as_str())
            .unwrap_or("<unavailable>")
    )));
    lines.push(Line::from(format!(
        "Managed/adopted worktrees: {}",
        worktrees.len()
    )));
    lines.push(Line::from(""));

    if worktrees.is_empty() {
        lines.push(Line::from(
            "No managed/adopted worktrees for this repository.",
        ));
    } else {
        for (index, record) in worktrees.iter().enumerate() {
            let selected = index == app.managed_selected;
            let prefix = if selected { ">" } else { " " };
            let ownership = if record.adopted { "adopted" } else { "managed" };
            let text = format!(
                "{prefix} {} {} {}",
                fit_display(ownership, 8),
                fit_display(record.branch.as_deref().unwrap_or("<detached>"), 18),
                sanitize_inline(&record.canonical_path)
            );
            let style = if selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(text, style)));
        }
    }

    if let Some(plan) = &app.pending_operation {
        lines.push(Line::from(""));
        lines.push(Line::from(
            "CONFIRM REQUIRED — no mutation has executed yet.",
        ));
        lines.push(Line::from(format!("Operation: {}", plan.kind.label())));
        lines.push(Line::from(format!("Cwd: {}", plan.cwd)));
        let command = if plan.argv.is_empty() {
            "metadata-only adoption (no Git mutation)".to_string()
        } else {
            let argv = plan
                .argv
                .iter()
                .map(|argument| format!("{argument:?}"))
                .collect::<Vec<_>>()
                .join(" ");
            format!("git -C {:?} {argv}", plan.cwd)
        };
        lines.push(Line::from(format!("Exact operation: {command}")));
        lines.push(Line::from(format!(
            "Expected: {}",
            plan.expected_side_effect
        )));
        if let Some(path) = &plan.target_worktree {
            lines.push(Line::from(format!("Target worktree: {path}")));
        }
        if let Some(branch) = &plan.target_branch {
            lines.push(Line::from(format!("Target branch: {branch}")));
        }
        lines.push(Line::from("Preconditions:"));
        lines.extend(plan.preconditions.iter().map(|precondition| {
            Line::from(format!(
                "  {} = {}",
                precondition.key, precondition.expected
            ))
        }));
        lines.push(Line::from("Press y to execute; c or Esc cancels."));
    }

    if let Some(receipt) = app.recent_operations.first() {
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "Latest receipt: {} · {:?}",
            receipt.plan.kind.label(),
            receipt.state
        )));
        if let Some(verification) = &receipt.verification {
            lines.push(Line::from(format!(
                "Verified: {}",
                truncate_display(verification, 90)
            )));
        }
        if let Some(failure) = &receipt.failure {
            lines.push(Line::from(format!(
                "Failure: {}",
                truncate_display(failure, 90)
            )));
        }
    }

    if let Some(notice) = &app.mutation_notice {
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "Notice: {}",
            truncate_display(notice, 100)
        )));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Managed Worktrees · Plan → Confirm → Verify "))
            .wrap(Wrap { trim: false }),
        outer[0],
    );

    let footer = match app.input_mode {
        InputMode::WorktreeCreateBranch => format!(
            "new branch> {} · Enter next · Esc cancel",
            app.input_buffer
        ),
        InputMode::WorktreeCreatePath => format!(
            "absolute worktree path> {} · Enter next · Esc cancel",
            app.input_buffer
        ),
        InputMode::WorktreeCreateStartPoint => format!(
            "start point> {} · Enter plan · Esc cancel",
            app.input_buffer
        ),
        InputMode::WorktreeDeleteBranch => format!(
            "branch to delete> {} · Enter plan · Esc cancel",
            app.input_buffer
        ),
        _ if app.pending_operation.is_some() => {
            "y CONFIRM execute · c cancel plan · Esc cancel plan".into()
        }
        _ => "j/k select · n create · a adopt current · d remove selected · x delete branch · Esc back".into(),
    };
    frame.render_widget(Paragraph::new(footer), outer[1]);
}

fn render_review(frame: &mut Frame<'_>, app: &AppState, thread_id: &str, area: Rect) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(1)])
        .split(area);

    let Some(review) = app.git_reviews.get(thread_id) else {
        frame.render_widget(
            Paragraph::new("Review has not been loaded.")
                .block(Block::bordered().title(" Review ")),
            outer[0],
        );
        return;
    };

    if review.observed_at_unix_ms == 0 {
        frame.render_widget(
            Paragraph::new("Loading Git review…").block(Block::bordered().title(" Review ")),
            outer[0],
        );
    } else if let Some(error) = &review.error {
        frame.render_widget(
            Paragraph::new(format!("Review unavailable: {error}"))
                .block(Block::bordered().title(" Review ")),
            outer[0],
        );
    } else {
        let forge_summary = forge_review_label(app, thread_id);
        let files = review
            .changes
            .iter()
            .enumerate()
            .map(|(index, change)| {
                let selected = index == app.review_selected;
                let prefix = if selected { ">" } else { " " };
                let text = format!("{prefix} {:2} {}", change.status_label(), change.path);
                let style = if selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(text, style))
            })
            .collect::<Vec<_>>();

        let mut diff_lines = presentation_diff_lines(review, app.review_word_diff)
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>();
        if diff_lines.is_empty() {
            diff_lines.push(Line::from(
                "No staged/unstaged tracked diff. Untracked files remain listed at left/top.",
            ));
        }

        if area.width >= 100 {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
                .split(outer[0]);
            frame.render_widget(
                Paragraph::new(files)
                    .block(Block::bordered().title(format!(
                        " Changed files ({}){} ",
                        review.changes.len(),
                        forge_summary
                    )))
                    .wrap(Wrap { trim: false }),
                columns[0],
            );
            frame.render_widget(
                Paragraph::new(diff_lines)
                    .block(Block::bordered().title(format!(
                        " Git diff · word={}{} ",
                        app.review_word_diff,
                        if review.truncated {
                            " · truncated"
                        } else {
                            ""
                        }
                    )))
                    .wrap(Wrap { trim: false })
                    .scroll((app.review_scroll, 0)),
                columns[1],
            );
        } else {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(6), Constraint::Min(4)])
                .split(outer[0]);
            frame.render_widget(
                Paragraph::new(files)
                    .block(Block::bordered().title(format!(" Changed files{} ", forge_summary)))
                    .wrap(Wrap { trim: false }),
                rows[0],
            );
            frame.render_widget(
                Paragraph::new(diff_lines)
                    .block(Block::bordered().title(format!(
                        " Git diff · word={}{} ",
                        app.review_word_diff,
                        if review.truncated {
                            " · truncated"
                        } else {
                            ""
                        }
                    )))
                    .wrap(Wrap { trim: false })
                    .scroll((app.review_scroll, 0)),
                rows[1],
            );
        }
    }

    frame.render_widget(
        Paragraph::new(
            "j/k file · PageUp/PageDown diff · w word-diff · e editor · . actions · Esc back",
        ),
        outer[1],
    );
}

fn render_context_actions(frame: &mut Frame<'_>, app: &AppState) {
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
                format!("{} {}", if selected { ">" } else { " " }, choice.label()),
                style,
            ))
        })
        .chain(std::iter::once(Line::from(
            "j/k move · Enter execute · Esc close",
        )))
        .collect::<Vec<_>>();
    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(18)
        .clamp(6, 24);
    let area = centered_fixed(64, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Actions "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_local_input_overlay(frame: &mut Frame<'_>, app: &AppState) {
    let (title, hint) = match app.input_mode {
        InputMode::Note => (" Local note ", "Enter save · Esc cancel"),
        InputMode::Snooze => (" Snooze ", "15m / 1h / 1d · Enter apply · Esc cancel"),
        InputMode::SavedViewName => (" Save current view ", "Enter save · Esc cancel"),
        InputMode::BatchAddTag => (
            " Batch visible · Add tag ",
            "Enter creates frozen plan · Esc cancel",
        ),
        InputMode::BatchRemoveTag => (
            " Batch visible · Remove tag ",
            "Enter creates frozen plan · Esc cancel",
        ),
        InputMode::BatchPriority => (
            " Batch visible · Set priority ",
            "integer · Enter creates frozen plan · Esc cancel",
        ),
        InputMode::BatchSnooze => (
            " Batch visible · Snooze ",
            "15m / 1h / 1d · Enter creates frozen plan · Esc cancel",
        ),
        InputMode::ForgeMergeRequestTitle => (
            " Create GitLab merge request ",
            "Enter creates a plan only · Esc cancel",
        ),
        InputMode::ForgeComment => (
            " Comment on GitLab merge request ",
            "Enter creates a plan only · Esc cancel",
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

fn render_forge_mutation_confirmation(frame: &mut Frame<'_>, app: &AppState) {
    let Some(plan) = &app.pending_forge_operation else {
        return;
    };

    let mut lines = vec![
        Line::from("CONFIRM REQUIRED — no GitLab mutation has executed yet."),
        Line::from(format!("Operation: {}", plan.kind.label())),
        Line::from(format!("Project: {}/{}", plan.host, plan.project_path)),
    ];
    if let Some(iid) = plan.change_request_iid {
        lines.push(Line::from(format!("Merge request: !{iid}")));
    }
    if plan.source_branch.is_some() || plan.target_branch.is_some() {
        lines.push(Line::from(format!(
            "Branches: {} -> {}",
            plan.source_branch.as_deref().unwrap_or("<none>"),
            plan.target_branch.as_deref().unwrap_or("<none>")
        )));
    }
    if let Some(title) = &plan.title {
        lines.push(Line::from(format!(
            "Title: {}",
            truncate_display(title, 88)
        )));
    }
    if let Some(bytes) = plan.payload_bytes {
        lines.push(Line::from(format!(
            "Payload: {bytes} bytes · body intentionally not persisted"
        )));
    }
    lines.push(Line::from(format!(
        "Expected: {}",
        truncate_display(&plan.expected_side_effect, 100)
    )));
    lines.push(Line::from(""));
    lines.push(Line::from("Preconditions revalidated at execution time:"));
    lines.extend(
        plan.preconditions
            .iter()
            .take(8)
            .map(|item| Line::from(format!("  {} = {}", item.key, item.expected))),
    );
    lines.push(Line::from(""));
    lines.push(Line::from("y CONFIRM execute · c/Esc cancel"));

    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(18)
        .clamp(10, 22);
    let area = centered_fixed(82, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Forge Mutation Plan "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_local_batch_confirmation(frame: &mut Frame<'_>, app: &AppState) {
    let Some(plan) = &app.pending_local_batch else {
        return;
    };

    let mut lines = vec![
        Line::from("CONFIRM REQUIRED — no local write has executed yet."),
        Line::from(format!("Operation: {}", plan.action.label())),
        Line::from(format!("Frozen targets: {}", plan.targets.len())),
        Line::from("Target set will NOT be recomputed before execution."),
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
        lines.push(Line::from(format!(
            "  … and {} more frozen target(s)",
            plan.targets.len() - 8
        )));
    }

    lines.extend([
        Line::from(""),
        Line::from("SQLite: one transaction; any failure rolls the whole batch back."),
        Line::from("Authority: local overlays / ScratchWork only; no Codex/Git/Forge write."),
        Line::from("y CONFIRM execute · c/Esc cancel"),
    ]);

    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(18)
        .clamp(12, 24);
    let area = centered_fixed(86, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Local Batch Plan "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_launch_presets(frame: &mut Frame<'_>, app: &AppState) {
    let mut lines = Vec::new();
    if app.launch_presets.is_empty() {
        lines.push(Line::from("No [[launch]] presets in .codex-tui.toml."));
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
        Line::from("j/k move · Enter create exact launch plan · Esc close"),
    ]);
    let height = u16::try_from(lines.len().saturating_add(2))
        .unwrap_or(12)
        .clamp(7, 22);
    let area = centered_fixed(92, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Repository Launch Presets "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_launch_confirmation(frame: &mut Frame<'_>, app: &AppState) {
    let Some(plan) = &app.pending_launch_plan else {
        return;
    };
    let lines = vec![
        Line::from("CONFIRM REQUIRED — process has not started."),
        Line::from(format!("Preset: {}", sanitize_inline(&plan.name))),
        Line::from(format!("Config: {}", plan.config_path.display())),
        Line::from(format!("Cwd: {}", plan.cwd.display())),
        Line::from(format!("Exact argv: {}", plan.command_preview())),
        Line::from(""),
        Line::from("No shell/eval/interpolation is used."),
        Line::from("External process only; this is not the embedded Terminal Drawer."),
        Line::from("y CONFIRM start · c/Esc cancel"),
    ];
    let area = centered_fixed(92, 13, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Launch Preset Plan "))
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

const HELP_LINES: &[&str] = &[
    "Global: ? help · Ctrl+K palette · / search · . actions · t terminal · T close terminal · Esc back",
    "Registry: j/k · Enter · Space attention · / search · l local-only · g repo-only · h recent/all-history · p pin · e alias · x ack",
    "Thread: a composer · y/n/c approval · i answer · Ctrl+C interrupt · r review",
    "Review: j/k file · w word-diff · e editor · . Forge actions · PageUp/PageDown · Esc",
    "Workspace: Git + Forge · . actions/launch presets · r review · m worktrees · Esc",
    "Managed Worktrees: n create · a adopt · d remove · x delete branch · y confirm",
    "Board: h/l stage · j/k item · Space attention · s snooze · = bind · 1–9 hot slot",
    "Board: Tab Saved View · . batch-local/context · Enter open · a Quick Prompt · n Scratch",
    "Scratch: local-only detail · Esc Board",
    "Terminal focus: keys go to PTY · F6 return to app · Ctrl+] alternate · Shift+PgUp/PgDn scrollback.",
    "Authority: Codex/Git/Forge stay canonical; codex-tui stores operator state only.",
];

fn render_help(frame: &mut Frame<'_>) {
    let area = centered_rect(70, 70, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(
            HELP_LINES
                .iter()
                .map(|line| Line::from(*line))
                .collect::<Vec<_>>(),
        )
        .block(Block::bordered().title(" Help "))
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn interactive_request_lines(request: &InteractiveRequest) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("NEEDS YOU")];
    match &request.kind {
        InteractiveRequestKind::CommandApproval {
            command,
            cwd,
            reason,
        } => {
            lines.push(Line::from(format!("Command approval: {command}")));
            if !cwd.is_empty() {
                lines.push(Line::from(format!("cwd: {cwd}")));
            }
            if let Some(reason) = reason {
                lines.push(Line::from(format!("reason: {reason}")));
            }
            lines.push(Line::from("y accept · n decline · c cancel"));
        }
        InteractiveRequestKind::FileChangeApproval { reason } => {
            lines.push(Line::from("File change approval"));
            if let Some(reason) = reason {
                lines.push(Line::from(format!("reason: {reason}")));
            }
            lines.push(Line::from("y accept · n decline · c cancel"));
        }
        InteractiveRequestKind::PermissionsApproval {
            reason,
            network_requested,
            filesystem_requested,
        } => {
            lines.push(Line::from(format!(
                "Permission request: network={} filesystem={}",
                network_requested, filesystem_requested
            )));
            if let Some(reason) = reason {
                lines.push(Line::from(format!("reason: {reason}")));
            }
            lines.push(Line::from("y grant for this turn · n/c decline"));
        }
        InteractiveRequestKind::UserInput { questions } => {
            lines.push(Line::from(format!(
                "User input requested: {} question(s)",
                questions.len()
            )));
            if let Some(question) = questions.first() {
                lines.push(Line::from(format!(
                    "{}: {}",
                    question.header, question.question
                )));
                if !question.options.is_empty() {
                    lines.push(Line::from(format!(
                        "options: {}",
                        question.options.join(", ")
                    )));
                }
            }
            lines.push(Line::from("i answer"));
        }
    }
    lines
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::backend::{CodexBackend, FakeBackend};
    use pretty_assertions::assert_eq;
    use ratatui::{Terminal, backend::TestBackend};

    fn render_snapshot(width: u16) -> String {
        let backend = TestBackend::new(width, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let app = AppState::new(FakeBackend::seeded().snapshot().threads);
        terminal.draw(|frame| render(frame, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn registry_viewport_keeps_selected_row_visible() {
        let mut app = AppState::new(FakeBackend::scaled(100).snapshot().threads);
        app.selected = 99;

        let (_visible, viewport) = registry_viewport(&app, 10);
        assert_eq!(
            viewport,
            RegistryViewport {
                start: 93,
                end: 100,
                total: 100,
                matched: 100,
                row_capacity: 7,
            }
        );
    }

    #[test]
    fn registry_renders_scrollbar_and_last_selected_row() {
        let backend = TestBackend::new(100, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = AppState::new(FakeBackend::scaled(100).snapshot().threads);
        app.selected = 99;

        terminal.draw(|frame| render(frame, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut snapshot = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                snapshot.push_str(buffer[(x, y)].symbol());
            }
            snapshot.push('\n');
        }

        assert!(snapshot.contains("Synthetic work item 00099"));
        assert!(!snapshot.contains("Synthetic work item 00000"));
        assert!(
            snapshot.contains('█'),
            "overflowing registry must render a scrollbar thumb"
        );
    }

    #[test]
    fn registry_viewport_uses_filtered_thread_count() {
        let mut app = AppState::new(FakeBackend::scaled(100).snapshot().threads);
        app.filter = "repo-001".into();
        let visible = app.visible_indices();
        app.selected = *visible.last().expect("filtered row");

        let (viewport_visible, viewport) = registry_viewport(&app, 10);
        assert_eq!(viewport.total, viewport_visible.len());
        assert_eq!(viewport.end, viewport.total);
        assert!(viewport.total < 100);
    }

    #[cfg(not(windows))]
    #[test]
    fn registry_marks_foreign_windows_cwd_without_linux_prefix() {
        let backend = TestBackend::new(160, 28);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.threads[0].metadata.cwd = r"/vsdata/repo/C:\Users\jun\repo".into();

        terminal.draw(|frame| render(frame, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut snapshot = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                snapshot.push_str(buffer[(x, y)].symbol());
            }
            snapshot.push('\n');
        }

        assert!(snapshot.contains("foreign-windows"));
        assert!(snapshot.contains(r"C:\Users\jun\repo"));
        assert!(!snapshot.contains(r"/vsdata/repo/C:\Users\jun\repo"));
        assert!(snapshot.contains("Git: skipped · cwd foreign-windows on this host"));
    }

    #[test]
    fn registry_summary_surfaces_host_local_mode() {
        let backend = TestBackend::new(120, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.threads[0].metadata.cwd = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned();
        app.host_local_only = true;

        terminal.draw(|frame| render(frame, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut snapshot = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                snapshot.push_str(buffer[(x, y)].symbol());
            }
            snapshot.push('\n');
        }

        assert!(snapshot.contains("LOCAL ONLY"));
        assert!(snapshot.contains("l local-only"));
    }

    #[test]
    fn registry_summary_surfaces_repo_only_mode() {
        let backend = TestBackend::new(120, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.threads[0].metadata.cwd = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned();
        app.repo_backed_only = true;

        terminal.draw(|frame| render(frame, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut snapshot = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                snapshot.push_str(buffer[(x, y)].symbol());
            }
            snapshot.push('\n');
        }

        assert!(snapshot.contains("REPO ONLY"));
        assert!(snapshot.contains("g repo-only"));
    }

    #[test]
    fn registry_footer_exposes_terminal_shortcut() {
        let snapshot = render_snapshot(100);
        assert!(
            snapshot.contains("t terminal"),
            "Mission Control must expose the Terminal Drawer shortcut"
        );
    }

    #[test]
    fn registry_surfaces_backend_provenance_and_terminal_readiness() {
        let backend = TestBackend::new(160, 16);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.backend_status.source = "codex-app-server".into();
        app.backend_status.connected = true;
        app.backend_status.platform = Some("linux/linux".into());
        app.backend_status.codex_home = Some("/home/jun/.codex".into());
        app.threads[0].metadata.cwd.clear();

        terminal.draw(|frame| render(frame, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut snapshot = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                snapshot.push_str(buffer[(x, y)].symbol());
            }
            snapshot.push('\n');
        }

        assert!(snapshot.contains("Mission Control · codex-app-server · linux/linux"));
        assert!(snapshot.contains("Selected"));
        assert!(!snapshot.contains("Backend: codex-app-server"));
        assert!(snapshot.contains("Codex home: /home/jun/.codex"));
        assert!(snapshot.contains("selected cwd: empty · terminal blocked · git skipped"));

        app.threads[0].metadata.cwd = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned();
        let status = registry_scope_status(&app, 160);
        assert!(status.contains("selected cwd: local · terminal ready · git not-probed"));
    }

    #[test]
    fn daily_selected_hides_unavailable_forge_internals_but_workspace_keeps_doctor_hint() {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        let thread = app.threads[0].clone();
        app.forge_observations.insert(
            thread.id.0.clone(),
            crate::forge::ForgeObservation::unavailable(
                thread.id.clone(),
                thread.metadata.cwd.clone(),
                "resolve GitLab project: glab api /projects/example failed",
            ),
        );

        assert!(forge_context_lines(&app, &thread.id, false).is_empty());
        let diagnostic = forge_context_lines(&app, &thread.id, true);
        let diagnostic_text = diagnostic
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("");
        assert!(diagnostic_text.contains("doctor forge"));
        assert!(!diagnostic_text.contains("resolve GitLab project"));
    }

    #[test]
    fn registry_scope_status_distinguishes_repository_backing() {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.threads[0].metadata.cwd = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned();
        let thread_id = app.threads[0].id.clone();

        assert_eq!(selected_git_status(&app), "not-probed");

        let pending =
            crate::git::GitContext::pending(thread_id.clone(), app.threads[0].metadata.cwd.clone());
        app.git_contexts.insert(thread_id.0.clone(), pending);
        assert_eq!(selected_git_status(&app), "probing");

        let context = app.git_contexts.get_mut(&thread_id.0).expect("git context");
        context.observed_at_unix_ms = 1;
        assert_eq!(selected_git_status(&app), "not-repo");

        app.git_contexts
            .get_mut(&thread_id.0)
            .expect("git context")
            .is_repository = true;
        assert_eq!(selected_git_status(&app), "repo");

        app.git_contexts
            .get_mut(&thread_id.0)
            .expect("git context")
            .error = Some("git unavailable".into());
        assert_eq!(selected_git_status(&app), "degraded");
    }

    #[test]
    fn registry_status_surfaces_terminal_failure_notice() {
        let backend = TestBackend::new(120, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.mutation_notice =
            Some("terminal drawer unavailable: selected Codex thread has no cwd".into());

        terminal.draw(|frame| render(frame, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut snapshot = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                snapshot.push_str(buffer[(x, y)].symbol());
            }
            snapshot.push('\n');
        }

        assert!(snapshot.contains("notice"));
        assert!(snapshot.contains("terminal drawer unavailable"));
    }

    #[test]
    fn compact_registry_handles_cjk_emoji_graphemes_and_control_text() {
        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.threads[0].workspace = "机器人研发中心".into();
        app.threads[0].alias = None;
        app.threads[0].title = "唤醒词👨‍👩‍👧‍👦 e\u{301} 测试\n控制\u{0007}字符".into();

        terminal.draw(|frame| render(frame, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut snapshot = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                snapshot.push_str(buffer[(x, y)].symbol());
            }
            snapshot.push('\n');
        }

        for glyph in ['机', '器', '人', '唤', '醒', '词'] {
            assert!(
                snapshot.contains(glyph),
                "wide glyph {glyph:?} missing from TestBackend buffer"
            );
        }
        assert!(!snapshot.contains('\u{0007}'));
    }

    #[test]
    fn terminal_drawer_preserves_semantic_cjk_rows() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.terminal_drawer_open = true;
        app.terminal_focused = false;
        app.terminal_snapshot = Some(crate::terminal_drawer::TerminalSnapshot {
            cwd: "/repo/机器人".into(),
            size: TerminalSize { rows: 7, cols: 78 },
            rows: vec!["中文终端 e\u{301} 👩‍💻".into()],
            cursor_row: 0,
            cursor_col: 0,
            scrollback: 0,
            state: crate::terminal_drawer::TerminalProcessState::Running,
        });

        terminal.draw(|frame| render(frame, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut snapshot = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                snapshot.push_str(buffer[(x, y)].symbol());
            }
            snapshot.push('\n');
        }
        for glyph in ['中', '文', '终', '端', '机', '器', '人'] {
            assert!(
                snapshot.contains(glyph),
                "wide glyph {glyph:?} missing from TestBackend buffer"
            );
        }
    }

    #[test]
    fn terminal_drawer_reserves_rows_instead_of_covering_main_view() {
        let full = Rect::new(0, 0, 100, 30);
        let drawer = terminal_drawer_rect(full);
        let main = primary_view_rect(full, true);
        assert_eq!(main.height + drawer.height, full.height);
        assert_eq!(main.y + main.height, drawer.y);

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = AppState::new(FakeBackend::scaled(100).snapshot().threads);
        app.selected = 99;
        app.terminal_drawer_open = true;
        app.terminal_snapshot = Some(crate::terminal_drawer::TerminalSnapshot {
            cwd: "/repo".into(),
            size: terminal_drawer_pty_size(100, 30),
            rows: vec!["drawer row".into()],
            cursor_row: 0,
            cursor_col: 0,
            scrollback: 0,
            state: crate::terminal_drawer::TerminalProcessState::Running,
        });

        terminal.draw(|frame| render(frame, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut scrollbar_rows = Vec::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if buffer[(x, y)].symbol() == "█" {
                    scrollbar_rows.push(y);
                }
            }
        }
        assert!(!scrollbar_rows.is_empty());
        assert!(
            scrollbar_rows.iter().all(|row| *row < drawer.y),
            "Mission Control scrollbar must stay above the Drawer"
        );

        let drawer_row = (drawer.y..drawer.y + drawer.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| buffer[(x, y)].symbol()))
            .collect::<String>();
        assert!(drawer_row.contains("Terminal Drawer"));
        assert!(drawer_row.contains("drawer row"));
    }

    #[test]
    fn terminal_drawer_overlay_renders_status_rows_and_focus_hint() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.terminal_drawer_open = true;
        app.terminal_focused = true;
        app.terminal_snapshot = Some(crate::terminal_drawer::TerminalSnapshot {
            cwd: "/repo".into(),
            size: TerminalSize { rows: 10, cols: 98 },
            rows: vec!["hello from PTY".into(), "$ ".into()],
            cursor_row: 1,
            cursor_col: 2,
            scrollback: 0,
            state: crate::terminal_drawer::TerminalProcessState::Running,
        });

        terminal.draw(|frame| render(frame, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut snapshot = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                snapshot.push_str(buffer[(x, y)].symbol());
            }
            snapshot.push('\n');
        }
        assert!(snapshot.contains("Terminal Drawer"));
        assert!(snapshot.contains("hello from PTY"));
        assert!(snapshot.contains("F6 release"));
    }

    #[test]
    fn terminal_drawer_size_is_bounded_and_accounts_for_border() {
        assert_eq!(
            terminal_drawer_pty_size(100, 30),
            TerminalSize { rows: 10, cols: 98 }
        );
        let small = terminal_drawer_pty_size(20, 8);
        assert!(small.rows > 0);
        assert!(small.cols > 0);
    }

    #[test]
    fn help_hints_match_locked_keyboard_commands() {
        use crate::app::ViewKind;
        use crate::keymap::{Command, command_for_key};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let help = HELP_LINES.join("\n");
        for (needle, key, view, command) in [
            (
                "? help",
                KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
                ViewKind::Registry,
                Command::Help,
            ),
            (
                "/ search",
                KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
                ViewKind::Registry,
                Command::Search,
            ),
            (
                ". actions",
                KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE),
                ViewKind::Registry,
                Command::ContextActions,
            ),
            (
                "t terminal",
                KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
                ViewKind::Workspace,
                Command::TerminalDrawer,
            ),
            (
                "T close terminal",
                KeyEvent::new(KeyCode::Char('T'), KeyModifiers::NONE),
                ViewKind::Workspace,
                Command::CloseTerminalDrawer,
            ),
        ] {
            assert!(help.contains(needle), "missing help hint {needle:?}");
            assert_eq!(command_for_key(key, view), Some(command));
        }
    }

    #[test]
    fn ui_source_does_not_require_private_use_icon_fonts() {
        fn private_use(ch: char) -> bool {
            matches!(
                ch as u32,
                0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd
            )
        }

        assert!(
            !include_str!("ui.rs").chars().any(private_use),
            "UI source must not depend on Nerd Font/private-use glyphs"
        );
    }

    #[test]
    fn very_narrow_registry_layouts_render_without_panicking() {
        for width in [20, 30, 40] {
            let snapshot = render_snapshot(width);
            assert!(!snapshot.is_empty(), "width={width}");
        }
    }

    #[test]
    fn responsive_layout_breakpoints_are_locked() {
        assert_eq!(layout_mode(40), LayoutMode::Compact);
        assert_eq!(layout_mode(80), LayoutMode::Standard);
        assert_eq!(layout_mode(120), LayoutMode::Wide);
        assert_eq!(layout_mode(160), LayoutMode::Wide);
    }

    #[test]
    fn snapshots_cover_40_80_120_160_columns() {
        for width in [40, 80, 120, 160] {
            let snapshot = render_snapshot(width);
            assert!(snapshot.contains("Mission Control"), "width={width}");
            assert!(snapshot.contains("M0 bootstrap"), "width={width}");
            if width >= 120 {
                assert!(snapshot.contains("Selected"), "width={width}");
            }
        }
    }
}
