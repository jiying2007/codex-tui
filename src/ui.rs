use crate::app::{AppState, ContextChoice, InputMode, View};
use crate::conversation::{InteractiveRequest, InteractiveRequestKind};
use crate::domain::{AttentionReason, CwdLocality, RuntimeStatus, display_cwd};
use crate::forge::ForgeFreshness;
use crate::goal::GoalStatus;
use crate::i18n::{UiLanguage, pick};
use crate::operation::OperationState;
use crate::planning::{
    PlanningAttention, SavedView, SavedViewLayout, ScratchState, WorkflowStage, apply_saved_view,
    saved_view_group_key,
};
use crate::text::{fit_display, sanitize_inline, truncate_display};

mod overlays;
mod review;
mod terminal;
mod thread;

#[cfg(test)]
use crate::{pty::TerminalSize, terminal_drawer::TerminalProcessState};
use overlays::{
    render_command_palette, render_context_actions, render_forge_mutation_confirmation,
    render_launch_confirmation, render_launch_presets, render_local_batch_confirmation,
    render_local_input_overlay, render_saved_view_editor, render_thread_queue,
    render_transcript_search,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use terminal::render_terminal_drawer;
#[cfg(test)]
use terminal::terminal_process_state_label;
pub use terminal::{terminal_drawer_pty_size, terminal_drawer_rect};

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

fn runtime_status_label(status: &RuntimeStatus, language: UiLanguage) -> &'static str {
    match (status, language) {
        (RuntimeStatus::Working, UiLanguage::SimplifiedChinese) => "运行中",
        (RuntimeStatus::WaitingHuman, UiLanguage::SimplifiedChinese) => "等待人工",
        (RuntimeStatus::Ready, UiLanguage::SimplifiedChinese) => "就绪",
        (RuntimeStatus::SystemError, UiLanguage::SimplifiedChinese) => "错误",
        (RuntimeStatus::Inactive, UiLanguage::SimplifiedChinese) => "空闲",
        _ => status.label(),
    }
}

fn attention_reason_label(reason: &AttentionReason, language: UiLanguage) -> &'static str {
    match (reason, language) {
        (AttentionReason::ApprovalRequired, UiLanguage::SimplifiedChinese) => "审批",
        (AttentionReason::UserInputRequired, UiLanguage::SimplifiedChinese) => "输入",
        (AttentionReason::ReadyForReview, UiLanguage::SimplifiedChinese) => "评审",
        (AttentionReason::SystemError, UiLanguage::SimplifiedChinese) => "错误",
        (AttentionReason::MarkedUnread, UiLanguage::SimplifiedChinese) => "未读",
        _ => reason.label(),
    }
}

fn planning_attention_label(reason: &PlanningAttention, language: UiLanguage) -> &'static str {
    match (reason, language) {
        (PlanningAttention::ApprovalRequired, UiLanguage::SimplifiedChinese) => "审批",
        (PlanningAttention::UserInputRequired, UiLanguage::SimplifiedChinese) => "输入",
        (PlanningAttention::SystemError, UiLanguage::SimplifiedChinese) => "错误",
        (PlanningAttention::MarkedUnread, UiLanguage::SimplifiedChinese) => "未读",
        (PlanningAttention::ConflictRisk, UiLanguage::SimplifiedChinese) => "冲突",
        (PlanningAttention::GoalBlocked, UiLanguage::SimplifiedChinese) => "目标受阻",
        (PlanningAttention::UsageLimited, UiLanguage::SimplifiedChinese) => "用量受限",
        (PlanningAttention::BudgetLimited, UiLanguage::SimplifiedChinese) => "预算受限",
        (PlanningAttention::ReviewUnseen, UiLanguage::SimplifiedChinese) => "评审",
        (PlanningAttention::PipelineFailed, UiLanguage::SimplifiedChinese) => "流水线失败",
        (PlanningAttention::ChangeRequested, UiLanguage::SimplifiedChinese) => "请求修改",
        _ => reason.label(),
    }
}

fn cwd_locality_label(locality: CwdLocality, language: UiLanguage) -> &'static str {
    match (locality, language) {
        (CwdLocality::LocalDirectory, UiLanguage::SimplifiedChinese) => "本机",
        (CwdLocality::NativeMissing, UiLanguage::SimplifiedChinese) => "失效",
        (CwdLocality::ForeignWindows, UiLanguage::SimplifiedChinese) => "外部-Windows",
        (CwdLocality::ForeignUnix, UiLanguage::SimplifiedChinese) => "外部-Unix",
        (CwdLocality::Relative, UiLanguage::SimplifiedChinese) => "相对路径",
        (CwdLocality::Empty, UiLanguage::SimplifiedChinese) => "空",
        _ => locality.label(),
    }
}

fn cwd_locality_display_label(locality: Option<CwdLocality>, language: UiLanguage) -> &'static str {
    locality.map_or_else(
        || tr_language(language, "unprobed", "未探测"),
        |locality| cwd_locality_label(locality, language),
    )
}

fn goal_status_label(status: GoalStatus, language: UiLanguage) -> &'static str {
    match (status, language) {
        (GoalStatus::Active, UiLanguage::SimplifiedChinese) => "进行中",
        (GoalStatus::Paused, UiLanguage::SimplifiedChinese) => "已暂停",
        (GoalStatus::Blocked, UiLanguage::SimplifiedChinese) => "受阻",
        (GoalStatus::UsageLimited, UiLanguage::SimplifiedChinese) => "用量受限",
        (GoalStatus::BudgetLimited, UiLanguage::SimplifiedChinese) => "预算受限",
        (GoalStatus::Complete, UiLanguage::SimplifiedChinese) => "完成",
        _ => status.label(),
    }
}

fn scratch_state_label(state: ScratchState, language: UiLanguage) -> &'static str {
    match (state, language) {
        (ScratchState::Inbox, UiLanguage::SimplifiedChinese) => "收件箱",
        (ScratchState::Ready, UiLanguage::SimplifiedChinese) => "就绪",
        (ScratchState::Done, UiLanguage::SimplifiedChinese) => "完成",
        (ScratchState::Inbox, UiLanguage::English) => "Inbox",
        (ScratchState::Ready, UiLanguage::English) => "Ready",
        (ScratchState::Done, UiLanguage::English) => "Done",
    }
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

