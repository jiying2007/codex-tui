use crate::app::{AppState, InputMode, View};
use crate::conversation::{InteractiveRequest, InteractiveRequestKind};
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
        InputMode::Composer => Line::from("composer active in Thread view"),
        InputMode::UserInput => Line::from("user-input answer active in Thread view"),
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
            Span::raw("! shared-worktree  "),
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
        let text = format!(
            "{prefix}{pin}{collision} {:7} {:18} {:10} {}",
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
    let mut lines = if let Some(thread) = app.selected_thread() {
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
                "Pending interactive: {}",
                app.pending_requests
                    .iter()
                    .filter(|request| request.thread_id == thread.id)
                    .count()
            )),
        ]
    } else {
        vec![Line::from("No thread selected")]
    };

    if let Some(thread) = app.selected_thread() {
        lines.push(Line::from(""));
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
                        truncate(&worktree.canonical_path, 42)
                    )));
                }
                if let Some(repo) = &context.repo {
                    lines.push(Line::from(format!(
                        "Repo: {}",
                        truncate(&repo.primary_root, 42)
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

    Paragraph::new(lines)
        .block(Block::bordered().title(" Context "))
        .wrap(Wrap { trim: false })
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
        let mut request_lines = interactive_request_lines(request);
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

    let (composer_title, composer_text) = if app.input_mode == InputMode::UserInput {
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
            "a composer · y accept · n decline · c cancel · i answer · Ctrl+C interrupt",
        ),
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
            Line::from(
                "Thread: a composer · y/n/c approval · i answer · Ctrl+C interrupt · PageUp/PageDown",
            ),
            Line::from(
                "Authority: Codex/Git/Forge stay canonical; codex-tui stores operator state only.",
            ),
        ])
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
