from pathlib import Path
from lean_patch_support import replace, between, write

write('src/ui/mission.rs', r'''use super::{AppState, tr};

pub(super) fn footer(app: &AppState, width: u16) -> &'static str {
    if width < 100 {
        tr(app, "F1 Today F2 Projects F3 Review  Enter open  Ctrl+K / ?",
            "F1 今日 F2 项目 F3 交付  Enter 打开  Ctrl+K / ?")
    } else {
        tr(app, "F1 Today  F2 Projects  F3 Review  Enter open  Space next  o delivery  Ctrl+K actions  ? help",
            "F1 今日  F2 项目  F3 交付  Enter 打开  Space 待处理  o 交付  Ctrl+K 操作  ? 帮助")
    }
}

pub(super) fn attention_label(app: &AppState, index: usize) -> &'static str {
    let Some(signal) = app.thread_attention_signal(index) else {
        let Some(thread) = app.threads.get(index) else { return "-"; };
        if app.work_card_for_thread(&thread.id).is_some_and(|card| card.snoozed) {
            return tr(app, "snoozed", "稍后");
        }
        return if thread.attention.is_empty() { "-" } else { tr(app, "ack", "已处理") };
    };
    match signal.reason {
        "approval" => tr(app, "approve", "审批"),
        "input" => tr(app, "input", "输入"),
        "interactive" | "waiting" => tr(app, "needs you", "待处理"),
        "error" => tr(app, "error", "错误"),
        "pipeline-failed" => tr(app, "CI fail", "CI失败"),
        "change-requested" => tr(app, "changes", "需修改"),
        "conflict" => tr(app, "conflict", "冲突"),
        "goal-blocked" => tr(app, "blocked", "目标受阻"),
        "usage-limited" => tr(app, "usage", "用量受限"),
        "budget-limited" => tr(app, "budget", "预算受限"),
        "review" => tr(app, "review", "评审"),
        "unread" => tr(app, "unread", "未读"),
        _ => signal.reason,
    }
}
''')
write('src/ui/planning_viewport.rs', r'''//! Scan lightweight metadata, format only visible rows. No all-card Line allocation.
use crate::planning::{SourceKind, WorkCardProjection};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Row { Group(usize), Card(usize) }

fn group<'a>(card: &'a WorkCardProjection, field: Option<&str>) -> Option<&'a str> {
    match field {
        Some("stage") => Some(card.stage.label()),
        Some("workspace") => Some(card.workspace.as_deref().unwrap_or("No workspace")),
        Some("source") => Some(match card.anchor.kind {
            SourceKind::ScratchWork => "ScratchWork", SourceKind::CodexThread => "CodexThread",
            SourceKind::ForgeWorkItem => "ForgeWorkItem", SourceKind::Goal => "Goal",
            SourceKind::Worktree => "Worktree", SourceKind::ChangeRequest => "ChangeRequest",
        }),
        _ => None,
    }
}

pub(super) fn measure(cards: &[&WorkCardProjection], field: Option<&str>, selected: usize) -> (usize, usize) {
    let mut previous = None;
    let mut total = 0;
    let mut selected_line = 0;
    let selected = selected.min(cards.len().saturating_sub(1));
    for (index, card) in cards.iter().enumerate() {
        let key = group(card, field);
        if key.is_some() && key != previous { total += 1; }
        previous = key;
        if index == selected { selected_line = total; }
        total += 1;
    }
    (total, selected_line)
}

pub(super) fn visible(cards: &[&WorkCardProjection], field: Option<&str>, start: usize, capacity: usize) -> Vec<Row> {
    if capacity == 0 { return vec![]; }
    let end = start.saturating_add(capacity);
    let mut rows = Vec::with_capacity(capacity.min(cards.len().saturating_mul(2)));
    let mut previous = None;
    let mut position = 0;
    for (index, card) in cards.iter().enumerate() {
        let key = group(card, field);
        if key.is_some() && key != previous {
            if position >= start && position < end { rows.push(Row::Group(index)); }
            position += 1;
        }
        previous = key;
        if position >= start && position < end { rows.push(Row::Card(index)); }
        position += 1;
        if position >= end { break; }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{ScratchWork, ScratchState, reconcile_scratch_card};
    fn card(id: usize, workspace: &str) -> WorkCardProjection {
        reconcile_scratch_card(&ScratchWork { id: format!("{id}"), title: format!("任务-{id}"), note: None,
            workspace: Some(workspace.into()), priority: None, state: ScratchState::Ready,
            created_at_unix_ms: 1, updated_at_unix_ms: 1 })
    }
    #[test]
    fn group_headers_and_selected_card_use_the_same_row_coordinates() {
        let cards = [card(0, "a"), card(1, "a"), card(2, "b")];
        let refs = cards.iter().collect::<Vec<_>>();
        assert_eq!(measure(&refs, Some("workspace"), 2), (5, 4));
        assert_eq!(visible(&refs, Some("workspace"), 2, 2), vec![Row::Card(1), Row::Group(2)]);
        assert_eq!(visible(&refs, Some("workspace"), 4, 1), vec![Row::Card(2)]);
        assert_eq!(measure(&refs, None, 2), (3, 2));
    }
    #[test]
    fn large_list_formats_at_most_the_requested_window_and_reaches_tail() {
        let cards = (0..10_000).map(|id| card(id, "one")).collect::<Vec<_>>();
        let refs = cards.iter().collect::<Vec<_>>();
        let (total, selected) = measure(&refs, Some("workspace"), 9_999);
        assert_eq!((total, selected), (10_001, 10_000));
        let rows = visible(&refs, Some("workspace"), selected.saturating_sub(19), 20);
        assert_eq!(rows.len(), 20);
        assert_eq!(rows.last(), Some(&Row::Card(9_999)));
        assert!(visible(&refs, None, 0, 0).is_empty());
        assert!(visible(&[], None, 0, 20).is_empty());
    }
}
''')
replace('src/ui.rs', 'mod overlays;', 'mod mission;\nmod planning_viewport;\nmod overlays;')
between('src/ui.rs', '        let pending_interactive = app\n', '        let collision =', '        let attention = mission::attention_label(app, index);\n')
replace('src/ui.rs', '''                "{prefix}{pin}{collision}{locality} {} {} {}",
                fit_display(runtime_status_label(&thread.runtime, app.language), 7),
                fit_display(&thread.workspace, 12),
                sanitize_inline(thread.display_title())''', '''                "{prefix}{pin}{collision}{locality} {} {} {} {}",
                fit_display(attention, 8),
                fit_display(runtime_status_label(&thread.runtime, app.language), 7),
                fit_display(&thread.workspace, 10),
                sanitize_inline(thread.display_title())''')
