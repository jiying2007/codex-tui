use crate::{
    app::{Action, AppState, reduce},
    backend::{CodexBackend, FakeBackend},
};
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    hint::black_box,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const SCALE_EVIDENCE_SCHEMA: &str = "codex-tui/scale-evidence/v2";
pub const DEFAULT_ROWS: usize = 10_000;
pub const DEFAULT_WARMUP_ITERATIONS: usize = 5;
pub const DEFAULT_ITERATIONS: usize = 50;
pub const MAX_ROWS: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingSummary {
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScaleEvidenceReport {
    pub schema: &'static str,
    pub rows: usize,
    pub warmup_iterations: usize,
    pub iterations: usize,
    pub registry_construct_ms: f64,
    pub planning_reconcile: TimingSummary,
    pub recent_projection: TimingSummary,
    pub all_history_projection: TimingSummary,
    pub search_projection: TimingSummary,
    pub host_local_projection: TimingSummary,
    pub source: String,
    pub observed_at: String,
}

pub fn run(
    rows: usize,
    warmup_iterations: usize,
    iterations: usize,
    source: String,
) -> Result<ScaleEvidenceReport> {
    anyhow::ensure!(rows > 0, "--rows must be > 0");
    anyhow::ensure!(
        rows <= MAX_ROWS,
        "--rows must be <= {MAX_ROWS} for the retained evidence harness"
    );
    anyhow::ensure!(warmup_iterations > 0, "--warmup must be > 0");
    anyhow::ensure!(iterations > 0, "--iterations must be > 0");
    anyhow::ensure!(!source.trim().is_empty(), "--source must not be empty");

    let cwd = std::env::current_dir()
        .context("resolve current directory for host-local scale evidence")?
        .to_string_lossy()
        .into_owned();

    let construct_started = Instant::now();
    let mut threads = FakeBackend::scaled(rows).snapshot().threads;
    for thread in &mut threads {
        thread.metadata.cwd.clone_from(&cwd);
    }
    let mut app = AppState::new(threads);
    let registry_construct_ms = construct_started.elapsed().as_secs_f64() * 1000.0;

    let planning_reconcile =
        sample_planning_reconcile(&mut app, warmup_iterations, iterations);

    let recent_projection = sample_projection(&app, warmup_iterations, iterations);

    reduce(&mut app, Action::ToggleAllHistory);
    let all_history_projection = sample_projection(&app, warmup_iterations, iterations);

    app.filter = "repo-031 synthetic 099".into();
    let search_projection = sample_projection(&app, warmup_iterations, iterations);

    app.filter.clear();
    reduce(&mut app, Action::ToggleHostLocalFilter);
    let host_local_projection = sample_projection(&app, warmup_iterations, iterations);

    Ok(ScaleEvidenceReport {
        schema: SCALE_EVIDENCE_SCHEMA,
        rows,
        warmup_iterations,
        iterations,
        registry_construct_ms,
        planning_reconcile,
        recent_projection,
        all_history_projection,
        search_projection,
        host_local_projection,
        source,
        observed_at: observed_at(),
    })
}

pub fn run_cli(args: &[String]) -> Result<i32> {
    let mut rows = DEFAULT_ROWS;
    let mut warmup_iterations = DEFAULT_WARMUP_ITERATIONS;
    let mut iterations = DEFAULT_ITERATIONS;
    let mut source = "local-scale-evidence".to_string();
    let mut json = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--rows" => {
                index += 1;
                rows = args
                    .get(index)
                    .context("--rows requires a value")?
                    .parse()
                    .context("--rows must be a positive integer")?;
            }
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
            other => anyhow::bail!("unknown scale evidence option: {other}"),
        }
        index += 1;
    }

    let report = run(rows, warmup_iterations, iterations, source)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("schema: {}", report.schema);
        println!("rows: {}", report.rows);
        println!("warmup-iterations: {}", report.warmup_iterations);
        println!("iterations: {}", report.iterations);
        println!("registry-construct-ms: {:.3}", report.registry_construct_ms);
        print_timing("planning-reconcile", &report.planning_reconcile);
        print_timing("recent-projection", &report.recent_projection);
        print_timing("all-history-projection", &report.all_history_projection);
        print_timing("search-projection", &report.search_projection);
        print_timing("host-local-projection", &report.host_local_projection);
        println!("source: {}", report.source);
        println!("observed-at: {}", report.observed_at);
    }

    Ok(0)
}

fn sample_planning_reconcile(
    app: &mut AppState,
    warmup_iterations: usize,
    iterations: usize,
) -> TimingSummary {
    for _ in 0..warmup_iterations {
        black_box(reduce(app, Action::ReconcilePlanning { now_unix_ms: 1 }));
    }

    sample_timings(iterations, || {
        black_box(reduce(app, Action::ReconcilePlanning { now_unix_ms: 1 }));
    })
}

fn sample_projection(app: &AppState, warmup_iterations: usize, iterations: usize) -> TimingSummary {
    for _ in 0..warmup_iterations {
        black_box(app.visible_indices_with_match_count());
    }

    sample_timings(iterations, || {
        black_box(app.visible_indices_with_match_count());
    })
}

fn sample_timings(mut iterations: usize, mut operation: impl FnMut()) -> TimingSummary {
    let mut samples = Vec::with_capacity(iterations);
    while iterations > 0 {
        let started = Instant::now();
        operation();
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        iterations -= 1;
    }
    samples.sort_by(f64::total_cmp);

    TimingSummary {
        p50_ms: percentile(&samples, 0.50),
        p95_ms: percentile(&samples, 0.95),
        p99_ms: percentile(&samples, 0.99),
        max_ms: samples.last().copied().unwrap_or(0.0),
    }
}

fn percentile(samples: &[f64], percentile: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let rank = (percentile * samples.len() as f64).ceil() as usize;
    samples[rank.saturating_sub(1).min(samples.len() - 1)]
}

fn print_timing(label: &str, timing: &TimingSummary) {
    println!("{label}-p50-ms: {:.3}", timing.p50_ms);
    println!("{label}-p95-ms: {:.3}", timing.p95_ms);
    println!("{label}-p99-ms: {:.3}", timing.p99_ms);
    println!("{label}-max-ms: {:.3}", timing.max_ms);
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
    fn scale_evidence_covers_registry_projection_shapes() {
        let report = run(64, 1, 3, "test".into()).expect("scale evidence");
        assert_eq!(report.schema, SCALE_EVIDENCE_SCHEMA);
        assert_eq!(report.rows, 64);
        assert_eq!(report.iterations, 3);
        for timing in [
            &report.planning_reconcile,
            &report.recent_projection,
            &report.all_history_projection,
            &report.search_projection,
            &report.host_local_projection,
        ] {
            assert!(timing.p50_ms >= 0.0);
            assert!(timing.p95_ms >= timing.p50_ms);
            assert!(timing.p99_ms >= timing.p95_ms);
            assert!(timing.max_ms >= timing.p99_ms);
        }
    }

    #[test]
    fn scale_evidence_rejects_unbounded_fixture_sizes() {
        let error = run(MAX_ROWS + 1, 1, 1, "test".into()).expect_err("oversized fixture");
        assert!(error.to_string().contains("--rows must be <="));
    }
}
