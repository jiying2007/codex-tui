//! Bilingual read-only locator; no resume/approve/write action.
use super::{AppState, tr};
use crate::{app::LookupPhase, text::sanitize_inline};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::{Block, Clear, Paragraph, Wrap},
};

pub(super) fn render(frame: &mut Frame<'_>, app: &AppState) {
    let Some(panel) = &app.target_lookup else {
        return;
    };
    let frame_area = frame.area();
    if frame_area.width < 12 || frame_area.height < 8 {
        return;
    }
    let width = frame_area.width.min(100);
    let height = frame_area.height.min(23);
    let area = Rect {
        x: frame_area.x + (frame_area.width - width) / 2,
        y: frame_area.y + (frame_area.height - height) / 2,
        width,
        height,
    };
    let block = Block::bordered().title(tr(app, " Search all targets ", " 搜索全部目标 "));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .areas(inner);
    let label = match &panel.phase {
        LookupPhase::Editing => {
            tr(app, "Enter query (1–256 bytes)", "输入关键词（1–256 字节）").into()
        }
        LookupPhase::Requested => {
            tr(app, "Searching configured targets…", "正在检索已配置目标…").into()
        }
        LookupPhase::Running(pages) => format!(
            "{} {}/{}",
            tr(app, "Searching / Partial", "检索中 / 部分结果"),
            if pages.is_empty() {
                0
            } else {
                panel.selected + 1
            },
            pages.len(),
        ),
        LookupPhase::Results(pages) => format!(
            "{} {}/{}",
            tr(app, "Read-only result", "只读结果"),
            if pages.is_empty() {
                0
            } else {
                panel.selected + 1
            },
            pages.len(),
        ),
    };
    frame.render_widget(Paragraph::new(label).wrap(Wrap { trim: false }), header);
    let text = match &panel.phase {
        LookupPhase::Editing => {
            let mut text = format!("> {}", sanitize_inline(&panel.query));
            if panel.invalid {
                text.push_str(tr(app, "\nInvalid or empty query", "\n关键词为空或无效"));
            }
            text
        }
        LookupPhase::Requested => tr(
            app,
            "Read-only metadata; active target unchanged. Esc cancels.",
            "仅查询元数据，不改变活动目标。Esc 取消。",
        )
        .into(),
        LookupPhase::Running(pages) | LookupPhase::Results(pages) => pages
            .get(panel.selected)
            .cloned()
            .unwrap_or_else(|| tr(app, "Waiting for first target…", "等待首个目标…").into()),
    };
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .scroll((panel.scroll, 0)),
        body,
    );
    let hint = match &panel.phase {
        LookupPhase::Editing => tr(app, "Enter search · Esc close", "Enter 检索 · Esc 关闭"),
        LookupPhase::Requested => tr(app, "Esc cancel", "Esc 取消"),
        LookupPhase::Running(_) | LookupPhase::Results(_) => tr(
            app,
            "↑/↓ result · Home/End jump · PgUp/PgDn scroll · Esc close",
            "↑/↓ 结果 · Home/End 跳转 · PgUp/PgDn 滚动 · Esc 关闭",
        ),
    };
    frame.render_widget(Paragraph::new(hint).wrap(Wrap { trim: false }), footer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{Action, TargetLookupAction, reduce},
        i18n::UiLanguage,
    };
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn locator_renders_both_languages_at_sixty_columns() {
        let mut app = AppState::new(vec![]);
        reduce(&mut app, Action::TargetLookup(TargetLookupAction::Open));
        app.target_lookup.as_mut().unwrap().phase = LookupPhase::Results(vec![
            "Target: second\nThreadId: shared-id\nCWD: /fixture/project".into(),
        ]);
        for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
            app.language = language;
            let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
            terminal.draw(|frame| super::render(frame, &app)).unwrap();
            let buffer = terminal.backend().buffer();
            let screen = (0..buffer.area.height)
                .map(|y| {
                    (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(screen.contains("Target: second"));
            assert!(screen.contains("ThreadId: shared-id"));
            assert!(screen.contains("CWD: /fixture/project"));
            assert!(screen.contains("Esc"));
        }
        app.language = UiLanguage::English;
        app.target_lookup.as_mut().unwrap().phase =
            LookupPhase::Running(vec!["Target: first\nThreadId: observed".into()]);
        let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
        terminal.draw(|frame| super::render(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        let screen = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(screen.contains("Searching / Partial"));
        assert!(screen.contains("ThreadId: observed"));
    }
}
