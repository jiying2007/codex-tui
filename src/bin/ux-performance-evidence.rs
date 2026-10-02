use anyhow::{Context, Result};
use codex_tui::{
    app::{Action, AppState, View, reduce},
    backend::{CodexBackend, FakeBackend},
    conversation::{ConversationItem, ConversationItemKind, ConversationState},
    ui,
};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use std::{env, time::Instant};

const WIDTH: u16 = 160;
const HEIGHT: u16 = 50;

#[derive(Clone, Debug)]
struct Args {
    source_sha: String,
    source: String,
    rows: usize,
    warmup: usize,
    iterations: usize,
}

fn parse_args() -> Result<Args> {
    let mut source_sha = None;
    let mut source = None;
    let mut rows = 10_000_usize;
    let mut warmup = 20_usize;
    let mut iterations = 200_usize;
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        let value = args
            .next()
            .with_context(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--source-sha" => source_sha = Some(value),
            "--source" => source = Some(value),
            "--rows" => rows = value.parse().context("parse --rows")?,
            "--warmup" => warmup = value.parse().context("parse --warmup")?,
            "--iterations" => iterations = value.parse().context("parse --iterations")?,
            _ => anyhow::bail!("unknown argument: {arg}"),
        }
    }

    let source_sha = source_sha.context("--source-sha is required")?;
    anyhow::ensure!(
        source_sha.len() == 40 && source_sha.chars().all(|ch| ch.is_ascii_hexdigit()),
        "--source-sha must be a 40-character Git SHA"
    );
    anyhow::ensure!(rows >= 1_000, "--rows must be >= 1000");
    anyhow::ensure!(warmup >= 1, "--warmup must be >= 1");
    anyhow::ensure!(iterations >= 1, "--iterations must be >= 1");

    Ok(Args {
        source_sha,
        source: source.unwrap_or_else(|| "unknown".into()),
        rows,
        warmup,
        iterations,
    })
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn stats(mut samples: Vec<f64>) -> Value {
    samples.sort_by(f64::total_cmp);
    json!({
        "p50Ms": percentile(&samples, 0.50),
        "p95Ms": percentile(&samples, 0.95),
        "p99Ms": percentile(&samples, 0.99),
        "maxMs": samples.last().copied().unwrap_or_default(),
    })
}

fn measure_render(app: &AppState, warmup: usize, iterations: usize) -> Result<Value> {
    let backend = TestBackend::new(WIDTH, HEIGHT);
    let mut terminal = Terminal::new(backend)?;

    for _ in 0..warmup {
        terminal.draw(|frame| ui::render(frame, app))?;
    }

    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        terminal.draw(|frame| ui::render(frame, app))?;
        samples.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    Ok(stats(samples))
}

fn board_fixture(rows: usize) -> AppState {
    let mut app = AppState::new(FakeBackend::scaled(rows).snapshot().threads);
    reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1_000 });
    app.view = View::Board;
    app
}

fn thread_fixture(rows: usize) -> AppState {
    let snapshot = FakeBackend::seeded().snapshot();
    let mut app = AppState::new(snapshot.threads);
    let thread_id = app.threads[0].id.clone();
    let mut conversation = ConversationState::loading(thread_id.clone());
    conversation.loading = false;
    conversation.items = (0..rows)
        .map(|index| ConversationItem {
            turn_id: format!("turn-{:05}", index / 8),
            item_id: format!("item-{index:05}"),
            kind: match index % 5 {
                0 => ConversationItemKind::User,
                1 => ConversationItemKind::Assistant,
                2 => ConversationItemKind::Reasoning,
                3 => ConversationItemKind::Tool,
                _ => ConversationItemKind::Command,
            },
            text: format!(
                "Synthetic conversation item {index:05} with stable payload for render qualification"
            ),
            status: (index % 7 == 0).then(|| "completed".into()),
        })
        .collect();
    app.conversations.insert(thread_id.0.clone(), conversation);
    app.view = View::Thread(thread_id);
    app
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let board = board_fixture(args.rows);
    let thread = thread_fixture(args.rows);

    let board_stats = measure_render(&board, args.warmup, args.iterations)?;
    let thread_stats = measure_render(&thread, args.warmup, args.iterations)?;

    let output = json!({
        "schema": "codex-tui/ux-performance/v1",
        "sourceSha": args.source_sha,
        "source": args.source,
        "rows": args.rows,
        "warmupIterations": args.warmup,
        "iterations": args.iterations,
        "sampleQualified": args.warmup >= 20 && args.iterations >= 200,
        "terminal": {
            "width": WIDTH,
            "height": HEIGHT,
        },
        "benchmarks": {
            "board10kRender": board_stats,
            "thread10kRender": thread_stats,
        }
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