# The other branch now consumes a borrowed label, not an allocated String.
replace('src/ui.rs', '                fit_display(&attention, 10),', '                fit_display(attention, 10),')
p = Path('src/ui.rs'); s = p.read_text(); a = s.index('fn render_registry('); b = s.index('\nfn backend_platform_label', a)
part = s[a:b]; first = part.index('    let footer = Paragraph::new('); last = part.index('    frame.render_widget(footer, chunks[1]);', first)
part = part[:first] + '''    let footer = Paragraph::new(vec![
        Line::from(mission::footer(app, area.width)),
        status_line,
    ]);
''' + part[last:]
p.write_text(s[:a] + part + s[b:])
replace('src/ui.rs', 'tr(app, "Mission Control", "任务中心"),', 'tr(app, "Mission Control · Today [F1]", "任务中心 · 今日 [F1]"),')
between('src/ui.rs', '        SavedViewLayout::List | SavedViewLayout::ReviewQueue => {', '    }\n\n    let input =', r'''        SavedViewLayout::List | SavedViewLayout::ReviewQueue => {
            let cards = app.visible_planning_cards();
            let (total, selected_line) = planning_viewport::measure(&cards, view.group_by.as_deref(), app.board_selected);
            let viewport = board_viewport(total, selected_line, outer[0].height);
            let width = usize::from(outer[0].width.saturating_sub(2));
            let mut lines = planning_viewport::visible(&cards, view.group_by.as_deref(), viewport.start, viewport.row_capacity)
                .into_iter().map(|row| match row {
                    planning_viewport::Row::Group(index) => Line::from(truncate_display(
                        &format!("── {} ──", sanitize_inline(&saved_view_group_key(cards[index], view.group_by.as_deref()))), width)),
                    planning_viewport::Row::Card(index) => {
                        let mut line = planning_card_line(cards[index], index == app.board_selected, app.language, &view.visible_fields);
                        // planning_card_line has one styled span; keep its selection style.
                        for span in &mut line.spans { span.content = truncate_display(&span.content, width).into(); }
                        line
                    }
                }).collect::<Vec<_>>();
            if lines.is_empty() {
                lines.push(Line::from(tr(app, "No cards match this Saved View.", "没有符合当前已保存视图的卡片。")));
            }
            frame.render_widget(Paragraph::new(lines).block(Block::bordered().title(format!(
                " {} · {} ", sanitize_inline(saved_view_name(&view, app.language)), saved_view_layout_label(view.layout, app.language)))), outer[0]);
            render_board_scrollbar(frame, outer[0], total, viewport);
        }
''')
# Compact daily footer, while typed input and degraded-state notices retain their own space.
p = Path('src/ui.rs'); s = p.read_text(); a = s.index('fn render_board('); b = s.index('\nfn planning_card_line', a); part = s[a:b]
start = part.index('    } else if app.language.is_simplified_chinese() {', part.index('    let input ='))
end = part.index('    };\n    frame.render_widget', start)
part = part[:start] + '    } else {\n        mission::footer(app, area.width).to_string()\n' + part[end:]
p.write_text(s[:a] + part + s[b:])
# Actual terminal rendering, not just metadata priority assertions.
p = Path('tests/lean_navigation.rs')
p.write_text(p.read_text() + r'''
#[test]
fn compact_terminal_preserves_actionable_attention_without_color() {
    use codex_tui::{domain::{AttentionReason, RuntimeStatus, ThreadId, ThreadMetadata, ThreadSummary}, i18n::UiLanguage, ui};
    use ratatui::{Terminal, backend::TestBackend};
    let thread = ThreadSummary { id: ThreadId::new("one"), title: "Needs answer".into(), workspace: "robot".into(),
        runtime: RuntimeStatus::WaitingHuman, attention: vec![AttentionReason::UserInputRequired], pinned: false,
        alias: None, metadata: ThreadMetadata::default() };
    let mut app = AppState::new(vec![thread]);
    app.host_local_only = false;
    app.repo_backed_only = false;
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        app.language = language;
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
        let text = terminal.backend().buffer().content.iter().map(|cell| cell.symbol()).collect::<String>();
        assert!(text.contains(if language == UiLanguage::English { "input" } else { "输入" }), "{text}");
    }
}
''')
