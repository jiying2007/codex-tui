//! Changed-state CPU diagnostics through production reducer and TestBackend.
//! These exclude transport/terminal/network time and do not certify an end-to-end SLO.
use crate::{
    app::{Action, AppState, planning_worker, reduce},
    backend::{CodexBackend, FakeBackend},
    conversation::ConversationPage,
    render_performance::{board_fixture, percentile, thread_fixture},
    ui,
};
use anyhow::{Context, Result};
use ratatui::{Terminal, backend::TestBackend};
use serde::Serialize;
use std::time::Instant;

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
    let mut terminal = Terminal::new(TestBackend::new(160, 40))?;
    let names = [
        "registry-input-and-render-10k",
        "board-navigation-and-render-10k",
        "thread-revision-and-render-10k",
        "planning-ui-snapshot-10k",
        "planning-worker-compute-10k",
        "planning-ui-commit-10k",
    ];
    let mut samples: [Vec<f64>; 6] = std::array::from_fn(|_| Vec::with_capacity(iterations));
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
        if index >= warmup {
            for (values, value) in samples.iter_mut().zip([
                registry_ms,
                board_ms,
                thread_ms,
                planning[0],
                planning[1],
                planning[2],
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
        fixtures: names
            .into_iter()
            .zip(samples)
            .map(|(name, values)| Fixture {
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

pub fn run_cli(args: &[String]) -> Result<i32> {
    let (mut warmup, mut iterations) = (20, 200);
    let mut source = "local-diagnostic".to_string();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "--warmup" | "--iterations" | "--source" => {
                index += 1;
                let value = args.get(index).context("benchmark option requires value")?;
                match arg.as_str() {
                    "--warmup" => warmup = value.parse()?,
                    "--iterations" => iterations = value.parse()?,
                    _ => source = value.clone(),
                }
            }
            "--json" => {}
            other => anyhow::bail!("unknown interaction benchmark option: {other}"),
        }
        index += 1;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&run(warmup, iterations, source)?)?
    );
    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_state_diagnostics_do_not_claim_real_terminal_or_cost_savings() {
        let report = run(1, 2, "test".into()).unwrap();
        assert_eq!(report.fixtures.len(), 6);
        assert!(!report.sample_qualified);
        assert_eq!(report.model_inference_calls, 0);
        assert!(report.human_time_savings.is_none() && report.model_token_savings.is_none());
        assert!(report.fixtures.iter().all(|f| f.timing.p95_ms.is_finite()));
    }
}
