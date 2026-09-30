use crate::{
    backend::{CodexBackend, FakeBackend},
    planning::{
        ReconcileInput, SavedView, SavedViewLayout, WorkCardProjection, apply_saved_view,
        reconcile_thread_card,
    },
};
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    hint::black_box,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const PERFORMANCE_SCHEMA: &str = "codex-tui/performance/v1";
pub const PERFORMANCE_FIXTURE: &str = "resident-planning-10k";
pub const STABLE_MIN_ITERATIONS: usize = 200;
pub const STABLE_P95_MS_MAX: f64 = 50.0;
pub const STABLE_P99_MS_MAX: f64 = 100.0;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceBenchmarkReport {
    pub schema: &'static str,
    pub fixture: &'static str,
    pub rows: usize,
    pub warmup_iterations: usize,
    pub iterations: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub source: String,
    pub observed_at: String,
    pub stable_slo_pass: bool,
}

pub fn run(
    warmup_iterations: usize,
    iterations: usize,
    source: String,
) -> PerformanceBenchmarkReport {
    let cards = cards_10k();
    let view = rich_query_view();

    for _ in 0..warmup_iterations {
        black_box(apply_saved_view(&cards, &view));
    }

    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        black_box(apply_saved_view(&cards, &view));
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);

    let p50_ms = percentile(&samples, 0.50);
    let p95_ms = percentile(&samples, 0.95);
    let p99_ms = percentile(&samples, 0.99);
    let max_ms = samples.last().copied().unwrap_or(0.0);
    let stable_slo_pass = iterations >= STABLE_MIN_ITERATIONS
        && p95_ms <= STABLE_P95_MS_MAX
        && p99_ms <= STABLE_P99_MS_MAX;

    PerformanceBenchmarkReport {
        schema: PERFORMANCE_SCHEMA,
        fixture: PERFORMANCE_FIXTURE,
        rows: cards.len(),
        warmup_iterations,
        iterations,
        p50_ms,
        p95_ms,
        p99_ms,
        max_ms,
        source,
        observed_at: observed_at(),
        stable_slo_pass,
    }
}

pub fn run_cli(args: &[String]) -> Result<i32> {
    let mut warmup_iterations = 20usize;
    let mut iterations = STABLE_MIN_ITERATIONS;
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
            other => anyhow::bail!("unknown release benchmark option: {other}"),
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
        println!("schema: {}", report.schema);
        println!("fixture: {}", report.fixture);
        println!("rows: {}", report.rows);
        println!("warmup-iterations: {}", report.warmup_iterations);
        println!("iterations: {}", report.iterations);
        println!("p50-ms: {:.3}", report.p50_ms);
        println!("p95-ms: {:.3}", report.p95_ms);
        println!("p99-ms: {:.3}", report.p99_ms);
        println!("max-ms: {:.3}", report.max_ms);
        println!("source: {}", report.source);
        println!("observed-at: {}", report.observed_at);
        println!("stable-slo-pass: {}", report.stable_slo_pass);
    }
    Ok(if report.stable_slo_pass { 0 } else { 5 })
}

fn cards_10k() -> Vec<WorkCardProjection> {
    let snapshot = FakeBackend::scaled(10_000).snapshot();
    snapshot
        .threads
        .iter()
        .map(|thread| {
            reconcile_thread_card(ReconcileInput {
                thread,
                git: None,
                local: None,
                collision_count: 0,
                backend_observed_at_unix_ms: Some(1),
                backend_error: None,
                now_unix_ms: 1,
            })
        })
        .collect()
}

fn rich_query_view() -> SavedView {
    SavedView {
        id: "release:resident-planning-10k".into(),
        name: "Release resident planning 10k".into(),
        source_scope: "all".into(),
        filter: "status:needs-you -stage:done project:repo".into(),
        group_by: Some("workspace".into()),
        order_by: Some("priority".into()),
        layout: SavedViewLayout::List,
        visible_fields: vec![],
    }
}

fn percentile(samples: &[f64], percentile: f64) -> f64 {
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
    fn percentile_uses_nearest_rank() {
        let samples = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&samples, 0.50), 3.0);
        assert_eq!(percentile(&samples, 0.95), 5.0);
        assert_eq!(percentile(&samples, 0.99), 5.0);
    }

    #[test]
    fn benchmark_uses_exact_release_fixture() {
        let report = run(1, 3, "test".into());
        assert_eq!(report.schema, PERFORMANCE_SCHEMA);
        assert_eq!(report.fixture, PERFORMANCE_FIXTURE);
        assert_eq!(report.rows, 10_000);
        assert_eq!(report.iterations, 3);
        assert!(report.p50_ms >= 0.0);
        assert!(report.p95_ms >= report.p50_ms);
        assert!(report.p99_ms >= report.p95_ms);
        assert!(!report.stable_slo_pass);
    }
}
