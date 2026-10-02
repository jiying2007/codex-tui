use super::{EXIT_DEGRADED, EXIT_OK, OutputFormat, SourceStatus, WorkRow};
use crate::{
    forge::{self, ForgeFreshness},
    sqlite_store::SqliteStore,
};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusSnapshot {
    schema: &'static str,
    generated_at_unix_ms: u64,
    degraded: bool,
    source: SourceStatus,
    local_store_error: Option<String>,
    thread_count: usize,
    work_count: usize,
    needs_you_count: usize,
    stages: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AttentionSnapshot {
    schema: &'static str,
    generated_at_unix_ms: u64,
    degraded: bool,
    source: SourceStatus,
    local_store_error: Option<String>,
    items: Vec<WorkRow>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BoardColumn {
    stage: String,
    items: Vec<WorkRow>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BoardSnapshot {
    schema: &'static str,
    generated_at_unix_ms: u64,
    degraded: bool,
    source: SourceStatus,
    local_store_error: Option<String>,
    columns: Vec<BoardColumn>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CapabilityRow {
    name: String,
    state: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgeStatusSnapshot {
    schema: &'static str,
    generated_at_unix_ms: u64,
    degraded: bool,
    cwd: String,
    provider: Option<String>,
    client_name: Option<String>,
    client_version: Option<String>,
    authenticated: Option<bool>,
    server_version: Option<String>,
    server_edition: Option<String>,
    server_tier: Option<String>,
    remote_host: Option<String>,
    remote_project: Option<String>,
    freshness: String,
    capabilities: Vec<CapabilityRow>,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorktreeRow {
    repo_common_dir: String,
    repo_primary_root: String,
    canonical_path: String,
    branch: Option<String>,
    adopted: bool,
    created_at_unix_ms: u64,
    last_verified_at_unix_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorktreeSnapshot {
    schema: &'static str,
    generated_at_unix_ms: u64,
    degraded: bool,
    local_store_error: Option<String>,
    worktrees: Vec<WorktreeRow>,
}

pub(super) async fn run_status(
    fake: bool,
    fixture_10k: bool,
    format: OutputFormat,
) -> Result<i32> {
    let work = super::build_work_snapshot(fake, fixture_10k).await;
    let mut stages = BTreeMap::new();
    for row in &work.work {
        *stages.entry(row.stage.clone()).or_insert(0) += 1;
    }
    let snapshot = StatusSnapshot {
        schema: "codex-tui/headless-status/v1",
        generated_at_unix_ms: work.generated_at_unix_ms,
        degraded: work.degraded,
        source: work.source,
        local_store_error: work.local_store_error,
        thread_count: work
            .work
            .iter()
            .filter(|row| row.source_kind == "codex-thread")
            .count(),
        work_count: work.work.len(),
        needs_you_count: work
            .work
            .iter()
            .filter(|row| !row.snoozed && !row.attention.is_empty())
            .count(),
        stages,
    };
    print_status(&snapshot, format)?;
    Ok(if snapshot.degraded {
        EXIT_DEGRADED
    } else {
        EXIT_OK
    })
}

pub(super) async fn run_attention(
    fake: bool,
    fixture_10k: bool,
    format: OutputFormat,
) -> Result<i32> {
    let work = super::build_work_snapshot(fake, fixture_10k).await;
    let items = work
        .work
        .iter()
        .filter(|row| !row.snoozed && !row.attention.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    let snapshot = AttentionSnapshot {
        schema: "codex-tui/headless-attention/v1",
        generated_at_unix_ms: work.generated_at_unix_ms,
        degraded: work.degraded,
        source: work.source,
        local_store_error: work.local_store_error,
        items,
    };
    print_attention(&snapshot, format)?;
    Ok(if snapshot.degraded {
        EXIT_DEGRADED
    } else {
        EXIT_OK
    })
}

pub(super) async fn run_board(
    fake: bool,
    fixture_10k: bool,
    format: OutputFormat,
) -> Result<i32> {
    let work = super::build_work_snapshot(fake, fixture_10k).await;
    let columns = ["inbox", "ready", "working", "review", "done"]
        .into_iter()
        .map(|stage| BoardColumn {
            stage: stage.into(),
            items: work
                .work
                .iter()
                .filter(|row| row.stage == stage)
                .cloned()
                .collect(),
        })
        .collect::<Vec<_>>();
    let snapshot = BoardSnapshot {
        schema: "codex-tui/headless-board/v1",
        generated_at_unix_ms: work.generated_at_unix_ms,
        degraded: work.degraded,
        source: work.source,
        local_store_error: work.local_store_error,
        columns,
    };
    print_board(&snapshot, format)?;
    Ok(if snapshot.degraded {
        EXIT_DEGRADED
    } else {
        EXIT_OK
    })
}

pub(super) async fn run_forge(fake: bool, format: OutputFormat) -> Result<i32> {
    let snapshot = if fake {
        ForgeStatusSnapshot {
            schema: "codex-tui/headless-forge/v1",
            generated_at_unix_ms: super::now_unix_ms(),
            degraded: false,
            cwd: "<fake>".into(),
            provider: None,
            client_name: Some("fake".into()),
            client_version: Some("fixture".into()),
            authenticated: Some(true),
            server_version: Some("fixture".into()),
            server_edition: Some("fixture".into()),
            server_tier: None,
            remote_host: None,
            remote_project: None,
            freshness: "fresh".into(),
            capabilities: vec![],
            error: None,
        }
    } else {
        let cwd = match std::env::current_dir() {
            Ok(path) => path.to_string_lossy().into_owned(),
            Err(error) => {
                let snapshot = ForgeStatusSnapshot {
                    schema: "codex-tui/headless-forge/v1",
                    generated_at_unix_ms: super::now_unix_ms(),
                    degraded: true,
                    cwd: String::new(),
                    provider: None,
                    client_name: None,
                    client_version: None,
                    authenticated: None,
                    server_version: None,
                    server_edition: None,
                    server_tier: None,
                    remote_host: None,
                    remote_project: None,
                    freshness: "unavailable".into(),
                    capabilities: vec![],
                    error: Some(format!("current directory: {error}")),
                };
                print_forge(&snapshot, format)?;
                return Ok(EXIT_DEGRADED);
            }
        };
        let doctor = forge::doctor(cwd.clone()).await;
        let mut capabilities = doctor
            .observation
            .capabilities
            .iter()
            .map(|(capability, state)| CapabilityRow {
                name: capability.label().into(),
                state: state.label().into(),
            })
            .collect::<Vec<_>>();
        capabilities.sort_by(|left, right| left.name.cmp(&right.name));
        let degraded = doctor.observation.error.is_some()
            || doctor.authenticated == Some(false)
            || doctor.observation.freshness == ForgeFreshness::Unavailable;
        ForgeStatusSnapshot {
            schema: "codex-tui/headless-forge/v1",
            generated_at_unix_ms: super::now_unix_ms(),
            degraded,
            cwd,
            provider: doctor
                .observation
                .identity
                .as_ref()
                .map(|identity| identity.provider.label().to_string()),
            client_name: doctor.client_name,
            client_version: doctor.client_version,
            authenticated: doctor.authenticated,
            server_version: doctor.server_version,
            server_edition: doctor.server_edition,
            server_tier: doctor.server_tier,
            remote_host: doctor.remote.as_ref().map(|remote| remote.host.clone()),
            remote_project: doctor
                .remote
                .as_ref()
                .map(|remote| remote.path_with_namespace.clone()),
            freshness: doctor.observation.freshness.label().into(),
            capabilities,
            error: doctor.observation.error,
        }
    };
    print_forge(&snapshot, format)?;
    Ok(if snapshot.degraded {
        EXIT_DEGRADED
    } else {
        EXIT_OK
    })
}

pub(super) fn run_worktrees(fake: bool, format: OutputFormat) -> Result<i32> {
    let generated_at_unix_ms = super::now_unix_ms();
    let snapshot = if fake {
        WorktreeSnapshot {
            schema: "codex-tui/headless-worktrees/v1",
            generated_at_unix_ms,
            degraded: false,
            local_store_error: None,
            worktrees: vec![],
        }
    } else {
        match SqliteStore::discover().and_then(|store| store.load_managed_worktrees()) {
            Ok(records) => WorktreeSnapshot {
                schema: "codex-tui/headless-worktrees/v1",
                generated_at_unix_ms,
                degraded: false,
                local_store_error: None,
                worktrees: records
                    .into_iter()
                    .map(|record| WorktreeRow {
                        repo_common_dir: record.repo.git_common_dir,
                        repo_primary_root: record.repo.primary_root,
                        canonical_path: record.canonical_path,
                        branch: record.branch,
                        adopted: record.adopted,
                        created_at_unix_ms: record.created_at_unix_ms,
                        last_verified_at_unix_ms: record.last_verified_at_unix_ms,
                    })
                    .collect(),
            },
            Err(error) => WorktreeSnapshot {
                schema: "codex-tui/headless-worktrees/v1",
                generated_at_unix_ms,
                degraded: true,
                local_store_error: Some(format!("{error:#}")),
                worktrees: vec![],
            },
        }
    };
    print_worktrees(&snapshot, format)?;
    Ok(if snapshot.degraded {
        EXIT_DEGRADED
    } else {
        EXIT_OK
    })
}

fn print_status(snapshot: &StatusSnapshot, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshot)?),
        OutputFormat::Text => {
            println!("schema: {}", snapshot.schema);
            println!("degraded: {}", snapshot.degraded);
            println!(
                "source: {} connected={}",
                snapshot.source.source, snapshot.source.connected
            );
            println!("threads: {}", snapshot.thread_count);
            println!("work: {}", snapshot.work_count);
            println!("needs-you: {}", snapshot.needs_you_count);
            for (stage, count) in &snapshot.stages {
                println!("stage-{stage}: {count}");
            }
            if let Some(error) = &snapshot.local_store_error {
                println!("store-error: {error}");
            }
        }
    }
    Ok(())
}

fn print_attention(snapshot: &AttentionSnapshot, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshot)?),
        OutputFormat::Text => {
            println!("schema: {}", snapshot.schema);
            println!("degraded: {}", snapshot.degraded);
            println!("attention: {}", snapshot.items.len());
            for item in &snapshot.items {
                println!(
                    "{}\t{}\t{}\t{}",
                    item.local_id,
                    item.stage,
                    item.attention.join(","),
                    item.title
                );
            }
        }
    }
    Ok(())
}

fn print_board(snapshot: &BoardSnapshot, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshot)?),
        OutputFormat::Text => {
            println!("schema: {}", snapshot.schema);
            println!("degraded: {}", snapshot.degraded);
            for column in &snapshot.columns {
                println!("[{}] {}", column.stage, column.items.len());
                for item in &column.items {
                    println!("{}\t{}\t{}", item.local_id, item.attention.join(","), item.title);
                }
            }
        }
    }
    Ok(())
}

