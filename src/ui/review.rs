use super::{forge_freshness_label, tr};
use crate::app::AppState;
use crate::git::presentation_diff_lines;
use crate::syntax_highlight::cached_review_diff;
use crate::text::{sanitize_inline, truncate_display};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

fn safe_review_file_row(prefix: &str, status: &str, path: &str) -> String {
    format!("{prefix} {status:2} {}", sanitize_inline(path))
}

fn review_evidence_lines(app: &AppState, thread_id: &str) -> Vec<Line<'static>> {
    let Some(git_review) = app.git_reviews.get(thread_id) else {
        return vec![];
    };

    let mut lines = vec![Line::from(format!(
        "Git: {}={} · {}={}",
        tr(app, "changed", "变更"),
        git_review.changes.len(),
        tr(app, "truncated", "截断"),
        git_review.truncated
    ))];

    let Some(observation) = app.forge_observations.get(thread_id) else {
        lines.push(Line::from(tr(
            app,
            "Forge: not observed",
            "Forge: 尚未观测",
        )));
        return lines;
    };

    if let Some(identity) = &observation.identity {
        lines.push(Line::from(format!(
            "Forge: {} · {}/{} · {}",
            identity.provider.label(),
            sanitize_inline(&identity.host),
            sanitize_inline(&identity.path_with_namespace),
            forge_freshness_label(
                observation.freshness_at(crate::operation::now_unix_ms()),
                app.language,
            )
        )));
    } else {
        lines.push(Line::from(tr(app, "Forge: unavailable", "Forge: 不可用")));
    }

    if observation.incomplete_overview_reason().is_some() {
        lines.push(Line::from(tr(
            app,
            "Forge overview partial or capped; verify directly with provider",
            "Forge 概览部分不可用或已达分页上限，请向平台核实",
        )));
    }

    let branch = app
        .git_contexts
        .get(thread_id)
        .and_then(|context| context.branch.as_deref());

    if let Some(branch) = branch {
        if let Some(change) = observation.change_request_for_branch(branch) {
            let merge_status = change.detailed_merge_status.as_deref().unwrap_or("n/a");
            lines.push(Line::from(format!(
                "CR #{}: {}{} · merge={} · {}",
                change.iid,
                if change.draft { "draft · " } else { "" },
                sanitize_inline(&change.state),
                sanitize_inline(merge_status),
                truncate_display(&change.title, 48)
            )));
        } else {
            lines.push(Line::from(format!(
                "{}: {}",
                tr(
                    app,
                    "CR: no match in observed recent results",
                    "CR: 最近结果中未找到，不代表不存在",
                ),
                sanitize_inline(branch)
            )));
        }

        if let Some(pipeline) = observation.pipeline_for_branch(branch) {
            lines.push(Line::from(format!(
                "Pipeline #{}: {}",
                pipeline.id,
                sanitize_inline(&pipeline.status)
            )));
        } else {
            lines.push(Line::from(tr(
                app,
                "Pipeline: not observed in recent results",
                "Pipeline: 最近结果中未找到",
            )));
        }
    }

    if let Some(review) = observation.review.as_ref() {
        let approvals = if review.approvals_available {
            match (review.approvals_required, review.approvals_left) {
                (Some(required), Some(left)) => format!("{left}/{required} left"),
                _ => review.approved_by_count.to_string(),
            }
        } else {
            "n/a".into()
        };
        let discussions = if review.discussions_available {
            review.unresolved_discussions.to_string()
        } else {
            "n/a".into()
        };
        lines.push(Line::from(format!(
            "{}: approvals={} · changes-requested={} · unresolved={}",
            tr(app, "Review", "评审"),
            approvals,
            review.changes_requested_by_count,
            discussions
        )));
    }

    lines
}

