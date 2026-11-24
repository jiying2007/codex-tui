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
        right
            .id
            .0
            .eq_ignore_ascii_case(query)
            .cmp(&left.id.0.eq_ignore_ascii_case(query))
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
    match crate::app_server::probe_metadata_target(target, TARGET_TIMEOUT).await {
        Ok(snapshot) => {
            unavailable.observe(&snapshot);
            project(snapshot.threads, unavailable, query)
        }
        Err(error) => {
            unavailable.error = Some(format!("{error:#}"));
            project(vec![], unavailable, query)
        }
    }
}

fn empty_report() -> SearchReport {
    SearchReport {
        schema: "codex-tui/headless-multi-target-threads/v1",
        generated_at_unix_ms: now_unix_ms(),
        inventory_scope: INVENTORY_SCOPE,
        degraded: false,
        error: None,
        targets: vec![],
    }
}

async fn collect_report(
    config: &AppServerConfig,
    query: &str,
    mut progress: impl FnMut(&SearchReport, usize),
) -> SearchReport {
    let mut report = empty_report();
    if let Err(error) = validate_options("threads", &["--all-targets", "--query", query]) {
        report.error = Some(error);
        report.degraded = true;
        return report;
    }
    let query = query.trim().to_lowercase();
    match target_names(config) {
        Ok(names) => {
            let total = names.len();
            progress(&report, total);
            for name in names {
                let target = search_target(config, name, &query).await;
                report.degraded |= target.source.degraded || target.truncated;
                report.targets.push(target);
                if report.targets.len() < total {
                    progress(&report, total);
                }
            }
        }
        Err(error) => report.error = Some(error.to_string()),
    }
    report.degraded |= report.error.is_some();
    report
}

/// The TUI and CLI use the same bounded, isolated metadata search.
pub async fn target_lookup_pages(
    config: &AppServerConfig,
    query: &str,
    language: crate::i18n::UiLanguage,
    mut progress: impl FnMut(Vec<String>),
) -> Vec<String> {
    let report = collect_report(config, query, |report, total| {
        progress(report_pages(report, language, total));
    })
    .await;
    report_pages(&report, language, report.targets.len())
}

fn report_pages(
    report: &SearchReport,
    language: crate::i18n::UiLanguage,
    total: usize,
) -> Vec<String> {
    use crate::i18n::pick;
    let status = if report.targets.len() < total {
        pick(language, "Partial / query in progress", "部分结果 / 检索进行中")
    } else if report.degraded {
        pick(language, "Partial / unavailable", "部分结果 / 不可用")
    } else {
        pick(language, "Complete for this scope", "本次范围完整")
    };
    let mut pages = vec![format!(
        "{status}\nTargets: {}/{}\n{}\n{}",
        report.targets.len(),
        total,
        pick(
            language,
            "Ordinary non-archived metadata only.",
            "仅普通未归档会话元数据。"
        ),
        pick(
            language,
            "Read-only. Reverify identity on the corresponding target before resuming.",
            "只读结果。恢复会话前请到对应目标重新核验身份。"
        ),
    )];
    if let Some(error) = &report.error {
        pages[0].push_str(&format!("\nError: {}", headless_field(error)));
    }
    for target in &report.targets {
        let source = &target.source;
        let name = headless_field(source.target_id.as_deref().unwrap_or("<unknown>"));
        let home = headless_field(source.codex_home.as_deref().unwrap_or("<unknown>"));
        let transport = headless_field(source.transport.as_deref().unwrap_or("<unknown>"));
        let status = if source.degraded || target.truncated {
            if source.connected || !target.threads.is_empty() {
                "Partial"
            } else {
                "Unavailable"
            }
        } else {
            "Complete"
        };
        let scope =
            format!("Target: {name}\nCODEX_HOME: {home}\nTransport: {transport}\nStatus: {status}");
        pages.push(format!(
            "{scope}\nMatches observed: {}\nShown: {}\nTruncated: {}\nDiagnostic: {}",
            target.matched_count,
            target.threads.len(),
            target.truncated,
            crate::text::truncate_display(source.error.as_deref().unwrap_or(""), 1024),
        ));
        for row in &target.threads {
            pages.push(format!(
                "{scope}\nThreadId: {}\nCWD: {}\nTitle: {}\nWorkspace: {}\nRuntime: {}\nAttention: {}",
                headless_field(&row.thread.id),
                headless_field(&row.cwd),
                crate::text::truncate_display(&row.thread.title, 240),
                crate::text::truncate_display(&row.thread.workspace, 240),
                headless_field(&row.thread.runtime),
                headless_field(&row.thread.attention.join(",")),
            ));
        }
    }
    pages
}

pub(super) async fn run(options: &HeadlessOptions) -> Result<i32> {
    let report = match source::read_config() {
        Ok(config) => {
            collect_report(
                &config.app_server,
                options.query.as_deref().unwrap_or_default(),
                |_, _| {},
            )
            .await
        }
        Err(_) => {
            let mut report = empty_report();
            report.error = Some("target configuration unreadable or invalid".into());
            report.degraded = true;
            report
        }
    };
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
    fn locator_pages_keep_same_id_in_distinct_target_scopes() {
        let mut report = empty_report();
        for name in ["first", "second"] {
            let mut threads = FakeBackend::seeded().snapshot().threads;
            threads.truncate(1);
            threads[0].id.0 = "shared-id".into();
            let mut source = SourceStatus::unavailable(Some(name.into()));
            source.codex_home = Some(format!("/{name}/.codex"));
            report.targets.push(project(threads, source, "shared-id"));
        }
        report.degraded = true;
        let pages = report_pages(&report, crate::i18n::UiLanguage::English, 2);
        assert_eq!(pages.len(), 5);
        assert!(pages[0].contains("Partial"));
        for (index, name) in [(2, "first"), (4, "second")] {
            assert!(pages[index].contains(&format!("Target: {name}")));
            assert!(pages[index].contains(&format!("CODEX_HOME: /{name}/.codex")));
            assert!(pages[index].contains("ThreadId: shared-id"));
        }
    }

    #[test]
    fn default_config_names_only_implicit_local_and_excess_targets_are_rejected() {
        let mut config = AppServerConfig::default();
        assert_eq!(
            target_names(&config).unwrap(),
            BTreeSet::from([String::from("local")])
        );
        for i in 0..MAX_TARGETS {
            config.targets.insert(
                format!("remote-{i}"),
                crate::app_server_target::AppServerTargetConfig::Stdio { codex_bin: None },
            );
        }
        assert!(target_names(&config).is_err());
    }
}
