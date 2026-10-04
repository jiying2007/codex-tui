use crate::{
    app::{AppState, View},
    backend::{CodexBackend, FakeBackend},
    conversation::{ConversationItem, ConversationItemKind, ConversationPage, ConversationState},
    domain::{ThreadId, ThreadUiState},
    planning::{SourceRef, WorkCardOverlay, WorkCardProjection, WorkflowStage},
    ui,
};
use anyhow::{Context, Result};
use ratatui::{Terminal, backend::TestBackend};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    hint::black_box,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const RENDER_PERFORMANCE_SCHEMA: &str = "codex-tui/render-performance/v1";
pub const BOARD_RENDER_FIXTURE: &str = "board-render-10k";
pub const THREAD_RENDER_FIXTURE: &str = "thread-render-10k";
pub const RENDER_ROWS: usize = 10_000;
pub const RENDER_MIN_ITERATIONS: usize = 200;
pub const RENDER_VIEWPORT_WIDTH: u16 = 160;
pub const RENDER_VIEWPORT_HEIGHT: u16 = 40;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderFixtureReport {
    pub fixture: &'static str,
    pub rows: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPerformanceReport {
    pub schema: &'static str,
    pub source_sha: &'static str,
    pub viewport_width: u16,
    pub viewport_height: u16,
    pub warmup_iterations: usize,
    pub iterations: usize,
    pub fixtures: Vec<RenderFixtureReport>,
    pub source: String,
    pub observed_at: String,
    pub sample_qualified: bool,
}

pub fn run(warmup_iterations: usize, iterations: usize, source: String) -> RenderPerformanceReport {
    let board = board_fixture();
    let thread = thread_fixture();

    let board_report = measure_fixture(
        BOARD_RENDER_FIXTURE,
        RENDER_ROWS,
        &board,
        warmup_iterations,
        iterations,
    );
    let thread_report = measure_fixture(
        THREAD_RENDER_FIXTURE,
        RENDER_ROWS,
        &thread,
        warmup_iterations,
        iterations,
    );

    let sample_qualified = iterations >= RENDER_MIN_ITERATIONS
        && [board_report.clone(), thread_report.clone()]
            .into_iter()
            .all(|fixture| {
                fixture.p50_ms.is_finite()
                    && fixture.p50_ms >= 0.0
                    && fixture.p50_ms <= fixture.p95_ms
                    && fixture.p95_ms <= fixture.p99_ms
                    && fixture.p99_ms <= fixture.max_ms
            });

    RenderPerformanceReport {
        schema: RENDER_PERFORMANCE_SCHEMA,
        source_sha: crate::compat::source_sha(),
        viewport_width: RENDER_VIEWPORT_WIDTH,
        viewport_height: RENDER_VIEWPORT_HEIGHT,
        warmup_iterations,
        iterations,
        fixtures: vec![board_report, thread_report],
        source,
        observed_at: observed_at(),
        sample_qualified,
    }
}

pub fn run_cli(args: &[String]) -> Result<i32> {
    let mut warmup_iterations = 20usize;
    let mut iterations = RENDER_MIN_ITERATIONS;
    let mut source = "local-retained-runner".to_string();
    let mut json = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--warmup" => {
                index += 1;
                warmup_iterations = args
                    .get(index)
                    .context("--warmup requires a value")?
                    .parse()
                    .context("--warmup must be a positive integer")?;
            }
            "--iterations" => {
                index += 1;
                iterations = args
                    .get(index)
                    .context("--iterations requires a value")?
                    .parse()
                    .context("--iterations must be a positive integer")?;
            }
            "--source" => {
                index += 1;
                source = args
                    .get(index)
                    .context("--source requires a value")?
                    .trim()
                    .to_string();
            }
            "--json" => json = true,
            other => anyhow::bail!("unknown render benchmark option: {other}"),
        }
        index += 1;
    }

    anyhow::ensure!(warmup_iterations > 0, "--warmup must be > 0");
    anyhow::ensure!(iterations > 0, "--iterations must be > 0");
    anyhow::ensure!(!source.is_empty(), "--source must not be empty");

    let report = run(warmup_iterations, iterations, source);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "{} {}x{} · warmup={} · iterations={} · exact-sha={}",
            report.schema,
            report.viewport_width,
            report.viewport_height,
            report.warmup_iterations,
            report.iterations,
            report.source_sha
        );
        for fixture in &report.fixtures {
            println!(
                "{} rows={} p50={:.3}ms p95={:.3}ms p99={:.3}ms max={:.3}ms",
                fixture.fixture,
                fixture.rows,
                fixture.p50_ms,
                fixture.p95_ms,
                fixture.p99_ms,
                fixture.max_ms
            );
        }
    }
    Ok(0)
}