pub(super) fn render_review(frame: &mut Frame<'_>, app: &AppState, thread_id: &str, area: Rect) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(1)])
        .split(area);

    let Some(review) = app.git_reviews.get(thread_id) else {
        frame.render_widget(
            Paragraph::new(tr(app, "Review has not been loaded.", "评审尚未加载。"))
                .block(Block::bordered().title(tr(app, " Review ", " 评审 "))),
            outer[0],
        );
        return;
    };

    if review.observed_at_unix_ms == 0 {
        frame.render_widget(
            Paragraph::new(tr(app, "Loading Git review…", "正在加载 Git 评审…"))
                .block(Block::bordered().title(tr(app, " Review ", " 评审 "))),
            outer[0],
        );
    } else if let Some(error) = &review.error {
        frame.render_widget(
            Paragraph::new(format!(
                "{}: {}",
                tr(app, "Review unavailable", "评审不可用"),
                sanitize_inline(error)
            ))
            .block(Block::bordered().title(tr(app, " Review ", " 评审 "))),
            outer[0],
        );
    } else {
        let evidence = review_evidence_lines(app, thread_id);
        let files = review
            .changes
            .iter()
            .enumerate()
            .map(|(index, change)| {
                let selected = index == app.review_selected;
                let prefix = if selected { ">" } else { " " };
                let text = safe_review_file_row(prefix, &change.status_label(), &change.path);
                let style = if selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(text, style))
            })
            .collect::<Vec<_>>();

        let cached =
            cached_review_diff(thread_id, review.observed_at_unix_ms, app.review_word_diff);
        let word_diff_ready = app.review_word_diff && cached.is_some();
        let mut diff_lines = cached.unwrap_or_else(|| {
            presentation_diff_lines(review, false)
                .into_iter()
                .map(Line::from)
                .collect()
        });
        if app.review_word_diff && !word_diff_ready {
            diff_lines.insert(
                0,
                Line::from(tr(
                    app,
                    "Word diff not cached or over budget; showing plain diff.",
                    "单词级差异未缓存或超出预算；正在显示普通差异。",
                )),
            );
        }
        if diff_lines.is_empty() {
            diff_lines.push(Line::from(tr(
                app,
                "No staged/unstaged tracked diff. Untracked files remain listed.",
                "没有已跟踪的暂存/未暂存 diff；未跟踪文件仍会列出。",
            )));
        }

        let truncation = if review.truncated {
            tr(app, " · truncated", " · 已截断")
        } else {
            ""
        };

        if area.width >= 100 {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
                .split(outer[0]);
            let left = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(7), Constraint::Min(3)])
                .split(columns[0]);

            frame.render_widget(
                Paragraph::new(evidence)
                    .block(Block::bordered().title(tr(app, " Evidence ", " 证据 ")))
                    .wrap(Wrap { trim: false }),
                left[0],
            );
            frame.render_widget(
                Paragraph::new(files)
                    .block(Block::bordered().title(format!(
                        " {} ({}) ",
                        tr(app, "Changed files", "变更文件"),
                        review.changes.len()
                    )))
                    .wrap(Wrap { trim: false }),
                left[1],
            );
            frame.render_widget(
                Paragraph::new(diff_lines)
                    .block(Block::bordered().title(format!(
                        " Git diff · {}={}{} ",
                        tr(app, "word", "单词级"),
                        word_diff_ready,
                        truncation
                    )))
                    .wrap(Wrap { trim: false })
                    .scroll((app.review_scroll, 0)),
                columns[1],
            );
        } else {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(7),
                    Constraint::Length(6),
                    Constraint::Min(4),
                ])
                .split(outer[0]);
            frame.render_widget(
                Paragraph::new(evidence)
                    .block(Block::bordered().title(tr(app, " Evidence ", " 证据 ")))
                    .wrap(Wrap { trim: false }),
                rows[0],
            );
            frame.render_widget(
                Paragraph::new(files)
                    .block(Block::bordered().title(tr(app, " Changed files ", " 变更文件 ")))
                    .wrap(Wrap { trim: false }),
                rows[1],
            );
            frame.render_widget(
                Paragraph::new(diff_lines)
                    .block(Block::bordered().title(format!(
                        " Git diff · {}={}{} ",
                        tr(app, "word", "单词级"),
                        word_diff_ready,
                        truncation
                    )))
                    .wrap(Wrap { trim: false })
                    .scroll((app.review_scroll, 0)),
                rows[2],
            );
        }
    }

    frame.render_widget(
        Paragraph::new(tr(
            app,
            "j/k file · PageUp/PageDown diff · w word-diff · e editor · o external · . actions · Esc back",
            "j/k 文件 · PageUp/PageDown diff · w 单词级 diff · e 编辑器 · o 外部打开 · . 操作 · Esc 返回",
        )),
        outer[1],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    #[test]
    fn evidence_without_forge_still_reports_git_state() {
        let app = AppState::new(FakeBackend::seeded().snapshot().threads);
        assert!(review_evidence_lines(&app, "missing").is_empty());
    }
}

#[cfg(test)]
mod display_input_safety_tests {
    use super::safe_review_file_row;

    #[test]
    fn untrusted_git_filename_cannot_create_new_view_lines_or_terminal_controls() {
        let displayed = safe_review_file_row(">", "M.", "normal\nnext\r\t\u{001b}[31m");
        assert!(displayed.starts_with("> M. normal"));
        assert!(!displayed.contains('\n'));
        assert!(!displayed.contains('\r'));
        assert!(!displayed.contains('\t'));
        assert!(!displayed.contains('\u{001b}'));
        assert!(displayed.contains('�'));
    }
}
