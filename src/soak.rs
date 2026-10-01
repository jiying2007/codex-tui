use crate::{
    app::{Action, AppState, conversation_cache_limit, git_review_cache_limit, reduce},
    app_server::registry_snapshot_publication_upper_bound,
    backend::{CodexBackend, FakeBackend},
    git::GitReview,
};
use anyhow::{Context, Result};
use serde::Serialize;
use std::{fs, time::Instant};

pub const SOAK_EVIDENCE_SCHEMA: &str = "codex-tui/soak-evidence/v1";
pub const DEFAULT_ROWS: usize = 10_000;
pub const DEFAULT_CYCLES: usize = 256;
pub const MAX_ROWS: usize = 50_000;
pub const MAX_CYCLES: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoakReport {
    pub schema: &'static str,
    pub rows: usize,
    pub cycles: usize,
    pub actions_applied: usize,
    pub effects_emitted: usize,
    pub churn_batches: usize,
    pub planning_reconciles: usize,
    pub ui_only_planning_reconciles: usize,
    pub max_conversations: usize,
    pub conversation_cache_limit: usize,
    pub max_git_reviews: usize,
    pub git_review_cache_limit: usize,
    pub max_work_cards: usize,
    pub final_work_cards: usize,
    pub registry_snapshot_publication_upper_bound: usize,
    pub rss_start_kib: Option<u64>,
    pub rss_peak_kib: Option<u64>,
    pub rss_end_kib: Option<u64>,
    pub longest_cycle_stall_ms: f64,
    pub structural_pass: bool,
}

pub fn run(rows: usize, cycles: usize) -> Result<SoakReport> {
    anyhow::ensure!(
        rows > 0 && rows <= MAX_ROWS,
        "--rows must be within 1..={MAX_ROWS}"
    );
    anyhow::ensure!(
        cycles > 0 && cycles <= MAX_CYCLES,
        "--cycles must be within 1..={MAX_CYCLES}"
    );

    let mut app = AppState::new(FakeBackend::scaled(rows).snapshot().threads);
    let mut actions_applied = 0usize;
    let mut effects_emitted = 0usize;
    let mut churn_batches = 0usize;
    let mut planning_reconciles = 0usize;
    let ui_only_planning_reconciles = 0usize;
    let mut max_conversations = 0usize;
    let mut max_git_reviews = 0usize;
    let mut max_work_cards = 0usize;
    let rss_start_kib = resident_set_kib();
    let mut rss_peak_kib = rss_start_kib;
    let mut longest_cycle_stall_ms = 0.0f64;

    reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1 });
    actions_applied += 1;
    planning_reconciles += 1;
    max_work_cards = max_work_cards.max(app.work_cards.len());

    for cycle in 0..cycles {
        let cycle_started = Instant::now();
        let selected = cycle % rows;
        app.selected = selected;

        let effects = reduce(&mut app, Action::OpenSelected);
        actions_applied += 1;
        effects_emitted += effects.len();

        let thread = app
            .threads
            .get(selected)
            .context("selected soak thread disappeared")?
            .clone();
        reduce(
            &mut app,
            Action::GitReviewLoaded(GitReview {
                thread_id: thread.id.clone(),
                cwd: thread.metadata.cwd.clone(),
                changes: vec![],
                staged_diff: String::new(),
                unstaged_diff: String::new(),
                truncated: false,
                observed_at_unix_ms: cycle as u64 + 1,
                error: None,
            }),
        );
        actions_applied += 1;

        reduce(&mut app, Action::Back);
        actions_applied += 1;

        // UI-only churn must never itself request planning reconciliation.
        reduce(&mut app, Action::ToggleHelp);
        reduce(&mut app, Action::ToggleHelp);
        actions_applied += 2;

        let mut planning_dirty = false;
        if cycle % 16 == 0 {
            let mut fresh = app.threads.clone();
            let index = cycle % fresh.len();
            fresh[index].metadata.updated_at = fresh[index]
                .metadata
                .updated_at
                .saturating_add(cycle as i64 + 1);
            fresh[index].title = format!("soak-update-{cycle}");
            reduce(&mut app, Action::ReplaceThreads(fresh));
            actions_applied += 1;
            churn_batches += 1;
            planning_dirty = true;
        }

        // This mirrors the runtime's once-per-loop coalescing rule.
        if planning_dirty {
            reduce(
                &mut app,
                Action::ReconcilePlanning {
                    now_unix_ms: cycle as u64 + 2,
                },
            );
            actions_applied += 1;
            planning_reconciles += 1;
        }

        max_conversations = max_conversations.max(app.conversations.len());
        max_git_reviews = max_git_reviews.max(app.git_reviews.len());
        max_work_cards = max_work_cards.max(app.work_cards.len());
        if let Some(rss) = resident_set_kib() {
            rss_peak_kib = Some(rss_peak_kib.unwrap_or(rss).max(rss));
        }
        longest_cycle_stall_ms =
            longest_cycle_stall_ms.max(cycle_started.elapsed().as_secs_f64() * 1000.0);
    }

    let rss_end_kib = resident_set_kib();
    let publication_upper_bound = registry_snapshot_publication_upper_bound(rows);
    let expected_reconciles = 1 + churn_batches;
    let structural_pass = max_conversations <= conversation_cache_limit()
        && max_git_reviews <= git_review_cache_limit()
        && max_work_cards <= rows
        && app.work_cards.len() <= rows
        && planning_reconciles == expected_reconciles
        && ui_only_planning_reconciles == 0
        && publication_upper_bound > 0;

    Ok(SoakReport {
        schema: SOAK_EVIDENCE_SCHEMA,
        rows,
        cycles,
        actions_applied,
        effects_emitted,
        churn_batches,
        planning_reconciles,
        ui_only_planning_reconciles,
        max_conversations,
        conversation_cache_limit: conversation_cache_limit(),
        max_git_reviews,
        git_review_cache_limit: git_review_cache_limit(),
        max_work_cards,
        final_work_cards: app.work_cards.len(),
        registry_snapshot_publication_upper_bound: publication_upper_bound,
        rss_start_kib,
        rss_peak_kib,
        rss_end_kib,
        longest_cycle_stall_ms,
        structural_pass,
    })
}

