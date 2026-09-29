use crate::app::{AppState, InputMode, View};
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

    let status_line = match app.input_mode {
        InputMode::Search => {
            Line::from(format!("/{}  · Enter keep · Esc cancel", app.input_buffer))
        }
        InputMode::Alias => Line::from(format!(
            "alias> {}  · Enter save · Esc cancel",
            app.input_buffer
        )),
        InputMode::Normal => {
            if let Some(error) = &app.backend_status.error {
                Line::from(format!(
                    "{} · offline/degraded · {}",
                    app.backend_status.source,
                    truncate(error, 80)
                ))
            } else {
                Line::from(format!(
                    "{} · {}",
                    app.backend_status.source,
                    if app.backend_status.connected {
                        "connected"
                    } else {
                        "offline"
                    }
                ))
            }
        }
    };
    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::raw("j/k move  "),
            Span::raw("Space attention  "),
            Span::raw("/ search  "),
            Span::raw("p pin  "),
            Span::raw("e alias  "),
            Span::raw("x ack  "),
            Span::raw("? help"),
        ]),
        status_line,
    ]);
    frame.render_widget(footer, chunks[1]);
}

fn thread_list(app: &AppState) -> Paragraph<'static> {
    let visible = app.visible_indices();
    let attention_count = visible
        .iter()
        .filter(|index| app.thread_needs_attention(**index))
        .count();
    let mut lines = Vec::with_capacity(visible.len() + 1);
    let summary = if app.filter.is_empty() {
        format!(
            "{} threads · {} need attention",
            app.threads.len(),
            attention_count
        )
    } else {
        format!(
            "{}/{} threads · {} need attention · filter: {}",
            visible.len(),
            app.threads.len(),
            attention_count,
            app.filter
        )
    };
    lines.push(Line::from(summary));

    for index in visible {
        let thread = &app.threads[index];
        let selected = index == app.selected;
        let prefix = if selected { ">" } else { " " };
        let pin = if thread.pinned { "*" } else { " " };
        let attention = if app.thread_needs_attention(index) {
            thread
                .attention
                .iter()
                .map(|reason| reason.label())
                .collect::<Vec<_>>()
                .join(",")
        } else if thread.attention.is_empty() {
            "-".into()
        } else {
            "ack".into()
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
        .block(
            Block::bordered().title(format!(" Mission Control · {} ", app.backend_status.source)),
        )
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
            Line::from(format!(
                "Model: {}",
                thread.metadata.model.as_deref().unwrap_or("unknown")
            )),
            Line::from(format!("Cwd: {}", thread.metadata.cwd)),
            Line::from(format!("Source: {}", thread.metadata.source)),
            Line::from(format!(
                "Workspace basis: {}",
                thread.metadata.workspace_basis
            )),
            Line::from(format!(
                "Loaded: {}",
                thread
                    .metadata
                    .loaded
                    .map_or("unknown".into(), |value| value.to_string())
            )),
            Line::from(format!("Pinned: {}", thread.pinned)),
            Line::from(format!(
                "Local attention ack: {}",
                app.acknowledged_attention.contains(&thread.id.0)
            )),
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
        Paragraph::new(format!("{title}\n{thread_id}")).block(Block::bordered().title(" Thread ")),
        chunks[0],
    );

    let ui = app.thread_ui.get(thread_id).cloned().unwrap_or_default();
    let conversation_lines = match app.conversations.get(thread_id) {
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
            .scroll((ui.scroll, 0)),
        chunks[1],
    );

    let composer = Paragraph::new(format!(
        "draft: {}\nscroll={} follow={}",
        if ui.draft.is_empty() {
            "<empty>"
        } else {
            &ui.draft
        },
        ui.scroll,
        ui.follow
    ))
    .block(Block::bordered().title(" Local thread UI state "));
    frame.render_widget(composer, chunks[2]);
    frame.render_widget(
        Paragraph::new("Esc back · PageUp/PageDown history · a composer (M2b) · Ctrl+C interrupt"),
        chunks[3],
    );
}

fn render_help(frame: &mut Frame<'_>) {
    let area = centered_rect(70, 70, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Global: ? help · Ctrl+K palette · / search · Esc back"),
            Line::from(
                "Registry: j/k · Enter · Space attention · / search · p pin · e alias · x ack",
            ),
            Line::from("Thread: PageUp/PageDown · r review · w workspace · g goal"),
            Line::from(
                "Authority: Codex/Git/Forge stay canonical; codex-tui stores operator state only.",
            ),
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
    let mut out = value
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>();
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
