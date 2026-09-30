use crate::{
    app_server,
    backend::{BackendSnapshot, CodexBackend, FakeBackend},
    compat,
    domain::{AttentionReason, ThreadSummary},
    planning::{
        Freshness, PlanningSnapshot, ReconcileInput, SourceKind, SourceRef, WorkCardProjection,
        reconcile_scratch_card_with_local, reconcile_thread_card,
    },
    sqlite_store::SqliteStore,
    store::{LocalStateV1, LocalStore},
};
use anyhow::Result;
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

pub const EXIT_OK: i32 = 0;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_DEGRADED: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceStatus {
    source: String,
    connected: bool,
    version: Option<String>,
    platform: Option<String>,
    capabilities: Vec<String>,
    optional_capabilities_missing: Vec<String>,
    last_refresh_unix_ms: Option<u64>,
    degraded: bool,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadRow {
    id: String,
    workspace: String,
    title: String,
    runtime: String,
    attention: Vec<String>,
    pinned: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadSnapshot {
    schema: &'static str,
    generated_at_unix_ms: u64,
    degraded: bool,
    source: SourceStatus,
    threads: Vec<ThreadRow>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProvenanceRow {
    source: String,
    freshness: String,
    degraded_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkRow {
    local_id: String,
    source_kind: String,
    source_ref: String,
    title: String,
    workspace: Option<String>,
    stage: String,
    stage_reason: String,
    attention: Vec<String>,
    snoozed: bool,
    priority: Option<i32>,
    provenance: Vec<ProvenanceRow>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkSnapshot {
    schema: &'static str,
    generated_at_unix_ms: u64,
    degraded: bool,
    source: SourceStatus,
    local_store_error: Option<String>,
    work: Vec<WorkRow>,
}

pub async fn run(args: &[String]) -> Result<i32> {
    let Some(command) = args.first().map(String::as_str) else {
        print_usage();
        return Ok(EXIT_USAGE);
    };
    let flags = &args[1..];
    if flags
        .iter()
        .any(|arg| !matches!(arg.as_str(), "--json" | "--fake" | "--fixture-10k"))
    {
        eprintln!("unknown headless option");
        print_usage();
        return Ok(EXIT_USAGE);
    }

    let format = if flags.iter().any(|arg| arg == "--json") {
        OutputFormat::Json
    } else {
        OutputFormat::Text
    };
    let fixture_10k = flags.iter().any(|arg| arg == "--fixture-10k");
    let fake = fixture_10k || flags.iter().any(|arg| arg == "--fake");

    match command {
        "threads" => {
            let snapshot = build_thread_snapshot(fake, fixture_10k).await;
            let exit = if snapshot.degraded {
                EXIT_DEGRADED
            } else {
                EXIT_OK
            };
            print_thread_snapshot(&snapshot, format)?;
            Ok(exit)
        }
        "work" => {
            let snapshot = build_work_snapshot(fake, fixture_10k).await;
            let exit = if snapshot.degraded {
                EXIT_DEGRADED
            } else {
                EXIT_OK
            };
            print_work_snapshot(&snapshot, format)?;
            Ok(exit)
        }
        _ => {
            eprintln!("unknown headless command: {command}");
            print_usage();
            Ok(EXIT_USAGE)
        }
    }
}

pub async fn doctor_compat(json: bool) -> Result<i32> {
    let report = compat::probe().await;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        compat::print_text(&report);
    }
    Ok(if report.is_blocked() {
        EXIT_DEGRADED
    } else {
        EXIT_OK
    })
}

async fn build_thread_snapshot(fake: bool, fixture_10k: bool) -> ThreadSnapshot {
    let generated_at_unix_ms = now_unix_ms();
    match load_backend_snapshot(fake, fixture_10k).await {
        Ok(snapshot) => {
            let source = source_status(&snapshot);
            let degraded = source.degraded;
            let mut threads = snapshot.threads.iter().map(thread_row).collect::<Vec<_>>();
            threads.sort_by(|left, right| left.id.cmp(&right.id));
            ThreadSnapshot {
                schema: "codex-tui/headless-threads/v1",
                generated_at_unix_ms,
                degraded,
                source,
                threads,
            }
        }
        Err(error) => ThreadSnapshot {
            schema: "codex-tui/headless-threads/v1",
            generated_at_unix_ms,
            degraded: true,
            source: SourceStatus {
                source: if fake {
                    "fake".into()
                } else {
                    "codex-app-server".into()
                },
                connected: false,
                version: None,
                platform: None,
                capabilities: vec![],
                optional_capabilities_missing: vec![],
                last_refresh_unix_ms: None,
                degraded: true,
                error: Some(format!("{error:#}")),
            },
            threads: vec![],
        },
    }
}

async fn build_work_snapshot(fake: bool, fixture_10k: bool) -> WorkSnapshot {
    let generated_at_unix_ms = now_unix_ms();
    let (local, planning, local_store_error) = if fake {
        (LocalStateV1::default(), PlanningSnapshot::default(), None)
    } else {
        match SqliteStore::discover() {
            Ok(store) => {
                let local = store.load_state();
                let planning = store.load_planning_snapshot();
                match (local, planning) {
                    (Ok(local), Ok(planning)) => (local, planning, None),
                    (local, planning) => {
                        let error = local
                            .as_ref()
                            .err()
                            .map(|error| format!("operator state: {error:#}"))
                            .or_else(|| {
                                planning
                                    .as_ref()
                                    .err()
                                    .map(|error| format!("planning state: {error:#}"))
                            });
                        (
                            local.unwrap_or_default(),
                            planning.unwrap_or_default(),
                            error,
                        )
                    }
                }
            }
            Err(error) => (
                LocalStateV1::default(),
                PlanningSnapshot::default(),
                Some(format!("store discovery: {error:#}")),
            ),
        }
    };

    let backend = load_backend_snapshot(fake, fixture_10k).await;
    let (mut threads, source) = match backend {
        Ok(snapshot) => {
            let source = source_status(&snapshot);
            (snapshot.threads, source)
        }
        Err(error) => (
            vec![],
            SourceStatus {
                source: if fake {
                    "fake".into()
                } else {
                    "codex-app-server".into()
                },
                connected: false,
                version: None,
                platform: None,
                capabilities: vec![],
                optional_capabilities_missing: vec![],
                last_refresh_unix_ms: None,
                degraded: true,
                error: Some(format!("{error:#}")),
            },
        ),
    };

    apply_local_operator_state(&mut threads, &local);

    let mut projections = Vec::with_capacity(threads.len() + planning.scratch.len());
    for thread in &threads {
        let anchor = SourceRef::codex_thread(&thread.id);
        let local_card = planning.cards.iter().find(|card| card.anchor == anchor);
        projections.push(reconcile_thread_card(ReconcileInput {
            thread,
            git: None,
            local: local_card,
            collision_count: 0,
            backend_observed_at_unix_ms: source.last_refresh_unix_ms,
            backend_error: source.error.as_deref(),
            now_unix_ms: generated_at_unix_ms,
        }));
    }
    for scratch in &planning.scratch {
        let anchor = SourceRef {
            kind: SourceKind::ScratchWork,
            value: scratch.id.clone(),
        };
        let local_card = planning.cards.iter().find(|card| card.anchor == anchor);
        projections.push(reconcile_scratch_card_with_local(
            scratch,
            local_card,
            generated_at_unix_ms,
        ));
    }

    let mut work = projections.iter().map(work_row).collect::<Vec<_>>();
    work.sort_by(|left, right| left.local_id.cmp(&right.local_id));
    let degraded = source.degraded || local_store_error.is_some();

    WorkSnapshot {
        schema: "codex-tui/headless-work/v1",
        generated_at_unix_ms,
        degraded,
        source,
        local_store_error,
        work,
    }
}

async fn load_backend_snapshot(fake: bool, fixture_10k: bool) -> Result<BackendSnapshot> {
    if fixture_10k {
        return Ok(FakeBackend::scaled(10_000).snapshot());
    }
    if fake {
        return Ok(FakeBackend::seeded().snapshot());
    }
    app_server::probe(None).await
}

fn apply_local_operator_state(threads: &mut [ThreadSummary], local: &LocalStateV1) {
    for thread in threads {
        thread.pinned = local.pins.contains(&thread.id.0);
        thread.alias = local.aliases.get(&thread.id.0).cloned();
        if local.marked_unread.contains(&thread.id.0)
            && !thread.attention.contains(&AttentionReason::MarkedUnread)
        {
            thread.attention.push(AttentionReason::MarkedUnread);
        }
    }
}

fn source_status(snapshot: &BackendSnapshot) -> SourceStatus {
    let status = &snapshot.status;
    SourceStatus {
        source: status.source.clone(),
        connected: status.connected,
        version: status.version.clone(),
        platform: status.platform.clone(),
        capabilities: status.capabilities.clone(),
        optional_capabilities_missing: status.optional_capabilities_missing.clone(),
        last_refresh_unix_ms: status.last_refresh_unix_ms,
        degraded: !status.connected || status.error.is_some(),
        error: status.error.clone(),
    }
}

fn thread_row(thread: &ThreadSummary) -> ThreadRow {
    let mut attention = thread
        .attention
        .iter()
        .map(|reason| reason.label().to_string())
        .collect::<Vec<_>>();
    attention.sort();
    attention.dedup();
    ThreadRow {
        id: thread.id.0.clone(),
        workspace: thread.workspace.clone(),
        title: thread.display_title().to_string(),
        runtime: thread.runtime.label().to_ascii_lowercase(),
        attention,
        pinned: thread.pinned,
    }
}

fn work_row(card: &WorkCardProjection) -> WorkRow {
    WorkRow {
        local_id: card.local_id.clone(),
        source_kind: source_kind(&card.anchor.kind).into(),
        source_ref: card.anchor.value.clone(),
        title: card.title.clone(),
        workspace: card.workspace.clone(),
        stage: card.stage.label().to_ascii_lowercase(),
        stage_reason: card.stage_reason.clone(),
        attention: card
            .attention
            .iter()
            .map(|attention| attention.label().to_string())
            .collect(),
        snoozed: card.snoozed,
        priority: card.overlay.priority,
        provenance: card
            .provenance
            .iter()
            .map(|provenance| ProvenanceRow {
                source: provenance.source.clone(),
                freshness: freshness_label(provenance.freshness).into(),
                degraded_reason: provenance.degraded_reason.clone(),
            })
            .collect(),
    }
}

fn source_kind(kind: &SourceKind) -> &'static str {
    match kind {
        SourceKind::ScratchWork => "scratch-work",
        SourceKind::CodexThread => "codex-thread",
        SourceKind::ForgeWorkItem => "forge-work-item",
        SourceKind::Goal => "goal",
        SourceKind::Worktree => "worktree",
        SourceKind::ChangeRequest => "change-request",
    }
}

fn freshness_label(freshness: Freshness) -> &'static str {
    match freshness {
        Freshness::Fresh => "fresh",
        Freshness::Aging => "aging",
        Freshness::Stale => "stale",
        Freshness::Unavailable => "unavailable",
    }
}

fn print_thread_snapshot(snapshot: &ThreadSnapshot, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshot)?),
        OutputFormat::Text => {
            println!("schema: {}", snapshot.schema);
            println!("degraded: {}", snapshot.degraded);
            println!(
                "source: {} connected={}",
                snapshot.source.source, snapshot.source.connected
            );
            if let Some(error) = &snapshot.source.error {
                println!("error: {error}");
            }
            println!("threads: {}", snapshot.threads.len());
            for thread in &snapshot.threads {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    thread.id,
                    thread.runtime,
                    thread.attention.join(","),
                    thread.workspace,
                    thread.title
                );
            }
        }
    }
    Ok(())
}

