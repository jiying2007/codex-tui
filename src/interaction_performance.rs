//! Changed-state CPU diagnostics through production reducer and TestBackend.
//! These exclude transport/terminal/network time and do not certify an end-to-end SLO.
use crate::{
    app::{Action, AppState, planning_worker, reduce},
    backend::{CodexBackend, FakeBackend},
    conversation::ConversationPage,
    planning::SavedViewLayout,
    render_performance::{board_fixture, percentile, thread_fixture},
    ui,
};
use anyhow::{Context, Result};
use ratatui::{Terminal, backend::TestBackend};
use serde::Serialize;
use std::time::Instant;
mod cli;
mod fixtures;
pub use cli::run_cli;
use fixtures::{HeadlessFixture, list_fixture};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Distribution {
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
}
impl Distribution {
    fn of(mut samples: Vec<f64>) -> Self {
        samples.sort_by(f64::total_cmp);
        Self {
            p50_ms: percentile(&samples, 0.50),
            p95_ms: percentile(&samples, 0.95),
            p99_ms: percentile(&samples, 0.99),
            max_ms: samples.last().copied().unwrap_or(0.0),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    #[serde(rename = "fixture")]
    name: &'static str,
    rows: usize,
    #[serde(flatten)]
    timing: Distribution,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    schema: &'static str,
    source_sha: &'static str,
    source: String,
    rows: usize,
    warmup_iterations: usize,
    iterations: usize,
    viewport_width: u16,
    viewport_height: u16,
    fixtures: Vec<Fixture>,
    sample_qualified: bool,
    authority: &'static str,
    model_inference_calls: usize,
    human_time_savings: Option<f64>,
    model_token_savings: Option<f64>,
}

pub fn run(warmup: usize, iterations: usize, source: String) -> Result<Report> {
    anyhow::ensure!(
        warmup > 0 && iterations > 0,
        "positive warmup and samples required"
    );
    let mut registry = AppState::new(FakeBackend::scaled(10_000).snapshot().threads);
    let mut board = board_fixture();
    let mut thread = thread_fixture();
    let mut list = list_fixture(SavedViewLayout::List);
    let mut review_queue = list_fixture(SavedViewLayout::ReviewQueue);
    let headless = HeadlessFixture::new(10_000);
    let mut rebase_50k = AppState::new(FakeBackend::scaled(50_000).snapshot().threads);
    let mut terminal = Terminal::new(TestBackend::new(160, 40))?;
    let mut samples: [Vec<f64>; fixtures::NAMES.len()] =
        std::array::from_fn(|_| Vec::with_capacity(iterations));
    for index in 0..warmup + iterations {
        let started = Instant::now();
        reduce(&mut registry, Action::MoveSelection(1));
        terminal.draw(|frame| ui::render(frame, &registry))?;
        let registry_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        reduce(&mut board, Action::MovePlanningSelection(1));
        terminal.draw(|frame| ui::render(frame, &board))?;
        let board_ms = started.elapsed().as_secs_f64() * 1000.0;
        let id = thread.threads[0].id.0.clone();
        let conversation = thread
            .conversations
            .get_mut(&id)
            .context("thread fixture")?;
        // Preparing an incoming page is outside the reducer/render measurement.
        let mut page = ConversationPage {
            thread_id: conversation.thread_id.clone(),
            title: conversation.title.clone(),
            turns: vec![],
            items: conversation.items.clone(),
            next_turn_cursor: None,
            next_item_cursor: None,
        };
        page.items.last_mut().context("thread fixture item")?.text =
            format!("Changed message {index}");
        let started = Instant::now();
        conversation.replace_page(page);
        terminal.draw(|frame| ui::render(frame, &thread))?;
        let thread_ms = started.elapsed().as_secs_f64() * 1000.0;
        let planning = planning_worker::measure_once(&mut registry, index as u64);
        let started = Instant::now();
        reduce(&mut list, Action::MovePlanningSelection(1));
        terminal.draw(|frame| ui::render(frame, &list))?;
        let list_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        reduce(&mut review_queue, Action::MovePlanningSelection(1));
        terminal.draw(|frame| ui::render(frame, &review_queue))?;
        let review_queue_ms = started.elapsed().as_secs_f64() * 1000.0;
        let headless_ms = headless.measure();
        let rebase_10k_ms = planning_worker::measure_rebase_once(&mut registry, index as u64);
        let rebase_50k_ms = planning_worker::measure_rebase_once(&mut rebase_50k, index as u64);
        if index >= warmup {
            for (values, value) in samples.iter_mut().zip([
                registry_ms,
                board_ms,
                thread_ms,
                planning[0],
                planning[1],
                planning[2],
                list_ms,
                review_queue_ms,
                headless_ms,
                rebase_10k_ms,
                rebase_50k_ms,
            ]) {
                values.push(value);
            }
        }
    }
    Ok(Report {
        schema: "codex-tui/interaction-performance/v1",
        source_sha: crate::compat::source_sha(),
        source,
        rows: 10_000,
        warmup_iterations: warmup,
        iterations,
        viewport_width: 160,
        viewport_height: 40,
        fixtures: fixtures::NAMES
            .into_iter()
            .zip(samples)
            .map(|(name, values)| Fixture {
                rows: 10_000 * if name.ends_with("50k") { 5 } else { 1 },
                name,
                timing: Distribution::of(values),
            })
            .collect(),
        sample_qualified: warmup >= 20 && iterations >= 200,
        authority: "synthetic-cpu-path-diagnostic-not-terminal-slo",
        model_inference_calls: 0,
        human_time_savings: None,
        model_token_savings: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_state_diagnostics_do_not_claim_real_terminal_or_cost_savings() {
        let report = run(1, 2, "test".into()).unwrap();
        assert_eq!(report.fixtures.len(), 11);
        assert_eq!(report.fixtures.last().unwrap().rows, 50_000);
        assert!(!report.sample_qualified);
        assert_eq!(report.model_inference_calls, 0);
        assert!(report.human_time_savings.is_none() && report.model_token_savings.is_none());
        assert!(report.fixtures.iter().all(|f| f.timing.p95_ms.is_finite()));
    }
}