fn forge_freshness_label(freshness: ForgeFreshness, language: UiLanguage) -> &'static str {
    match (freshness, language) {
        (ForgeFreshness::Fresh, UiLanguage::SimplifiedChinese) => "最新",
        (ForgeFreshness::Aging, UiLanguage::SimplifiedChinese) => "稍旧",
        (ForgeFreshness::Stale, UiLanguage::SimplifiedChinese) => "过期",
        (ForgeFreshness::Unavailable, UiLanguage::SimplifiedChinese) => "不可用",
        _ => freshness.label(),
    }
}

fn operation_state_label(state: OperationState, language: UiLanguage) -> &'static str {
    match (state, language) {
        (OperationState::Planned, UiLanguage::SimplifiedChinese) => "已计划",
        (OperationState::Executing, UiLanguage::SimplifiedChinese) => "执行中",
        (OperationState::Succeeded, UiLanguage::SimplifiedChinese) => "已成功",
        (OperationState::Failed, UiLanguage::SimplifiedChinese) => "失败",
        (OperationState::OutcomeUnknown, UiLanguage::SimplifiedChinese) => "结果未知",
        (OperationState::Planned, UiLanguage::English) => "planned",
        (OperationState::Executing, UiLanguage::English) => "executing",
        (OperationState::Succeeded, UiLanguage::English) => "succeeded",
        (OperationState::Failed, UiLanguage::English) => "failed",
        (OperationState::OutcomeUnknown, UiLanguage::English) => "outcome unknown",
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
        ContextChoice::EditCurrentView => "编辑当前已保存视图…",
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
        ContextChoice::NewCodexThread => "新建 Codex 会话",
        ContextChoice::ForkCodexThread => "分叉 Codex 会话",
        ContextChoice::ForgeCreateMergeRequest => "Forge · 创建合并请求…",
        ContextChoice::ForgeComment => "Forge · 评论合并请求…",
        ContextChoice::ForgeApprove => "Forge · 批准合并请求",
        ContextChoice::ForgeMerge => "Forge · 合并合并请求",
    }
}

fn saved_view_name(view: &SavedView, language: UiLanguage) -> &str {
    if language == UiLanguage::English {
        return &view.name;
    }
    match view.id.as_str() {
        "builtin:all" => "全部工作",
        "builtin:attention" => "需要你处理",
        "builtin:review" => "需要评审",
        "builtin:forge" => "Forge 工作",
        _ => &view.name,
    }
}

fn saved_view_layout_label(layout: SavedViewLayout, language: UiLanguage) -> &'static str {
    match (layout, language) {
        (SavedViewLayout::List, UiLanguage::SimplifiedChinese) => "列表",
        (SavedViewLayout::Board, UiLanguage::SimplifiedChinese) => "看板",
        (SavedViewLayout::ReviewQueue, UiLanguage::SimplifiedChinese) => "评审队列",
        _ => layout.label(),
    }
}