fn print_work_snapshot(snapshot: &WorkSnapshot, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshot)?),
        OutputFormat::Text => {
            println!("schema: {}", snapshot.schema);
            println!("degraded: {}", snapshot.degraded);
            println!(
                "source: {} connected={}",
                snapshot.source.source, snapshot.source.connected
            );
            if let Some(error) = &snapshot.source.error {
                println!("source-error: {error}");
            }
            if let Some(error) = &snapshot.local_store_error {
                println!("store-error: {error}");
            }
            println!("work: {}", snapshot.work.len());
            for card in &snapshot.work {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    card.local_id,
                    card.stage,
                    card.attention.join(","),
                    card.workspace.as_deref().unwrap_or(""),
                    card.title
                );
            }
        }
    }
    Ok(())
}

fn print_usage() {
    eprintln!("usage: codex-tui headless <threads|work> [--json] [--fake|--fixture-10k]");
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fixture_10k_thread_snapshot_is_stable_and_not_degraded() {
        let snapshot = build_thread_snapshot(true, true).await;
        assert_eq!(snapshot.schema, "codex-tui/headless-threads/v1");
        assert!(!snapshot.degraded);
        assert_eq!(snapshot.threads.len(), 10_000);
        assert_eq!(
            snapshot.threads.first().expect("first").id,
            "thread-scale-00000"
        );
        assert_eq!(
            snapshot.threads.last().expect("last").id,
            "thread-scale-09999"
        );
    }

    #[tokio::test]
    async fn fixture_work_snapshot_does_not_probe_or_load_local_state() {
        let snapshot = build_work_snapshot(true, false).await;
        assert_eq!(snapshot.schema, "codex-tui/headless-work/v1");
        assert!(!snapshot.degraded);
        assert_eq!(snapshot.work.len(), 4);
        assert!(snapshot.local_store_error.is_none());
    }
}
