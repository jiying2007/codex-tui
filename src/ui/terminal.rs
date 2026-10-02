use crate::{
    app::AppState,
    i18n::{UiLanguage, pick},
    pty::TerminalSize,
    terminal_drawer::TerminalProcessState,
    text::truncate_display,
};
use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Block, Clear, Paragraph, Wrap},
};

fn tr<'a>(app: &AppState, english: &'a str, simplified_chinese: &'a str) -> &'a str {
    pick(app.language, english, simplified_chinese)
}

pub(super) fn terminal_process_state_label(state: &TerminalProcessState, language: UiLanguage) -> String {
    match (state, language) {
        (TerminalProcessState::Starting, UiLanguage::SimplifiedChinese) => "启动中".into(),
        (TerminalProcessState::Running, UiLanguage::SimplifiedChinese) => "运行中".into(),
        (TerminalProcessState::Exited { success, code }, UiLanguage::SimplifiedChinese) => {
            format!(
                "已退出 · success={} · code={}",
                success,
                code.map_or_else(|| "?".into(), |value| value.to_string())
            )
        }
        (TerminalProcessState::Error(error), UiLanguage::SimplifiedChinese) => {
            format!("错误 · {error}")
        }
        _ => state.label(),
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

pub(super) fn render_terminal_drawer(frame: &mut Frame<'_>, app: &AppState) {
    let area = terminal_drawer_rect(frame.area());
    frame.render_widget(Clear, area);

    let snapshot = app.terminal_snapshot.as_ref();
    let status = snapshot
        .map(|snapshot| terminal_process_state_label(&snapshot.state, app.language))
        .unwrap_or_else(|| tr(app, "starting", "启动中").into());
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
        tr(
            app,
            "unfocused · t focus · T close",
            "未聚焦 · t 聚焦 · T 关闭",
        )
    };
    let title = format!(
        " {} · {focus} · {} · {} ",
        tr(app, "Terminal Drawer", "终端抽屉"),
        truncate_display(cwd, 44),
        truncate_display(&status, 36)
    );

    let lines = snapshot.map_or_else(
        || {
            vec![Line::from(tr(
                app,
                "Starting platform default terminal…",
                "正在启动平台默认终端…",
            ))]
        },
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