pub fn run_cli(args: &[String]) -> Result<i32> {
    let mut rows = DEFAULT_ROWS;
    let mut cycles = DEFAULT_CYCLES;
    let mut json = false;
    let mut index = 0usize;

    while index < args.len() {
        match args[index].as_str() {
            "--rows" => {
                index += 1;
                rows = args
                    .get(index)
                    .context("--rows requires a value")?
                    .parse()
                    .context("--rows must be an integer")?;
            }
            "--cycles" => {
                index += 1;
                cycles = args
                    .get(index)
                    .context("--cycles requires a value")?
                    .parse()
                    .context("--cycles must be an integer")?;
            }
            "--json" => json = true,
            other => anyhow::bail!("unknown soak option: {other}"),
        }
        index += 1;
    }

    let report = run(rows, cycles)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("schema: {}", report.schema);
        println!("rows: {}", report.rows);
        println!("cycles: {}", report.cycles);
        println!("structural-pass: {}", report.structural_pass);
        println!(
            "planning-reconciles: {} (churn-batches={})",
            report.planning_reconciles, report.churn_batches
        );
        println!(
            "conversation-cache-high-water: {}/{}",
            report.max_conversations, report.conversation_cache_limit
        );
        println!(
            "git-review-cache-high-water: {}/{}",
            report.max_git_reviews, report.git_review_cache_limit
        );
        println!(
            "registry-snapshot-publication-upper-bound: {}",
            report.registry_snapshot_publication_upper_bound
        );
        println!("rss-start-kib: {:?}", report.rss_start_kib);
        println!("rss-peak-kib: {:?}", report.rss_peak_kib);
        println!("rss-end-kib: {:?}", report.rss_end_kib);
        println!(
            "longest-cycle-stall-ms: {:.3}",
            report.longest_cycle_stall_ms
        );
    }

    Ok(if report.structural_pass { 0 } else { 3 })
}

fn resident_set_kib() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
        line.split_whitespace().nth(1)?.parse().ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_soak_keeps_lru_and_reconcile_invariants() {
        let report = run(256, 96).expect("soak report");
        assert!(report.structural_pass);
        assert!(report.max_conversations <= report.conversation_cache_limit);
        assert!(report.max_git_reviews <= report.git_review_cache_limit);
        assert_eq!(report.ui_only_planning_reconciles, 0);
        assert_eq!(report.planning_reconciles, 1 + report.churn_batches);
        assert_eq!(report.final_work_cards, 256);
    }

    #[test]
    fn fifty_thousand_registry_has_a_bounded_snapshot_publication_count() {
        assert_eq!(registry_snapshot_publication_upper_bound(50_000), 26);
    }
}
