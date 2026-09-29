use crate::app::{AppState, View};
use crate::domain::ThreadSummary;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
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

pub fn render(frame: &mut Frame<'_>, app: &AppState) {
    match &app.view {
        View::Registry => render_registry(frame, app),
        View::Thread(id) => render_thread(frame, app, id.0.as_str()),
    }
    if app.show_help {
        render_help(frame);
    }
}

fn render_registry(frame: &mut Frame<'_>, app: &AppState) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(area);

    match layout_mode(area.width) {
        LayoutMode::Compact | LayoutMode::Standard => {
            frame.render_widget(thread_list(app), chunks[0]);
        }
        LayoutMode::Wide => {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
                .split(chunks[0]);
            frame.render_widget(thread_list(app), columns[0]);
            frame.render_widget(detail_panel(app), columns[1]);
        }
    }

    let footer = Paragraph::new(Line::from(vec![
        Span::raw("j/k move  "),
        Span::raw("Enter open  "),
        Span::raw("Space next attention  "),
        Span::raw("a quick prompt  "),
        Span::raw("Ctrl+K palette  "),
        Span::raw("? help"),
    ]));
    frame.render_widget(footer, chunks[1]);
}

fn thread_list(app: &AppState) -> Paragraph<'static> {
    let mut lines = Vec::with_capacity(app.threads.len() + 1);
    lines.push(Line::from(format!(
        "{} threads · {} need attention",
        app.threads.len(),
        app.threads.iter().filter(|thread| thread.needs_attention()).count()
    )));

    for (index, thread) in app.threads.iter().enumerate() {
        let selected = index == app.selected;
        let prefix = if selected { ">" } else { " " };
        let pin = if thread.pinned { "*" } else { " " };
        let attention = if thread.needs_attention() {
            thread
                .attention
                .iter()
                .map(|reason| reason.label())
                .collect::<Vec<_>>()
                .join(",")
        } else {
            "-".into()
        };
        let text = format!(
            "{prefix}{pin} {:7} {:18} {:10} {}",
            thread.runtime.label(),
            truncate(&thread.workspace, 18),
            attention,
            thread.display_title()
        );
        let style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(text, style)));
    }

    Paragraph::new(lines)
        .block(Block::bordered().title(" Mission Control "))
        .wrap(Wrap { trim: false })
}

fn detail_panel(app: &AppState) -> Paragraph<'static> {
    let lines = if let Some(thread) = app.selected_thread() {
        vec![
            Line::from(format!("Thread: {}", thread.id)),
            Line::from(format!("Workspace: {}", thread.workspace)),
            Line::from(format!("Runtime: {}", thread.runtime.label())),
            Line::from(format!(
                "Attention: {}",
                if thread.attention.is_empty() {
                    "none".into()
                } else {
                    thread
                        .attention
                        .iter()
                        .map(|reason| reason.label())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            )),
            Line::from("Source: FakeBackend (M0)"),
        ]
    } else {
        vec![Line::from("No thread selected")]
    };
    Paragraph::new(lines).block(Block::bordered().title(" Context "))
}

fn render_thread(frame: &mut Frame<'_>, app: &AppState, thread_id: &str) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
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
        Paragraph::new(format!("{title}\n{thread_id}"))
            .block(Block::bordered().title(" Thread ")),
        chunks[0],
    );

    frame.render_widget(
        Paragraph::new([
            Line::from("M0 uses a fake backend; canonical transcript persistence is intentionally absent."),
            Line::from("M1 will replace this body with paginated Codex App Server data."),
        ])
        .block(Block::bordered().title(" Conversation "))
        .wrap(Wrap { trim: false }),
        chunks[1],
    );

    let ui = app.thread_ui.get(thread_id).cloned().unwrap_or_default();
    let composer = Paragraph::new(format!(
        "draft: {}\nscroll={} follow={}",
        if ui.draft.is_empty() { "<empty>" } else { &ui.draft },
        ui.scroll,
        ui.follow
    ))
    .block(Block::bordered().title(" Local thread UI state "));
    frame.render_widget(composer, chunks[2]);
    frame.render_widget(
        Paragraph::new("Esc back · r review · w workspace · g goal"),
        chunks[3],
    );
}

fn render_help(frame: &mut Frame<'_>) {
    let area = centered_rect(70, 70, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new([
            Line::from("Global: ? help · Ctrl+K palette · / search · Esc back"),
            Line::from("Registry: j/k · Enter · Space attention · a quick prompt"),
            Line::from("Thread: PageUp/PageDown · r review · w workspace · g goal"),
            Line::from("Authority: Codex/Git/Forge stay canonical; codex-tui stores operator state only."),
        ])
        .block(Block::bordered().title(" Help "))
        .wrap(Wrap { trim: true }),
        area,
    );
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

fn truncate(value: &str, max: usize) -> String {
    let count = value.chars().count();
    if count <= max {
        return value.to_string();
    }
    let mut out = value.chars().take(max.saturating_sub(1)).collect::<String>();
    out.push('…');
    out
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
                assert!(snapshot.contains("Context"), "width={width}");
            }
        }
    }
}