fn print_forge(snapshot: &ForgeStatusSnapshot, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshot)?),
        OutputFormat::Text => {
            println!("schema: {}", snapshot.schema);
            println!("degraded: {}", snapshot.degraded);
            println!("cwd: {}", snapshot.cwd);
            println!("provider: {}", snapshot.provider.as_deref().unwrap_or("unknown"));
            println!(
                "authenticated: {}",
                snapshot
                    .authenticated
                    .map_or("unknown".into(), |value| value.to_string())
            );
            println!("freshness: {}", snapshot.freshness);
            for capability in &snapshot.capabilities {
                println!("capability-{}: {}", capability.name, capability.state);
            }
            if let Some(error) = &snapshot.error {
                println!("error: {error}");
            }
        }
    }
    Ok(())
}

fn print_worktrees(snapshot: &WorktreeSnapshot, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshot)?),
        OutputFormat::Text => {
            println!("schema: {}", snapshot.schema);
            println!("degraded: {}", snapshot.degraded);
            println!("worktrees: {}", snapshot.worktrees.len());
            for worktree in &snapshot.worktrees {
                println!(
                    "{}\t{}\t{}",
                    worktree.branch.as_deref().unwrap_or("<detached>"),
                    if worktree.adopted { "adopted" } else { "created" },
                    worktree.canonical_path
                );
            }
            if let Some(error) = &snapshot.local_store_error {
                println!("store-error: {error}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_status_and_board_are_deterministic_and_not_degraded() {
        let work = super::super::build_work_snapshot(true, false).await;
        assert!(!work.degraded);
        assert_eq!(work.work.len(), 4);

        let columns = ["inbox", "ready", "working", "review", "done"]
            .into_iter()
            .map(|stage| {
                work.work
                    .iter()
                    .filter(|row| row.stage == stage)
                    .count()
            })
            .collect::<Vec<_>>();
        assert_eq!(columns.iter().sum::<usize>(), work.work.len());
    }

    #[tokio::test]
    async fn fake_forge_and_worktrees_need_no_external_state() {
        assert_eq!(run_forge(true, OutputFormat::Json).await.expect("forge"), EXIT_OK);
        assert_eq!(run_worktrees(true, OutputFormat::Json).expect("worktrees"), EXIT_OK);
    }
}
