//! Explicit, bounded metadata discovery across configured targets; never a mutation route.
use super::*;
use crate::app_server_target::{AppServerConfig, ResolvedAppServerTarget};
use std::{collections::BTreeSet, time::Duration};

const MAX_TARGETS: usize = 8;
const MAX_MATCHES: usize = 100;
const TARGET_TIMEOUT: Duration = Duration::from_secs(20);
const INVENTORY_SCOPE: &str = "ordinary-nonarchived-registry-metadata";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LocatedThread {
    #[serde(flatten)]
    thread: ThreadRow,
    cwd: String,
    updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TargetMatches {
    source: SourceStatus,
    matched_count: usize,
    truncated: bool,
    threads: Vec<LocatedThread>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchReport {
    schema: &'static str,
    generated_at_unix_ms: u64,
    inventory_scope: &'static str,
    degraded: bool,
    error: Option<String>,
    targets: Vec<TargetMatches>,
}

fn target_names(config: &AppServerConfig) -> Result<BTreeSet<String>> {
    anyhow::ensure!(
        !config.active.trim().is_empty()
            && config
                .targets
                .keys()
                .all(|name| !name.is_empty() && name.trim() == name),
        "invalid configured target name"
    );
    let mut names: BTreeSet<_> = config.targets.keys().cloned().collect();
    names.insert(config.active.trim().to_owned());
    anyhow::ensure!(
        names.len() <= MAX_TARGETS,
        "too many targets for one bounded search; limit is {MAX_TARGETS}"
    );
    Ok(names)
}

fn project(threads: Vec<ThreadSummary>, source: SourceStatus, query: &str) -> TargetMatches {
    let mut matched: Vec<_> = threads
        .iter()
        .filter(|thread| {
            [
                thread.id.0.as_str(),
                thread.title.as_str(),
                thread.workspace.as_str(),
                thread.metadata.cwd.as_str(),
                thread.metadata.source.as_str(),
            ]
            .iter()
            .any(|field| field.to_lowercase().contains(query))
        })
        .collect();
    let matched_count = matched.len();
    matched.sort_unstable_by(|left, right| {
        (right.id.0.to_lowercase() == query)
            .cmp(&(left.id.0.to_lowercase() == query))
            .then_with(|| right.metadata.updated_at.cmp(&left.metadata.updated_at))
            .then_with(|| left.id.0.cmp(&right.id.0))
    });
    let threads = matched
        .into_iter()
        .take(MAX_MATCHES)
        .map(|thread| LocatedThread {
            thread: thread_row(thread),
            cwd: thread.metadata.cwd.clone(),
            updated_at: thread.metadata.updated_at,
        })
        .collect();
    TargetMatches {
        source,
        matched_count,
        truncated: matched_count > MAX_MATCHES,
        threads,
    }
}

async fn search_target(config: &AppServerConfig, name: String, query: &str) -> TargetMatches {
    let mut unavailable = SourceStatus::unavailable(Some(name.clone()));
    let target = match ResolvedAppServerTarget::resolve(config, Some(&name)) {
        Ok(target) => target,
        Err(_) => {
            // A malformed configuration may contain a private URL or credential.
            unavailable.error = Some("target configuration invalid or unavailable".into());
            return project(vec![], unavailable, query);
        }
    };
    unavailable.source = target.status_source();
    unavailable.transport = Some(target.endpoint.transport_label().into());
    let (threads, source) =
        match tokio::time::timeout(TARGET_TIMEOUT, SnapshotSource::Target(target).load()).await {
            Ok(result) => result,
            Err(_) => {
                unavailable.error = Some("target search exceeded 20-second deadline".into());
                (vec![], unavailable)
            }
        };
    project(threads, source, query)
}

pub(super) async fn run(options: &HeadlessOptions) -> Result<i32> {
    let mut report = SearchReport {
        schema: "codex-tui/headless-multi-target-threads/v1",
        generated_at_unix_ms: now_unix_ms(),
        inventory_scope: INVENTORY_SCOPE,
        degraded: false,
        error: None,
        targets: vec![],
    };
    let query = options.query.as_deref().unwrap_or_default().to_lowercase();
    match source::read_config() {
        Ok(config) => match target_names(&config.app_server) {
            Ok(names) => {
                // Read one target at a time; dropping a timed-out probe closes its
                // isolated transport. No concurrent registry or persisted index.
                for name in names {
                    report
                        .targets
                        .push(search_target(&config.app_server, name, &query).await);
                }
            }
            Err(error) => report.error = Some(error.to_string()),
        },
        Err(_) => report.error = Some("target configuration unreadable or invalid".into()),
    }
    report.degraded = report.error.is_some()
        || report
            .targets
            .iter()
            .any(|target| target.source.degraded || target.truncated);
    match options.format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
        OutputFormat::Text => {
            println!("schema: {}", report.schema);
            println!("inventory: {}", report.inventory_scope);
            println!("degraded: {}", report.degraded);
            if let Some(error) = &report.error {
                println!("search-error: {}", headless_field(error));
            }
            for target in &report.targets {
                print_source(&target.source);
                println!(
                    "matches: {} shown: {} truncated: {}",
                    target.matched_count,
                    target.threads.len(),
                    target.truncated
                );
                for row in &target.threads {
                    println!(
                        "{}\t{}\t{}\t{}\t{}",
                        headless_field(&row.thread.id),
                        headless_field(&row.thread.runtime),
                        headless_field(&row.thread.title),
                        headless_field(&row.thread.workspace),
                        headless_field(&row.cwd)
                    );
                }
            }
        }
    }
    Ok(if report.degraded {
        EXIT_DEGRADED
    } else {
        EXIT_OK
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    #[test]
    fn match_limit_is_explicit_and_exact_id_outranks_recent_partial_match() {
        let mut threads = FakeBackend::scaled(150).snapshot().threads;
        for thread in &mut threads {
            thread.title = "FindMe".into();
            thread.metadata.updated_at = 100;
        }
        threads[149].id.0 = "FindMe".into();
        threads[149].metadata.updated_at = 0;
        let target = project(threads, SourceStatus::unavailable(None), "findme");
        assert_eq!(target.matched_count, 150);
        assert!(target.truncated);
        assert_eq!(target.threads.len(), MAX_MATCHES);
        assert_eq!(target.threads[0].thread.id, "FindMe");
    }

    #[test]
    fn default_config_names_only_implicit_local_and_excess_targets_are_rejected() {
        let mut config = AppServerConfig::default();
        assert_eq!(target_names(&config).unwrap(), BTreeSet::from(["local".into()]));
        for i in 0..MAX_TARGETS {
            config.targets.insert(
                format!("remote-{i}"),
                crate::app_server_target::AppServerTargetConfig::Stdio { codex_bin: None },
            );
        }
        assert!(target_names(&config).is_err());
    }
}