fn measure_fixture(
    fixture: &'static str,
    rows: usize,
    app: &AppState,
    warmup_iterations: usize,
    iterations: usize,
) -> RenderFixtureReport {
    let backend = TestBackend::new(RENDER_VIEWPORT_WIDTH, RENDER_VIEWPORT_HEIGHT);
    let mut terminal = Terminal::new(backend).expect("create retained render terminal");

    for _ in 0..warmup_iterations {
        terminal
            .draw(|frame| ui::render(frame, app))
            .expect("warmup render");
        black_box(terminal.backend().buffer());
    }

    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        terminal
            .draw(|frame| ui::render(frame, app))
            .expect("retained render");
        black_box(terminal.backend().buffer());
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);

    RenderFixtureReport {
        fixture,
        rows,
        p50_ms: percentile(&samples, 0.50),
        p95_ms: percentile(&samples, 0.95),
        p99_ms: percentile(&samples, 0.99),
        max_ms: samples.last().copied().unwrap_or(0.0),
    }
}

pub(crate) fn board_fixture() -> AppState {
    let mut app = AppState::new(vec![]);
    app.view = View::Board;
    app.work_cards = (0..RENDER_ROWS)
        .map(|index| {
            let thread_id = ThreadId::new(format!("render-board-{index:05}"));
            WorkCardProjection {
                local_id: format!("thread:{}", thread_id.0),
                anchor: SourceRef::codex_thread(&thread_id),
                title: format!("Board render card {index:05}"),
                workspace: Some(format!("workspace-{:03}", index % 64)),
                branch: Some(format!("feature/{:04}", index % 256)),
                forge_provider: None,
                change_request_state: None,
                change_request_draft: false,
                stage: WorkflowStage::ALL[index % WorkflowStage::ALL.len()],
                stage_reason: "retained render fixture".into(),
                attention: BTreeSet::new(),
                snoozed: false,
                overlay: WorkCardOverlay {
                    priority: Some((index % 20) as i32),
                    ..WorkCardOverlay::default()
                },
                links: vec![],
                goal: None,
                provenance: vec![],
            }
        })
        .collect();
    app.board_stage_index = 0;
    app.board_selected = RENDER_ROWS / WorkflowStage::ALL.len() - 1;
    app
}

pub(crate) fn thread_fixture() -> AppState {
    let threads = FakeBackend::scaled(1).snapshot().threads;
    let thread_id = threads[0].id.clone();
    let mut app = AppState::new(threads);
    app.view = View::Thread(thread_id.clone());

    let items = (0..RENDER_ROWS)
        .map(|index| ConversationItem {
            turn_id: format!("turn-{:05}", index / 2),
            item_id: format!("item-{index:05}"),
            kind: if index % 2 == 0 {
                ConversationItemKind::User
            } else {
                ConversationItemKind::Assistant
            },
            text: format!(
                "Rendered conversation item {index:05}: bounded viewport evidence for long-history interaction and revision-cache behavior."
            ),
            status: None,
        })
        .collect();

    let mut state = ConversationState::loading(thread_id.clone());
    state.replace_page(ConversationPage {
        thread_id: thread_id.clone(),
        title: Some("10k retained thread render fixture".into()),
        turns: vec![],
        items,
        next_turn_cursor: None,
        next_item_cursor: None,
    });
    app.conversations.insert(thread_id.0.clone(), state);
    app.thread_ui.insert(
        thread_id.0.clone(),
        ThreadUiState {
            scroll: (RENDER_ROWS - 64) as u16,
            follow: false,
            ..ThreadUiState::default()
        },
    );
    app
}

pub(crate) fn percentile(samples: &[f64], percentile: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let rank = (percentile * samples.len() as f64).ceil() as usize;
    samples[rank.saturating_sub(1).min(samples.len() - 1)]
}

fn observed_at() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("unix-ms:{millis}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_render_report_covers_board_and_thread_without_thresholds() {
        let report = run(1, 3, "test".into());
        assert_eq!(report.schema, RENDER_PERFORMANCE_SCHEMA);
        assert_eq!(report.source_sha, crate::compat::source_sha());
        assert_eq!(report.viewport_width, 160);
        assert_eq!(report.viewport_height, 40);
        assert_eq!(report.fixtures.len(), 2);
        assert_eq!(report.fixtures[0].fixture, BOARD_RENDER_FIXTURE);
        assert_eq!(report.fixtures[1].fixture, THREAD_RENDER_FIXTURE);
        assert!(report.fixtures.iter().all(|fixture| fixture.rows == 10_000));
        assert!(!report.sample_qualified);
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let samples = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&samples, 0.50), 3.0);
        assert_eq!(percentile(&samples, 0.95), 5.0);
        assert_eq!(percentile(&samples, 0.99), 5.0);
    }
}