pub fn render(frame: &mut Frame<'_>, app: &AppState) {
    let content_area = primary_view_rect(frame.area(), app.terminal_drawer_open);
    match &app.view {
        View::Registry => render_registry(frame, app, content_area),
        View::Thread(id) => thread::render_thread(frame, app, id.0.as_str(), content_area),
        View::Review(id) => review::render_review(frame, app, id.0.as_str(), content_area),
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
    if app.transcript_search_open {
        render_transcript_search(frame, app);
    }
    if app.thread_queue_open {
        render_thread_queue(frame, app);
    }
    if app.saved_view_editor.is_some() {
        render_saved_view_editor(frame, app);
    }
    if app.command_palette_open {
        render_command_palette(frame, app);
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
        InputMode::TranscriptSearch
            | InputMode::ThreadQueueAdd
            | InputMode::ThreadQueueEdit
            | InputMode::Note
            | InputMode::Snooze
            | InputMode::SavedViewField
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
        InputMode::TranscriptSearch => Line::from(format!(
            "Ctrl+F> {}  · {}",
            app.input_buffer,
            tr(
                app,
                "Enter full-history search · Esc cancel",
                "Enter 全历史搜索 · Esc 取消"
            )
        )),
        InputMode::ThreadQueueAdd | InputMode::ThreadQueueEdit => Line::from(format!(
            "{}> {}  · {}",
            tr(app, "queue", "队列"),
            truncate_display(&app.input_buffer, 60),
            tr(app, "Enter submit · Esc cancel", "Enter 提交 · Esc 取消")
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
        InputMode::SavedViewField => Line::from(format!(
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
            } else if !app.backend_status.registry_complete {
                Line::from(format!(
                    "{} · {}",
                    tr(app, "loading full history", "正在加载完整历史"),
                    registry_scope_status(app, area.width)
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
            Span::raw(tr(app, "Ctrl+F history  ", "Ctrl+F 全文  ")),
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
        .unwrap_or_else(|| tr(app, "<unknown>", "<未知>"))
}

fn backend_home_label(app: &AppState) -> &str {
    app.backend_status
        .codex_home
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| tr(app, "<unknown>", "<未知>"))
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
    if app
        .cwd_locality_for_display(&thread.metadata.cwd)
        .is_some_and(|locality| !locality.terminal_usable())
    {
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
            let locality = app.cwd_locality_for_display(&thread.metadata.cwd);
            (
                cwd_locality_display_label(locality, app.language),
                match locality {
                    Some(CwdLocality::LocalDirectory) => tr(app, "ready", "就绪"),
                    Some(_) => tr(app, "blocked", "不可用"),
                    None => tr(app, "unchecked", "未检查"),
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

fn registry_history_label(
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
            "{scope}{history} · 显示 {}/{} 匹配 · 共 {} · 本机 {local_count} · 外部 {foreign_count} · 失效 {stale_count} · 未探测 {unprobed_count} · 待处理 {} · {range}{text_filter}",
            visible.len(),
            matched,
            app.threads.len(),
            attention_count
        )
    } else {
        format!(
            "{scope}{history} {}/{} matched · {} total · {local_count} local · {foreign_count} foreign · {stale_count} stale · {unprobed_count} unprobed · {} need attention · {range}{text_filter}",
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
                .map(|reason| attention_reason_label(reason, app.language))
                .collect::<Vec<_>>();
            if pending_interactive {
                reasons.push(tr(app, "interactive", "交互"));
            }
            reasons.join(",")
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
                fit_display(runtime_status_label(&thread.runtime, app.language), 7),
                fit_display(&thread.workspace, 12),
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

fn detail_panel(app: &AppState) -> Paragraph<'static> {
    let mut lines = if let Some(thread) = app.selected_thread() {
        let attention = if thread.attention.is_empty() {
            tr(app, "none", "无").into()
        } else {
            thread
                .attention
                .iter()
                .map(|reason| attention_reason_label(reason, app.language))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let planning = app
            .work_card_for_thread(&thread.id)
            .map(|card| workflow_stage_label(card.stage, app.language))
            .unwrap_or_else(|| tr(app, "<unprojected>", "<未投影>"));
        let mut lines = if app.language.is_simplified_chinese() {
            vec![
                Line::from(format!("会话: {}", thread.id)),
                Line::from(format!("工作区: {}", sanitize_inline(&thread.workspace))),
                Line::from(format!(
                    "运行状态: {} · 待处理: {attention}",
                    runtime_status_label(&thread.runtime, app.language)
                )),
                Line::from(format!(
                    "模型: {}",
                    thread
                        .metadata
                        .model
                        .as_deref()
                        .unwrap_or_else(|| tr(app, "unknown", "未知"))
                )),
                Line::from(format!(
                    "Cwd [{}]: {}",
                    cwd_locality_display_label(
                        app.cwd_locality_for_display(&thread.metadata.cwd),
                        app.language,
                    ),
                    sanitize_inline(display_cwd(&thread.metadata.cwd))
                )),
                Line::from(format!("计划状态: {planning}")),
                Line::from(format!(
                    "{}: {}",
                    tr(app, "Goal", "目标"),
                    goal_summary(app, &thread.id.0)
                )),
            ]
        } else {
            vec![
                Line::from(format!("Thread: {}", thread.id)),
                Line::from(format!("Workspace: {}", sanitize_inline(&thread.workspace))),
                Line::from(format!(
                    "Runtime: {} · attention: {attention}",
                    runtime_status_label(&thread.runtime, app.language)
                )),
                Line::from(format!(
                    "Model: {}",
                    thread
                        .metadata
                        .model
                        .as_deref()
                        .unwrap_or_else(|| tr(app, "unknown", "未知"))
                )),
                Line::from(format!(
                    "Cwd [{}]: {}",
                    cwd_locality_display_label(
                        app.cwd_locality_for_display(&thread.metadata.cwd),
                        app.language,
                    ),
                    sanitize_inline(display_cwd(&thread.metadata.cwd))
                )),
                Line::from(format!("Planning: {planning}")),
                Line::from(format!(
                    "{}: {}",
                    tr(app, "Goal", "目标"),
                    goal_summary(app, &thread.id.0)
                )),
            ]
        };
        if let Some(note) = app
            .work_card_for_thread(&thread.id)
            .and_then(|card| card.overlay.note.as_deref())
            .filter(|note| !note.trim().is_empty())
        {
            lines.push(Line::from(format!(
                "{}: {}",
                tr(app, "Note", "备注"),
                truncate_display(&sanitize_inline(note), 54)
            )));
        }
        lines
    } else {
        vec![Line::from(tr(app, "No thread selected", "未选择会话"))]
    };

    if let Some(thread) = app.selected_thread() {
        lines.push(Line::from(""));
        let locality = app.cwd_locality_for_display(&thread.metadata.cwd);
        if locality.is_some_and(|locality| !locality.terminal_usable()) {
            lines.push(Line::from(if app.language.is_simplified_chinese() {
                format!(
                    "Git: 已跳过 · cwd {} 不属于本机",
                    cwd_locality_display_label(locality, app.language)
                )
            } else {
                format!(
                    "Git: skipped · cwd {} on this host",
                    cwd_locality_display_label(locality, app.language)
                )
            }));
        } else {
            match app.git_context(&thread.id) {
                None => lines.push(Line::from(tr(app, "Git: not probed", "Git: 未探测"))),
                Some(context) if context.observed_at_unix_ms == 0 => {
                    lines.push(Line::from(tr(app, "Git: probing…", "Git: 探测中…")));
                }
                Some(context) if context.error.is_some() => {
                    lines.push(Line::from(format!(
                        "{} · {}",
                        tr(app, "Git: degraded", "Git: 已降级"),
                        context.error.as_deref().unwrap_or_else(|| tr(
                            app,
                            "unknown error",
                            "未知错误"
                        ))
                    )));
                }
                Some(context) if !context.is_repository => {
                    lines.push(Line::from(tr(
                        app,
                        "Git: not a repository",
                        "Git: 不是仓库",
                    )));
                }
                Some(context) => {
                    let branch = context
                        .branch
                        .as_deref()
                        .or(context.head.as_deref())
                        .unwrap_or_else(|| tr(app, "unknown", "未知"));
                    lines.push(Line::from(format!("Git: {branch}")));
                    lines.push(Line::from(if app.language.is_simplified_chinese() {
                        format!(
                            "脏状态: {} · 文件={} · +{} -{}",
                            context.dirty,
                            context.changes.len(),
                            context.ahead,
                            context.behind
                        )
                    } else {
                        format!(
                            "Dirty: {} · files={} · +{} -{}",
                            context.dirty,
                            context.changes.len(),
                            context.ahead,
                            context.behind
                        )
                    }));
                    if let Some(worktree) = &context.worktree {
                        lines.push(Line::from(format!(
                            "Worktree: {}",
                            truncate_display(&worktree.canonical_path, 42)
                        )));
                    }
                    if let Some(repo) = &context.repo {
                        lines.push(Line::from(format!(
                            "{}: {}",
                            tr(app, "Repo", "仓库"),
                            truncate_display(&repo.primary_root, 42)
                        )));
                    }
                    let collisions = app.worktree_collision_count(&thread.id);
                    if collisions > 0 {
                        lines.push(Line::from(if app.language.is_simplified_chinese() {
                            format!("警告: 与 {collisions} 个活跃会话共享可变 checkout")
                        } else {
                            format!(
                                "WARNING: shared mutable checkout with {collisions} active thread(s)"
                            )
                        }));
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
        .block(Block::bordered().title(tr(app, " Selected ", " 已选择 ")))
        .wrap(Wrap { trim: false })
}

fn forge_context_lines(
    app: &AppState,
    thread_id: &crate::domain::ThreadId,
    diagnostic_hint: bool,
) -> Vec<Line<'static>> {
    let Some(observation) = app.forge_observation(thread_id) else {
        return if diagnostic_hint {
            vec![Line::from(tr(app, "Forge: not probed", "Forge: 未探测"))]
        } else {
            vec![]
        };
    };

    if observation.observed_at_unix_ms == 0 {
        return if diagnostic_hint {
            vec![Line::from(tr(app, "Forge: probing…", "Forge: 探测中…"))]
        } else {
            vec![]
        };
    }

    let Some(identity) = &observation.identity else {
        return if diagnostic_hint {
            vec![Line::from(tr(
                app,
                "Forge: unavailable · run codex-tui doctor forge for details",
                "Forge: 不可用 · 运行 codex-tui doctor forge 查看详情",
            ))]
        } else {
            vec![]
        };
    };

    let mut lines = vec![Line::from(format!(
        "Forge: {} · {}/{} · {}",
        identity.provider.label(),
        identity.host,
        identity.path_with_namespace,
        forge_freshness_label(
            observation.freshness_at(crate::operation::now_unix_ms()),
            app.language,
        )
    ))];

    let branch = app
        .git_context(thread_id)
        .and_then(|context| context.branch.as_deref());
    if let Some(branch) = branch {
        if let Some(change) = observation.change_request_for_branch(branch) {
            lines.push(Line::from(format!(
                "CR: {} · {}{} · {}",
                change.iid,
                if change.draft {
                    tr(app, "draft · ", "草稿 · ")
                } else {
                    ""
                },
                change.state,
                truncate_display(&change.title, 58)
            )));
        } else if app.language.is_simplified_chinese() {
            lines.push(Line::from(format!("CR: 分支 {branch} 没有合并请求")));
        } else {
            lines.push(Line::from(format!("CR: none for branch {branch}")));
        }
        if let Some(pipeline) = observation.pipeline_for_branch(branch) {
            lines.push(Line::from(format!(
                "Pipeline: #{} · {}",
                pipeline.id, pipeline.status
            )));
        } else {
            lines.push(Line::from(tr(
                app,
                "Pipeline: none for current branch",
                "Pipeline: 当前分支没有流水线",
            )));
        }
    } else {
        lines.push(Line::from(tr(
            app,
            "CR/Pipeline: current branch unavailable",
            "CR/Pipeline: 当前分支不可用",
        )));
    }

    if let Some(notice) = &app.mutation_notice {
        lines.push(Line::from(format!(
            "{}: {}",
            tr(app, "Mutation", "变更"),
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
        return if app.language.is_simplified_chinese() {
            format!(
                "{} · {} · token={} · {}秒",
                goal_status_label(goal.status, app.language),
                truncate_display(&goal.objective, 42),
                budget,
                goal.time_used_seconds
            )
        } else {
            format!(
                "{} · {} · tokens={} · {}s",
                goal_status_label(goal.status, app.language),
                truncate_display(&goal.objective, 42),
                budget,
                goal.time_used_seconds
            )
        };
    }
    if app
        .backend_status
        .optional_capabilities_missing
        .iter()
        .any(|capability| capability == "thread/goal/get")
    {
        return tr(
            app,
            "unavailable on this App Server",
            "当前 App Server 不支持",
        )
        .into();
    }
    if app.goal_checked.contains(thread_id) {
        return tr(app, "none", "无").into();
    }
    tr(app, "probing…", "探测中…").into()
}

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
                        planning_card_line(card, selected, app.language, &view.visible_fields)
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(
                    Paragraph::new(lines)
                        .block(Block::bordered().title(format!(
                            " {} ({}) ",
                            workflow_stage_label(*stage, app.language),
                            stage_cards.len()
                        )))
                        .wrap(Wrap { trim: false }),
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
                    .wrap(Wrap { trim: false }),
                outer[0],
            );
            render_board_scrollbar(frame, outer[0], cards.len(), viewport);
        }
        SavedViewLayout::List | SavedViewLayout::ReviewQueue => {
            let cards = app.visible_planning_cards();
            let mut lines = Vec::new();
            let mut previous_group = String::new();
            let mut selected_line = 0_usize;
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
            let total = lines.len();
            let viewport = board_viewport(total, selected_line, outer[0].height);
            let visible_lines = lines
                .into_iter()
                .skip(viewport.start)
                .take(viewport.row_capacity)
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(visible_lines)
                    .block(Block::bordered().title(format!(
                        " {} · {} ",
                        saved_view_name(&view, app.language),
                        saved_view_layout_label(view.layout, app.language)
                    )))
                    .wrap(Wrap { trim: false }),
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
    } else if app.hot_slot_bind_pending {
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
    frame.render_widget(Paragraph::new(input), outer[1]);
}

fn planning_card_line(
    card: &crate::planning::WorkCardProjection,
    selected: bool,
    language: UiLanguage,
    visible_fields: &[String],
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
    Line::from(Span::styled(text, style))
}

fn planning_card_field(
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
                    if card.change_request_draft {
                        format!("{state}/draft")
                    } else {
                        state.to_string()
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
            Line::from(format!(
                "{}: {}",
                tr(app, "Title", "标题"),
                sanitize_inline(&scratch.title)
            )),
            Line::from(format!(
                "{}: {} · {}={}",
                tr(app, "State", "状态"),
                scratch_state_label(scratch.state, app.language),
                tr(app, "priority", "优先级"),
                scratch
                    .priority
                    .map_or_else(|| tr(app, "none", "无").into(), |value| value.to_string())
            )),
            Line::from(format!(
                "{}: {}",
                tr(app, "Workspace", "工作区"),
                sanitize_inline(
                    scratch
                        .workspace
                        .as_deref()
                        .unwrap_or_else(|| tr(app, "<none>", "<无>"))
                )
            )),
            Line::from(format!(
                "{}: {}",
                tr(app, "Note", "备注"),
                sanitize_inline(
                    scratch
                        .note
                        .as_deref()
                        .unwrap_or_else(|| tr(app, "<empty>", "<空>"))
                )
            )),
            Line::from(""),
            Line::from(tr(
                app,
                "Local ScratchWork only. It is not a Codex thread, Git work item, or forge issue.",
                "仅为本地 ScratchWork；它不是 Codex 会话、Git 工作项或 Forge Issue。",
            )),
        ]
    } else {
        vec![Line::from(tr(
            app,
            "ScratchWork no longer exists.",
            "ScratchWork 已不存在。",
        ))]
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(tr(app, " ScratchWork ", " 本地 ScratchWork ")))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(tr(app, "Esc back to Board", "Esc 返回看板")),
        chunks[1],
    );
}

fn render_workspace(frame: &mut Frame<'_>, app: &AppState, thread_id: &str, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(1)])
        .split(area);

    let thread = app.threads.iter().find(|thread| thread.id.0 == thread_id);
    let Some(thread) = thread else {
        frame.render_widget(
            Paragraph::new(tr(app, "Thread no longer exists.", "会话已不存在。"))
                .block(Block::bordered().title(tr(app, " Workspace ", " 工作区 "))),
            chunks[0],
        );
        return;
    };

    let mut lines = vec![
        Line::from(format!("{}: {}", tr(app, "Thread", "会话"), thread.id)),
        Line::from(format!("Cwd: {}", thread.metadata.cwd)),
    ];

    match app.git_context(&thread.id) {
        None => lines.push(Line::from(tr(app, "Git: not probed", "Git: 未探测"))),
        Some(context) if context.observed_at_unix_ms == 0 => {
            lines.push(Line::from(tr(app, "Git: probing…", "Git: 探测中…")));
        }
        Some(context) if context.error.is_some() => {
            lines.push(Line::from(format!(
                "{} · {}",
                tr(app, "Git: degraded", "Git: 已降级"),
                context
                    .error
                    .as_deref()
                    .unwrap_or_else(|| tr(app, "unknown error", "未知错误"))
            )));
        }
        Some(context) if !context.is_repository => {
            lines.push(Line::from(tr(
                app,
                "Git: not a repository",
                "Git: 不是仓库",
            )));
        }
        Some(context) => {
            if let Some(repo) = &context.repo {
                lines.push(Line::from(format!(
                    "{}: {}",
                    tr(app, "Repo root", "仓库根目录"),
                    repo.primary_root
                )));
                lines.push(Line::from(format!(
                    "Git common dir: {}",
                    repo.git_common_dir
                )));
            }
            if let Some(worktree) = &context.worktree {
                lines.push(Line::from(format!("Worktree: {}", worktree.canonical_path)));
            }
            lines.push(Line::from(format!(
                "{}: {}",
                tr(app, "Branch", "分支"),
                context
                    .branch
                    .as_deref()
                    .or(context.head.as_deref())
                    .unwrap_or_else(|| tr(app, "<unknown>", "<未知>"))
            )));
            lines.push(Line::from(if app.language.is_simplified_chinese() {
                format!(
                    "上游: {} · 领先={} 落后={}",
                    context
                        .upstream
                        .as_deref()
                        .unwrap_or_else(|| tr(app, "<none>", "<无>")),
                    context.ahead,
                    context.behind
                )
            } else {
                format!(
                    "Upstream: {} · ahead={} behind={}",
                    context
                        .upstream
                        .as_deref()
                        .unwrap_or_else(|| tr(app, "<none>", "<无>")),
                    context.ahead,
                    context.behind
                )
            }));
            lines.push(Line::from(if app.language.is_simplified_chinese() {
                format!(
                    "脏状态: {} · 变更文件={}",
                    context.dirty,
                    context.changes.len()
                )
            } else {
                format!(
                    "Dirty: {} · changed files={}",
                    context.dirty,
                    context.changes.len()
                )
            }));
            let collisions = app.worktree_collision_count(&thread.id);
            if collisions > 0 {
                lines.push(Line::from(if app.language.is_simplified_chinese() {
                    format!("警告: 与 {collisions} 个活跃会话共享可变 checkout")
                } else {
                    format!("WARNING: shared mutable checkout with {collisions} active thread(s)")
                }));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(tr(app, "Changed files:", "变更文件:")));
            lines.extend(context.changes.iter().take(100).map(|change| {
                Line::from(format!(
                    "  {:2} {}",
                    change.status_label(),
                    sanitize_inline(&change.path)
                ))
            }));
            if context.changes.len() > 100 {
                lines.push(Line::from(if app.language.is_simplified_chinese() {
                    format!("  … 另有 {} 个变更", context.changes.len() - 100)
                } else {
                    format!("  … {} additional change(s)", context.changes.len() - 100)
                }));
            }
        }
    }

    lines.push(Line::from(""));
    lines.extend(forge_context_lines(app, &thread.id, true));

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(tr(
                app,
                " Workspace · Git + Forge ",
                " 工作区 · Git + Forge ",
            )))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(tr(
            app,
            "r review · m managed worktrees · . actions · Esc back",
            "r 评审 · m 受管 worktree · . 操作 · Esc 返回",
        )),
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
        "{}: {}",
        tr(app, "Repository", "仓库"),
        repo.map(|repo| repo.primary_root.as_str())
            .unwrap_or_else(|| tr(app, "<unavailable>", "<不可用>"))
    )));
    lines.push(Line::from(format!(
        "{}: {}",
        tr(app, "Managed/adopted worktrees", "受管/已接管 worktree"),
        worktrees.len()
    )));
    lines.push(Line::from(""));

    if worktrees.is_empty() {
        lines.push(Line::from(tr(
            app,
            "No managed/adopted worktrees for this repository.",
            "此仓库没有受管或已接管的 worktree。",
        )));
    } else {
        for (index, record) in worktrees.iter().enumerate() {
            let selected = index == app.managed_selected;
            let prefix = if selected { ">" } else { " " };
            let ownership = if record.adopted {
                tr(app, "adopted", "已接管")
            } else {
                tr(app, "managed", "受管")
            };
            let text = format!(
                "{prefix} {} {} {}",
                fit_display(ownership, 8),
                fit_display(
                    record.branch.as_deref().unwrap_or_else(|| tr(
                        app,
                        "<detached>",
                        "<分离 HEAD>"
                    )),
                    18
                ),
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
        lines.push(Line::from(tr(
            app,
            "CONFIRM REQUIRED — no mutation has executed yet.",
            "需要确认 — 尚未执行任何变更。",
        )));
        lines.push(Line::from(format!(
            "{}: {}",
            tr(app, "Operation", "操作"),
            plan.kind.label()
        )));
        lines.push(Line::from(format!("Cwd: {}", plan.cwd)));
        let command = if plan.argv.is_empty() {
            tr(
                app,
                "metadata-only adoption (no Git mutation)",
                "仅接管元数据（不执行 Git 变更）",
            )
            .to_string()
        } else {
            let argv = plan
                .argv
                .iter()
                .map(|argument| format!("{argument:?}"))
                .collect::<Vec<_>>()
                .join(" ");
            format!("git -C {:?} {argv}", plan.cwd)
        };
        lines.push(Line::from(format!(
            "{}: {command}",
            tr(app, "Exact operation", "精确操作")
        )));
        lines.push(Line::from(format!(
            "{}: {}",
            tr(app, "Expected", "预期结果"),
            plan.expected_side_effect
        )));
        if let Some(path) = &plan.target_worktree {
            lines.push(Line::from(format!(
                "{}: {path}",
                tr(app, "Target worktree", "目标 worktree")
            )));
        }
        if let Some(branch) = &plan.target_branch {
            lines.push(Line::from(format!(
                "{}: {branch}",
                tr(app, "Target branch", "目标分支")
            )));
        }
        lines.push(Line::from(tr(app, "Preconditions:", "前置条件:")));
        lines.extend(plan.preconditions.iter().map(|precondition| {
            Line::from(format!(
                "  {} = {}",
                precondition.key, precondition.expected
            ))
        }));
        lines.push(Line::from(tr(
            app,
            "Press y to execute; c or Esc cancels.",
            "按 y 执行；c 或 Esc 取消。",
        )));
    }

    if let Some(receipt) = app.recent_operations.first() {
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "{}: {} · {}",
            tr(app, "Latest receipt", "最新回执"),
            receipt.plan.kind.label(),
            operation_state_label(receipt.state, app.language)
        )));
        if let Some(verification) = &receipt.verification {
            lines.push(Line::from(format!(
                "{}: {}",
                tr(app, "Verified", "验证"),
                truncate_display(verification, 90)
            )));
        }
        if let Some(failure) = &receipt.failure {
            lines.push(Line::from(format!(
                "{}: {}",
                tr(app, "Failure", "失败"),
                truncate_display(failure, 90)
            )));
        }
    }

    if let Some(notice) = &app.mutation_notice {
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "{}: {}",
            tr(app, "Notice", "提示"),
            truncate_display(notice, 100)
        )));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(tr(
                app,
                " Managed Worktrees · Plan → Confirm → Verify ",
                " 受管 Worktree · 计划 → 确认 → 验证 ",
            )))
            .wrap(Wrap { trim: false }),
        outer[0],
    );

    let footer = match app.input_mode {
        InputMode::WorktreeCreateBranch => format!(
            "{}> {} · {}",
            tr(app, "new branch", "新分支"),
            app.input_buffer,
            tr(app, "Enter next · Esc cancel", "Enter 下一步 · Esc 取消")
        ),
        InputMode::WorktreeCreatePath => format!(
            "{}> {} · {}",
            tr(app, "absolute worktree path", "worktree 绝对路径"),
            app.input_buffer,
            tr(app, "Enter next · Esc cancel", "Enter 下一步 · Esc 取消")
        ),
        InputMode::WorktreeCreateStartPoint => format!(
            "{}> {} · {}",
            tr(app, "start point", "起点"),
            app.input_buffer,
            tr(app, "Enter plan · Esc cancel", "Enter 生成计划 · Esc 取消")
        ),
        InputMode::WorktreeDeleteBranch => format!(
            "{}> {} · {}",
            tr(app, "branch to delete", "要删除的分支"),
            app.input_buffer,
            tr(app, "Enter plan · Esc cancel", "Enter 生成计划 · Esc 取消")
        ),
        _ if app.pending_operation.is_some() => tr(
            app,
            "y CONFIRM execute · c cancel plan · Esc cancel plan",
            "y 确认执行 · c 取消计划 · Esc 取消计划",
        )
        .into(),
        _ => tr(
            app,
            "j/k select · n create · a adopt current · d remove selected · x delete branch · Esc back",
            "j/k 选择 · n 创建 · a 接管当前 · d 移除已选 · x 删除分支 · Esc 返回",
        )
        .into(),
    };
    frame.render_widget(Paragraph::new(footer), outer[1]);
}

const HELP_LINES: &[&str] = &[
    "Global: ? help · Ctrl+K palette · / search · Ctrl+F transcript · . actions · Esc back",
    "Registry: j/k · Enter · Space attention · l local-only · g repo-only · h recent/all-history · p pin · e alias · x ack",
    "Thread: a composer · q Thread Queue · y/n/c approval · i answer · Ctrl+C interrupt · r review",
    "Review: j/k file · w word-diff · e editor · o external · . Forge actions · PageUp/PageDown · Esc",
    "Workspace: . actions/launch presets · r review · m worktrees · Esc",
    "Managed Worktrees: n create · a adopt · d remove · x delete branch · y confirm",
    "Board: h/l stage · j/k item · Space attention · s snooze · = bind · 1–9 hot slot",
    "Board: Tab Saved View · . batch-local/context · Enter open · a Quick Prompt · n Scratch",
    "Scratch: Esc Board",
    "Terminal drawer: t open · T close",
    "Terminal focus: keys go to PTY · F6 return to app · Ctrl+] alternate · Shift+PgUp/PgDn scrollback.",
    "Authority: Codex/Git/Forge stay canonical; codex-tui stores operator state only.",
];

const HELP_LINES_ZH: &[&str] = &[
    "全局: ? 帮助 · Ctrl+K 命令面板 · / 搜索 · . 操作 · Esc 返回",
    "任务中心: j/k 移动 · Enter 打开 · Space 待处理 · l 仅本机 · g 仅仓库 · h 最近/全部历史 · p 固定 · e 别名 · x 已处理",
    "会话: a 编辑消息 · y/n/c 审批 · i 回答 · Ctrl+C 中断 · r 评审",
    "评审: j/k 文件 · w 单词级 diff · e 编辑器 · o 外部打开 · . Forge 操作 · PageUp/PageDown · Esc",
    "工作区: Git + Forge · . 操作/启动预设 · r 评审 · m worktree · Esc",
    "受管 Worktree: n 创建 · a 接管 · d 移除 · x 删除分支 · y 确认",
    "看板: h/l 阶段 · j/k 项目 · Space 待处理 · s 稍后提醒 · = 绑定 · 1–9 快捷槽",
    "看板: Tab 已保存视图 · . 本地批量/上下文 · Enter 打开 · a Quick Prompt · n Scratch",
    "Scratch: 仅本地详情 · Esc 返回看板",
    "终端抽屉: t 打开 · T 关闭（Scratch 除外）",
    "终端聚焦: 按键发送给 PTY · F6 返回应用 · Ctrl+] 备用 · Shift+PgUp/PgDn 回滚",
    "权限边界: Codex/Git/Forge 保持权威来源；codex-tui 只保存操作员状态。",
];

fn render_help(frame: &mut Frame<'_>, language: UiLanguage) {
    let area = centered_rect(70, 70, frame.area());
    let help_lines = if language.is_simplified_chinese() {
        HELP_LINES_ZH
    } else {
        HELP_LINES
    };
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(
            help_lines
                .iter()
                .map(|line| Line::from(*line))
                .collect::<Vec<_>>(),
        )
        .block(Block::bordered().title(tr_language(language, " Help ", " 帮助 ")))
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn interactive_request_lines(
    request: &InteractiveRequest,
    language: UiLanguage,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(tr_language(language, "NEEDS YOU", "需要你处理"))];
    match &request.kind {
        InteractiveRequestKind::CommandApproval {
            command,
            cwd,
            reason,
        } => {
            lines.push(Line::from(format!(
                "{}: {command}",
                tr_language(language, "Command approval", "命令审批")
            )));
            if !cwd.is_empty() {
                lines.push(Line::from(format!("cwd: {cwd}")));
            }
            if let Some(reason) = reason {
                lines.push(Line::from(format!(
                    "{}: {reason}",
                    tr_language(language, "reason", "原因")
                )));
            }
            lines.push(Line::from(tr_language(
                language,
                "y accept · n decline · c cancel",
                "y 接受 · n 拒绝 · c 取消",
            )));
        }
        InteractiveRequestKind::FileChangeApproval { reason } => {
            lines.push(Line::from(tr_language(
                language,
                "File change approval",
                "文件变更审批",
            )));
            if let Some(reason) = reason {
                lines.push(Line::from(format!(
                    "{}: {reason}",
                    tr_language(language, "reason", "原因")
                )));
            }
            lines.push(Line::from(tr_language(
                language,
                "y accept · n decline · c cancel",
                "y 接受 · n 拒绝 · c 取消",
            )));
        }
        InteractiveRequestKind::PermissionsApproval {
            reason,
            network_requested,
            filesystem_requested,
        } => {
            if language.is_simplified_chinese() {
                lines.push(Line::from(format!(
                    "权限请求: network={} filesystem={}",
                    network_requested, filesystem_requested
                )));
            } else {
                lines.push(Line::from(format!(
                    "Permission request: network={} filesystem={}",
                    network_requested, filesystem_requested
                )));
            }
            if let Some(reason) = reason {
                lines.push(Line::from(format!(
                    "{}: {reason}",
                    tr_language(language, "reason", "原因")
                )));
            }
            lines.push(Line::from(tr_language(
                language,
                "y grant for this turn · n/c decline",
                "y 本轮授权 · n/c 拒绝",
            )));
        }
        InteractiveRequestKind::UserInput { questions } => {
            if language.is_simplified_chinese() {
                lines.push(Line::from(format!(
                    "需要用户输入: {} 个问题",
                    questions.len()
                )));
            } else {
                lines.push(Line::from(format!(
                    "User input requested: {} question(s)",
                    questions.len()
                )));
            }
            if let Some(question) = questions.first() {
                lines.push(Line::from(format!(
                    "{}: {}",
                    question.header, question.question
                )));
                if !question.options.is_empty() {
                    lines.push(Line::from(format!(
                        "{}: {}",
                        tr_language(language, "options", "选项"),
                        question.options.join(", ")
                    )));
                }
            }
            lines.push(Line::from(tr_language(language, "i answer", "i 回答")));
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
    use crate::keymap::HELP_BINDINGS;
    use pretty_assertions::assert_eq;
    use ratatui::{Terminal, backend::TestBackend};
    use std::collections::BTreeSet;

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
    fn english_help_tokens_exactly_match_the_executable_key_contract() {
        let surfaces = HELP_BINDINGS
            .iter()
            .map(|binding| binding.surface)
            .collect::<BTreeSet<_>>();
        for surface in surfaces.iter() {
            let expected = HELP_BINDINGS
                .iter()
                .filter(|binding| binding.surface == *surface)
                .map(|binding| binding.token)
                .collect::<BTreeSet<_>>();
            let prefix = format!("{surface}:");
            let advertised = HELP_LINES
                .iter()
                .filter(|line| line.starts_with(&prefix))
                .flat_map(|line| {
                    line.split_once(':')
                        .map(|(_, tail)| tail)
                        .unwrap_or_default()
                        .split('·')
                })
                .filter_map(|segment| segment.split_whitespace().next())
                .collect::<BTreeSet<_>>();
            assert_eq!(
                advertised, expected,
                "Help/keymap contract drift on {surface}"
            );
        }
    }

    #[test]
    fn registry_history_label_distinguishes_partial_and_complete_history() {
        assert_eq!(
            registry_history_label(UiLanguage::English, true, false, false),
            "RECENT · HYDRATING"
        );
        assert_eq!(
            registry_history_label(UiLanguage::English, true, true, false),
            "SEARCH PARTIAL · HYDRATING"
        );
        assert_eq!(
            registry_history_label(UiLanguage::English, false, true, false),
            "SEARCH ALL"
        );
        assert_eq!(
            registry_history_label(UiLanguage::SimplifiedChinese, true, false, true),
            "全部历史（加载中）"
        );
        assert_eq!(
            registry_history_label(UiLanguage::SimplifiedChinese, false, false, true),
            "全部历史"
        );
        assert_eq!(
            registry_history_label(UiLanguage::English, false, false, false),
            "RECENT"
        );
    }

    #[test]
    fn production_ui_never_performs_authoritative_cwd_filesystem_classification() {
        let source = include_str!("ui.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production ui source");
        assert!(
            !production.contains(".cwd_locality("),
            "rendering must not call the filesystem-backed AppState cwd locality API"
        );
        assert!(
            !production.contains("classify_cwd("),
            "rendering must not call the filesystem-backed cwd classifier"
        );
    }

    #[test]
    fn simplified_chinese_ui_localizes_daily_chrome_and_help() {
        let backend = TestBackend::new(160, 28);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.language = UiLanguage::SimplifiedChinese;
        app.backend_status.connected = true;
        app.backend_status.registry_complete = true;
        app.backend_status.source = "codex-app-server".into();
        app.backend_status.platform = None;
        app.threads[0].metadata.model = None;

        terminal.draw(|frame| render(frame, &app)).expect("draw");
        let mut snapshot = String::new();
        let buffer = terminal.backend().buffer();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                snapshot.push_str(buffer[(x, y)].symbol());
            }
            snapshot.push('\n');
        }
        for glyph in [
            '任', '务', '中', '心', '已', '选', '择', '搜', '索', '未', '知',
        ] {
            assert!(
                snapshot.contains(glyph),
                "missing localized glyph {glyph:?}"
            );
        }

        app.show_help = true;
        terminal
            .draw(|frame| render(frame, &app))
            .expect("draw help");
        let mut help_snapshot = String::new();
        let buffer = terminal.backend().buffer();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                help_snapshot.push_str(buffer[(x, y)].symbol());
            }
            help_snapshot.push('\n');
        }
        for glyph in ['帮', '助', '权', '限', '边', '界'] {
            assert!(
                help_snapshot.contains(glyph),
                "missing localized help glyph {glyph:?}"
            );
        }
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

    #[test]
    fn ordinary_registry_search_renders_unprobed_native_cwds_without_locality_cache() {
        let backend = TestBackend::new(140, 14);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = AppState::new(FakeBackend::scaled(150).snapshot().threads);
        app.backend_status = FakeBackend::seeded().snapshot().status;
        let cwd = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned();
        for thread in &mut app.threads {
            thread.metadata.cwd.clone_from(&cwd);
        }
        app.filter = "Synthetic".into();

        assert_eq!(app.cwd_locality_for_display(&cwd), None);
        terminal.draw(|frame| render(frame, &app)).expect("draw");

        let buffer = terminal.backend().buffer();
        let mut snapshot = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                snapshot.push_str(buffer[(x, y)].symbol());
            }
            snapshot.push('\n');
        }

        assert!(snapshot.contains("150 unprobed"));
        assert_eq!(app.cwd_locality_for_display(&cwd), None);
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
        crate::app::reduce(&mut app, crate::app::Action::ToggleHostLocalFilter);

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
        crate::app::reduce(&mut app, crate::app::Action::ToggleRepoBackedFilter);

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
        app.backend_status.registry_complete = true;
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
        assert!(status.contains("selected cwd: unprobed · terminal unchecked · git not-probed"));

        let effects = crate::app::reduce(&mut app, crate::app::Action::RefreshGitProjections);
        assert!(!effects.is_empty());
        let status = registry_scope_status(&app, 160);
        assert!(status.contains("selected cwd: local · terminal ready · git probing"));
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
    fn simplified_chinese_status_helpers_cover_terminal_forge_and_receipts() {
        assert_eq!(
            terminal_process_state_label(
                &TerminalProcessState::Running,
                UiLanguage::SimplifiedChinese
            ),
            "运行中"
        );
        assert_eq!(
            forge_freshness_label(ForgeFreshness::Stale, UiLanguage::SimplifiedChinese),
            "过期"
        );
        assert_eq!(
            operation_state_label(OperationState::Succeeded, UiLanguage::SimplifiedChinese),
            "已成功"
        );
        assert_eq!(
            scratch_state_label(ScratchState::Ready, UiLanguage::SimplifiedChinese),
            "就绪"
        );
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
    fn board_viewport_keeps_large_selection_visible() {
        let viewport = board_viewport(10_000, 7_321, 22);
        assert_eq!(viewport.row_capacity, 20);
        assert!(7_321 >= viewport.start);
        assert!(7_321 < viewport.start + viewport.row_capacity);
        assert!(viewport.start <= 10_000 - viewport.row_capacity);
    }

    #[test]
    fn board_viewport_stays_at_zero_when_content_fits() {
        assert_eq!(
            board_viewport(5, 4, 12),
            BoardViewport {
                start: 0,
                row_capacity: 10,
            }
        );
    }

    #[test]
    fn board_viewport_handles_zero_height_without_underflow() {
        assert_eq!(
            board_viewport(100, 73, 1),
            BoardViewport {
                start: 0,
                row_capacity: 0,
            }
        );
    }

    #[test]
    fn help_hints_match_locked_keyboard_commands() {
        use crate::app::ViewKind;
        use crate::{command::Command, keymap::command_for_key};
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
                "Terminal drawer: t open",
                KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
                ViewKind::Workspace,
                Command::TerminalDrawer,
            ),
            (
                "T close",
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
