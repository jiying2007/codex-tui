use crate::app::{AppState, InputMode, View};
use crate::conversation::{InteractiveRequest, InteractiveRequestKind};
use crate::domain::ThreadSummary;
use crate::git::presentation_diff_lines;
use crate::planning::{SavedViewLayout, WorkflowStage, apply_saved_view, saved_view_group_key};
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
        View::Review(id) => render_review(frame, app, id.0.as_str()),
        View::Workspace(id) => render_workspace(frame, app, id.0.as_str()),
        View::ManagedWorktrees(id) => render_managed_worktrees(frame, app, id.0.as_str()),
        View::Board => render_board(frame, app),
        View::Scratch(id) => render_scratch(frame, app, id),
    }
    if app.show_help {
        render_help(frame);
    }
    if app.context_open {
        render_context_actions(frame, app);
    }
    if matches!(
        app.input_mode,
        InputMode::Note
            | InputMode::Snooze
            | InputMode::SavedViewName
            | InputMode::ForgeMergeRequestTitle
            | InputMode::ForgeComment
    ) {
        render_local_input_overlay(frame, app);
    }
    if app.pending_forge_operation.is_some() {
        render_forge_mutation_confirmation(frame, app);
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
        InputMode::ScratchTitle => Line::from(format!(
            "new scratch> {}  · Enter create · Esc cancel",
            app.input_buffer
        )),
        InputMode::Snooze => Line::from(format!(
            "snooze> {}  · examples 15m / 1h / 1d · Enter apply · Esc cancel",
            app.input_buffer
        )),
        InputMode::Note => Line::from(format!(
            "note> {}  · Enter save · Esc cancel",
            truncate(&app.input_buffer, 60)
        )),
        InputMode::SavedViewName => Line::from(format!(
            "view name> {}  · Enter save · Esc cancel",
            truncate(&app.input_buffer, 60)
        )),
        InputMode::GoalObjective => Line::from("Goal objective editor active in Thread view"),
        InputMode::ForgeMergeRequestTitle | InputMode::ForgeComment => {
            Line::from("forge mutation input active in Review/Workspace")
        }
        InputMode::WorktreeCreateBranch
        | InputMode::WorktreeCreatePath
        | InputMode::WorktreeCreateStartPoint
        | InputMode::WorktreeDeleteBranch => Line::from("managed-worktree input active"),
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
            Span::raw("s snooze  "),
            Span::raw("= bind / 1–9 jump  "),
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
            Line::from(format!(
                "Planning note: {}",
                app.work_card_for_thread(&thread.id)
                    .and_then(|card| card.overlay.note.as_deref())
                    .unwrap_or("<none>")
            )),
            Line::from(format!(
                "Planning stage: {}",
                app.work_card_for_thread(&thread.id)
                    .map(|card| card.stage.label())
                    .unwrap_or("<unprojected>")
            )),
            Line::from(format!(
                "Stage reason: {}",
                app.work_card_for_thread(&thread.id)
                    .map(|card| card.stage_reason.as_str())
                    .unwrap_or("<unprojected>")
            )),
            Line::from(format!("Goal: {}", goal_summary(app, &thread.id.0))),
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

    if let Some(thread) = app.selected_thread() {
        lines.push(Line::from(""));
        lines.extend(forge_context_lines(app, &thread.id));
    }

    Paragraph::new(lines)
        .block(Block::bordered().title(" Context "))
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
        format!(
            " · changes requested {}",
            review.changes_requested_by_count
        )
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

fn forge_context_lines(app: &AppState, thread_id: &crate::domain::ThreadId) -> Vec<Line<'static>> {
    let Some(observation) = app.forge_observation(thread_id) else {
        return vec![Line::from("Forge: not probed")];
    };

    if observation.observed_at_unix_ms == 0 {
        return vec![Line::from("Forge: probing…")];
    }

    let Some(identity) = &observation.identity else {
        return vec![Line::from(format!(
            "Forge: unavailable · {}",
            truncate(
                observation
                    .error
                    .as_deref()
                    .unwrap_or("identity unresolved"),
                80
            )
        ))];
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
                truncate(&change.title, 58)
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
        lines.push(Line::from(format!("Mutation: {}", truncate(notice, 90))));
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
            truncate(&goal.objective, 42),
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

fn render_thread(frame: &mut Frame<'_>, app: &AppState, thread_id: &str) {
    let area = frame.area();
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

fn render_board(frame: &mut Frame<'_>, app: &AppState) {
    let area = frame.area();
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
                            truncate(&card.title, 20),
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
            truncate(&app.input_buffer, 80)
        )
    } else if app.input_mode == InputMode::SavedViewName {
        format!(
            "view name> {} · Enter save · Esc cancel",
            truncate(&app.input_buffer, 80)
        )
    } else if app.hot_slot_bind_pending {
        "bind hot slot: press 1–9 · Esc cancels other input only".into()
    } else if let Some(error) = &app.planning_store_error {
        format!("LOCAL STORE DEGRADED · {}", truncate(error, 80))
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
        "{} {:7} {:10} {:12} {:8} {}",
        if selected { ">" } else { " " },
        card.stage.label(),
        truncate(&attention, 10),
        truncate(goal, 12),
        source,
        card.title
    );
    let style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    Line::from(Span::styled(text, style))
}

fn render_scratch(frame: &mut Frame<'_>, app: &AppState, scratch_id: &str) {
    let area = frame.area();
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
            Line::from(format!("Title: {}", scratch.title)),
            Line::from(format!(
                "State: {:?} · priority={}",
                scratch.state,
                scratch
                    .priority
                    .map_or_else(|| "none".into(), |value| value.to_string())
            )),
            Line::from(format!(
                "Workspace: {}",
                scratch.workspace.as_deref().unwrap_or("<none>")
            )),
            Line::from(format!(
                "Note: {}",
                scratch.note.as_deref().unwrap_or("<empty>")
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

fn render_workspace(frame: &mut Frame<'_>, app: &AppState, thread_id: &str) {
    let area = frame.area();
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
                Line::from(format!("  {:2} {}", change.status_label(), change.path))
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
    lines.extend(forge_context_lines(app, &thread.id));

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Workspace · Git + Forge "))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new("r review · m managed worktrees · . forge/context actions · Esc back"),
        chunks[1],
    );
}

fn render_managed_worktrees(frame: &mut Frame<'_>, app: &AppState, thread_id: &str) {
    let area = frame.area();
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
                "{prefix} {:8} {:18} {}",
                ownership,
                record.branch.as_deref().unwrap_or("<detached>"),
                record.canonical_path
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
                truncate(verification, 90)
            )));
        }
        if let Some(failure) = &receipt.failure {
            lines.push(Line::from(format!("Failure: {}", truncate(failure, 90))));
        }
    }

    if let Some(notice) = &app.mutation_notice {
        lines.push(Line::from(""));
        lines.push(Line::from(format!("Notice: {}", truncate(notice, 100))));
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

fn render_review(frame: &mut Frame<'_>, app: &AppState, thread_id: &str) {
    let area = frame.area();
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
            "j/k file · PageUp/PageDown diff · w word-diff · e editor · . forge/context actions · Esc back",
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
        .unwrap_or(12)
        .clamp(6, 14);
    let area = centered_fixed(58, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Context Actions "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_local_input_overlay(frame: &mut Frame<'_>, app: &AppState) {
    let (title, hint) = match app.input_mode {
        InputMode::Note => (" Local note ", "Enter save · Esc cancel"),
        InputMode::Snooze => (" Snooze ", "15m / 1h / 1d · Enter apply · Esc cancel"),
        InputMode::SavedViewName => (" Save current view ", "Enter save · Esc cancel"),
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
        lines.push(Line::from(format!("Title: {}", truncate(title, 88))));
    }
    if let Some(bytes) = plan.payload_bytes {
        lines.push(Line::from(format!(
            "Payload: {bytes} bytes · body intentionally not persisted"
        )));
    }
    lines.push(Line::from(format!(
        "Expected: {}",
        truncate(&plan.expected_side_effect, 100)
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

fn render_help(frame: &mut Frame<'_>) {
    let area = centered_rect(70, 70, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Global: ? help · Ctrl+K palette · / search · . context · Esc back"),
            Line::from(
                "Registry: j/k · Enter · Space attention · / search · p pin · e alias · x ack",
            ),
            Line::from(
                "Thread: a composer · y/n/c approval · i answer · Ctrl+C interrupt · r review",
            ),
            Line::from(
                "Review: j/k file · w word-diff · e editor · . Forge actions · PageUp/PageDown · Esc",
            ),
            Line::from(
                "Workspace: Git + Forge · . explicit Forge actions · r review · m worktrees · Esc",
            ),
            Line::from(
                "Managed Worktrees: n create · a adopt · d remove · x delete branch · y confirm",
            ),
            Line::from(
                "Board: h/l stage · j/k item · Space attention · s snooze · = bind · 1–9 hot slot",
            ),
            Line::from("Board: Tab Saved View · Enter open · a Quick Prompt · n Scratch"),
            Line::from("Scratch: local-only detail · Esc Board"),
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
