use crate::app::{AppState, ContextChoice, HELP_ENTRY_COUNT, InputMode, View};
use crate::conversation::InteractiveRequest;
use crate::domain::{AttentionReason, CwdLocality, RuntimeStatus, display_cwd};
use crate::forge::ForgeFreshness;
use crate::goal::GoalStatus;
use crate::i18n::{UiLanguage, pick};
use crate::operation::OperationState;
use crate::planning::{PlanningAttention, SavedView, SavedViewLayout, ScratchState, WorkflowStage};
use crate::text::{fit_display, sanitize_inline, truncate_display};

mod board;
mod overlays;
use board::render_board;
#[cfg(test)]
use board::{BoardViewport, board_viewport, planning_card_field};
mod registry;
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
#[cfg(test)]
use registry::{RegistryViewport, registry_history_label, registry_viewport};
use registry::{registry_footer, render_thread_list};
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
    let primary_area = primary_view_rect(frame.area(), app.terminal_drawer_open);
    // Brief in-memory source events yield to active editors and focused PTYs.
    let recent = app.recent_notification.as_ref().filter(|recent| {
        recent.visible_until > std::time::Instant::now()
            && primary_area.height >= 8
            && primary_area.width >= 40
            && app.recent_alert_controls_available()
    });
    let content_area = if recent.is_some() {
        Rect {
            y: primary_area.y.saturating_add(1),
            height: primary_area.height.saturating_sub(1),
            ..primary_area
        }
    } else {
        primary_area
    };
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
    if let Some(recent) = recent {
        frame.render_widget(
            Paragraph::new(truncate_display(
                &recent.display(app.language),
                usize::from(primary_area.width),
            )),
            Rect {
                height: 1,
                ..primary_area
            },
        );
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
        render_help(frame, app);
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
    let display_input = sanitize_inline(&app.input_buffer);

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
            display_input,
            tr(app, "Enter keep · Esc cancel", "Enter 保留 · Esc 取消")
        )),
        InputMode::TranscriptSearch => Line::from(format!(
            "Ctrl+F> {}  · {}",
            display_input,
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
            display_input,
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
            display_input,
            tr(app, "Enter create · Esc cancel", "Enter 创建 · Esc 取消")
        )),
        InputMode::Snooze => Line::from(format!(
            "{}> {}  · 15m / 1h / 1d · {}",
            tr(app, "snooze", "稍后提醒"),
            display_input,
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
                    sanitize_inline(&app.backend_status.source),
                    tr(app, "offline/degraded", "离线/降级"),
                    truncate_display(error, 80)
                ))
            } else if !app.backend_status.connected {
                Line::from(format!(
                    "{} · {}",
                    sanitize_inline(&app.backend_status.source),
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
    let footer = Paragraph::new(vec![registry_footer(app, area.width), status_line]);
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
        let thread_label = sanitize_inline(&thread.id.0);
        let model_label = sanitize_inline(
            thread
                .metadata
                .model
                .as_deref()
                .unwrap_or_else(|| tr(app, "unknown", "未知")),
        );
        let mut lines = if app.language.is_simplified_chinese() {
            vec![
                Line::from(format!("会话: {thread_label}")),
                Line::from(format!("工作区: {}", sanitize_inline(&thread.workspace))),
                Line::from(format!(
                    "运行状态: {} · 待处理: {attention}",
                    runtime_status_label(&thread.runtime, app.language)
                )),
                Line::from(format!("模型: {model_label}")),
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
                Line::from(format!("Thread: {thread_label}")),
                Line::from(format!("Workspace: {}", sanitize_inline(&thread.workspace))),
                Line::from(format!(
                    "Runtime: {} · attention: {attention}",
                    runtime_status_label(&thread.runtime, app.language)
                )),
                Line::from(format!("Model: {model_label}")),
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
                    let diagnostic = sanitize_inline(
                        context
                            .error
                            .as_deref()
                            .unwrap_or_else(|| tr(app, "unknown error", "未知错误")),
                    );
                    lines.push(Line::from(format!(
                        "{} · {diagnostic}",
                        tr(app, "Git: degraded", "Git: 已降级"),
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
                    lines.push(Line::from(format!("Git: {}", sanitize_inline(branch))));
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
        sanitize_inline(&identity.host),
        sanitize_inline(&identity.path_with_namespace),
        forge_freshness_label(
            observation.freshness_at(crate::operation::now_unix_ms()),
            app.language,
        )
    ))];

    if observation.incomplete_overview_reason().is_some() {
        lines.push(Line::from(tr(
            app,
            "Forge overview is partial or bounded · verify in provider",
            "Forge 概览可能不完整或达到分页上限 · 请向平台核实",
        )));
    }

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
                sanitize_inline(&change.state),
                truncate_display(&change.title, 58)
            )));
        } else if app.language.is_simplified_chinese() {
            lines.push(Line::from(format!(
                "CR: 最近结果未发现分支 {}（不代表不存在）",
                truncate_display(branch, 42)
            )));
        } else {
            lines.push(Line::from(format!(
                "CR: no match in recent results for {} (not proof of absence)",
                truncate_display(branch, 42)
            )));
        }
        if let Some(pipeline) = observation.pipeline_for_branch(branch) {
            lines.push(Line::from(format!(
                "Pipeline: #{} · {}",
                pipeline.id,
                sanitize_inline(&pipeline.status)
            )));
        } else {
            lines.push(Line::from(tr(
                app,
                "Pipeline: not found in recent results (not exhaustive)",
                "Pipeline: 最近结果未命中（不代表不存在）",
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
            Line::from(format!("Scratch: {}", sanitize_inline(&scratch.id))),
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
        Line::from(format!(
            "{}: {}",
            tr(app, "Thread", "会话"),
            sanitize_inline(&thread.id.0)
        )),
        Line::from(format!("Cwd: {}", sanitize_inline(&thread.metadata.cwd))),
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
                sanitize_inline(context.error.as_deref().unwrap_or_else(|| tr(
                    app,
                    "unknown error",
                    "未知错误"
                )))
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
                    sanitize_inline(&repo.primary_root)
                )));
                lines.push(Line::from(format!(
                    "Git common dir: {}",
                    sanitize_inline(&repo.git_common_dir)
                )));
            }
            if let Some(worktree) = &context.worktree {
                lines.push(Line::from(format!(
                    "Worktree: {}",
                    sanitize_inline(&worktree.canonical_path)
                )));
            }
            lines.push(Line::from(format!(
                "{}: {}",
                tr(app, "Branch", "分支"),
                sanitize_inline(
                    context
                        .branch
                        .as_deref()
                        .or(context.head.as_deref())
                        .unwrap_or_else(|| tr(app, "<unknown>", "<未知>"))
                )
            )));
            lines.push(Line::from(if app.language.is_simplified_chinese() {
                format!(
                    "上游: {} · 领先={} 落后={}",
                    sanitize_inline(
                        context
                            .upstream
                            .as_deref()
                            .unwrap_or_else(|| tr(app, "<none>", "<无>"))
                    ),
                    context.ahead,
                    context.behind
                )
            } else {
                format!(
                    "Upstream: {} · ahead={} behind={}",
                    sanitize_inline(
                        context
                            .upstream
                            .as_deref()
                            .unwrap_or_else(|| tr(app, "<none>", "<无>"))
                    ),
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
        repo.map(|repo| sanitize_inline(&repo.primary_root))
            .unwrap_or_else(|| tr(app, "<unavailable>", "<不可用>").into())
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
        lines.push(Line::from(format!("Cwd: {}", sanitize_inline(&plan.cwd))));
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
            "{}: {}",
            tr(app, "Exact operation", "精确操作"),
            sanitize_inline(&command)
        )));
        lines.push(Line::from(format!(
            "{}: {}",
            tr(app, "Expected", "预期结果"),
            sanitize_inline(&plan.expected_side_effect)
        )));
        if let Some(path) = &plan.target_worktree {
            lines.push(Line::from(format!(
                "{}: {}",
                tr(app, "Target worktree", "目标 worktree"),
                sanitize_inline(path)
            )));
        }
        if let Some(branch) = &plan.target_branch {
            lines.push(Line::from(format!(
                "{}: {}",
                tr(app, "Target branch", "目标分支"),
                sanitize_inline(branch)
            )));
        }
        lines.push(Line::from(tr(app, "Preconditions:", "前置条件:")));
        lines.extend(plan.preconditions.iter().map(|precondition| {
            Line::from(format!(
                "  {} = {}",
                sanitize_inline(&precondition.key),
                sanitize_inline(&precondition.expected)
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
    frame.render_widget(Paragraph::new(sanitize_inline(&footer)), outer[1]);
}

const HELP_LINES: &[&str] = &[
    "Global: ! alert · = bind · 1–9 jump · ? help · Ctrl+K palette · / search · Ctrl+F transcript · . actions · Esc back",
    "Registry: j/k · Enter · Space attention · l local-only · g repo-only · h recent/all-history · p pin · e alias · x ack",
    "Thread: a composer · q Thread Queue · d request details · y/n/c approval · i answer · Ctrl+C interrupt · r review",
    "Thread: PageUp/PageDown scroll · Home loaded history start · End follow latest",
    "Request details: ↑/↓ or PageUp/PageDown scroll · Home/End · Esc back without responding",
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
    "全局: ! 最近提醒 · = 绑定 · 1–9 直达 · ? 帮助 · Ctrl+K 命令面板 · / 搜索 · . 操作 · Esc 返回",
    "任务中心: j/k 移动 · Enter 打开 · Space 待处理 · l 仅本机 · g 仅仓库 · h 最近/全部历史 · p 固定 · e 别名 · x 已处理",
    "会话: a 编辑消息 · d 请求详情 · y/n/c 审批 · i 回答 · Ctrl+C 中断 · r 评审",
    "会话: PageUp/PageDown 滚动 · Home 已加载历史开头 · End 跟随最新",
    "请求详情: ↑/↓ 或 PageUp/PageDown 滚动 · Home/End · Esc 返回，不提交响应",
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

fn render_help(frame: &mut Frame<'_>, app: &AppState) {
    let area = centered_rect(90, 84, frame.area());
    let help_lines = if app.language.is_simplified_chinese() {
        HELP_LINES_ZH
    } else {
        HELP_LINES
    };
    debug_assert_eq!(help_lines.len(), HELP_ENTRY_COUNT);
    if area.width < 4 || area.height < 4 {
        return;
    }
    // Advance by complete help entries instead of estimating physical wrap
    // offsets; End always reaches the final entry on narrow SSH terminals.
    let top = app.help_top.min(help_lines.len().saturating_sub(1));
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width - 2,
        height: area.height - 3,
    };
    let footer = Rect {
        y: area.y + area.height - 2,
        height: 1,
        ..inner
    };
    let title = if app.language.is_simplified_chinese() {
        format!(" 帮助 · 从 {}/{} 项查看 ", top + 1, help_lines.len())
    } else {
        format!(" Help · from {}/{} ", top + 1, help_lines.len())
    };
    let controls = tr_language(
        app.language,
        "↑↓ scroll · PgUp/PgDn jump · Home/End · Esc close",
        "↑↓ 滚动 · PgUp/PgDn 翻页 · Home/End · Esc 关闭",
    );
    frame.render_widget(Clear, area);
    frame.render_widget(Block::bordered().title(title), area);
    frame.render_widget(
        Paragraph::new(
            help_lines
                .iter()
                .skip(top)
                .map(|line| Line::from(*line))
                .collect::<Vec<_>>(),
        )
        .wrap(Wrap { trim: true }),
        inner,
    );
    frame.render_widget(
        Paragraph::new(truncate_display(controls, footer.width.into())),
        footer,
    );
}

fn interactive_request_lines(
    request: &InteractiveRequest,
    language: UiLanguage,
) -> Vec<Line<'static>> {
    crate::app::request_details::request_lines(request, language, false)
        .into_iter()
        .map(Line::from)
        .collect()
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
mod tests;
