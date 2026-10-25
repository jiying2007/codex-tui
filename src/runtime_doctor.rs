use anyhow::Result;
use codex_tui::{
    app_server,
    app_server_target::ResolvedAppServerTarget,
    backend::BackendStatus,
    domain::{CwdLocality, classify_cwd, display_cwd},
    forge, git,
    sqlite_store::SqliteStore,
    store::LocalStore,
};

fn doctor_line(args: std::fmt::Arguments<'_>) -> String {
    codex_tui::text::sanitize_inline(&args.to_string())
}

macro_rules! doctor_println {
    ($($arg:tt)*) => {
        println!("{}", doctor_line(format_args!($($arg)*)))
    };
}

pub(crate) async fn doctor(scope: Option<&str>, target_override: Option<&str>) -> Result<i32> {
    let mut degraded = false;
    let store = SqliteStore::discover()?;
    let config = store.load_config()?;
    let target = ResolvedAppServerTarget::resolve(&config.app_server, target_override)?;

    doctor_println!("codex-tui {}", env!("CARGO_PKG_VERSION"));
    doctor_println!("config: {}", store.config_path().display());
    doctor_println!("state: {}", store.db_path().display());
    doctor_println!(
        "mouse: disabled (pointer navigation not implemented; legacy config mouse={} ignored)",
        config.ui.mouse
    );
    doctor_println!(
        "language: {} -> {}",
        config.ui.language.as_str(),
        config.ui.language.resolve().as_str()
    );
    doctor_println!("notifications: {}", config.notifications.mode.label());
    if config.notifications.mode == codex_tui::notification::NotificationMode::Off {
        doctor_println!(
            "notification-tip: alerts are opt-in; set [notifications].mode = \"terminal\" for SSH client BEL (not Windows Toast)"
        );
    }
    doctor_println!("app-server-target: {}", target.name);
    doctor_println!("app-server-endpoint: {}", target.diagnostic_endpoint());
    doctor_println!(
        "presentation: {} · background-redraw-min={}ms",
        config.ui.presentation.label(),
        config.ui.presentation.background_redraw_interval_ms()
    );

    // Screen the existing image before any read path can initialize derivative
    // tables or configure WAL. Failed admission is shared by all store clones.
    let health = store.health();
    match store.load_state() {
        Ok(state) => doctor_println!("operator-schemaVersion: {}", state.schema_version),
        Err(error) => {
            degraded = true;
            doctor_println!("operator-state: DEGRADED · {error:#}");
        }
    }

    if scope == Some("store") {
        doctor_println!(
            "integrity-scope: read-only SQLite quick_check plus current application CHECK constraints"
        );
        match health {
            Ok(health) => {
                doctor_println!("store-backend: sqlite");
                doctor_println!("store-schema: {}", health.schema_version);
                doctor_println!("integrity: {}", health.integrity);
                match store.load_planning_snapshot() {
                    Ok(snapshot) => {
                        doctor_println!("work-cards: {}", snapshot.cards.len());
                        doctor_println!("scratch: {}", snapshot.scratch.len());
                        doctor_println!("saved-views: {}", snapshot.saved_views.len());
                    }
                    Err(error) => {
                        degraded = true;
                        doctor_println!("planning: DEGRADED · {error:#}");
                    }
                }
                match store.load_managed_worktrees() {
                    Ok(worktrees) => doctor_println!("managed-worktrees: {}", worktrees.len()),
                    Err(error) => {
                        degraded = true;
                        doctor_println!("managed-worktrees: DEGRADED · {error:#}");
                    }
                }
                match store.load_recent_operation_receipts(10) {
                    Ok(receipts) => {
                        doctor_println!("recent-operation-receipts: {}", receipts.len());
                        for receipt in receipts {
                            doctor_println!(
                                "receipt: {} · {} · {:?}",
                                receipt.operation_id,
                                receipt.plan.kind.label(),
                                receipt.state
                            );
                        }
                    }
                    Err(error) => {
                        degraded = true;
                        doctor_println!("operation-receipts: DEGRADED · {error:#}");
                    }
                }
                match store.load_recent_forge_mutation_receipts(10) {
                    Ok(receipts) => {
                        doctor_println!("recent-forge-mutation-receipts: {}", receipts.len());
                        for receipt in receipts {
                            doctor_println!(
                                "forge-receipt: {} · {} · {:?} · {}/{} · mr={}",
                                receipt.operation_id,
                                receipt.plan.kind.label(),
                                receipt.state,
                                receipt.plan.host,
                                receipt.plan.project_path,
                                receipt
                                    .plan
                                    .change_request_iid
                                    .map_or_else(|| "-".into(), |iid| iid.to_string())
                            );
                        }
                    }
                    Err(error) => {
                        degraded = true;
                        doctor_println!("forge-mutation-receipts: DEGRADED · {error:#}");
                    }
                }
            }
            Err(error) => {
                degraded = true;
                doctor_println!("store-backend: sqlite");
                doctor_println!("integrity: DEGRADED");
                doctor_println!("error: {error:#}");
            }
        }
    } else if scope == Some("git") {
        let cwd = std::env::current_dir()?;
        let context = git::probe_context(
            codex_tui::domain::ThreadId::new("doctor"),
            cwd.to_string_lossy().into_owned(),
        )
        .await?;
        degraded |= !context.is_repository || context.error.is_some();
        doctor_println!("git-repository: {}", context.is_repository);
        doctor_println!("cwd: {}", context.cwd);
        if let Some(repo) = context.repo {
            doctor_println!("git-common-dir: {}", repo.git_common_dir);
            doctor_println!("repo-root: {}", repo.primary_root);
        }
        if let Some(worktree) = context.worktree {
            doctor_println!("worktree: {}", worktree.canonical_path);
        }
        doctor_println!(
            "branch: {}",
            context.branch.as_deref().unwrap_or("<detached/none>")
        );
        doctor_println!("dirty: {}", context.dirty);
        doctor_println!("changed-files: {}", context.changes.len());
        if let Some(error) = context.error {
            doctor_println!("error: {error}");
        }
    } else if scope == Some("forge") {
        let cwd = std::env::current_dir()?;
        let snapshot = forge::doctor(cwd.to_string_lossy().into_owned()).await;
        doctor_println!(
            "forge-client: {}",
            snapshot.client_name.as_deref().unwrap_or("<unresolved>")
        );
        doctor_println!(
            "forge-client-version: {}",
            snapshot
                .client_version
                .as_deref()
                .unwrap_or("<unavailable>")
        );
        doctor_println!(
            "forge-authenticated: {}",
            snapshot
                .authenticated
                .map(|value| value.to_string())
                .as_deref()
                .unwrap_or("<unknown>")
        );
        doctor_println!(
            "forge-server-version: {}",
            snapshot.server_version.as_deref().unwrap_or("<unknown>")
        );
        doctor_println!(
            "forge-server-edition: {}",
            snapshot.server_edition.as_deref().unwrap_or("<unknown>")
        );
        doctor_println!(
            "forge-server-tier: {}",
            snapshot
                .server_tier
                .as_deref()
                .unwrap_or("<unknown/not-discoverable>")
        );
        if let Some(remote) = &snapshot.remote {
            doctor_println!("forge-remote: {}", remote.remote_name);
            doctor_println!("forge-host: {}", remote.host);
            doctor_println!("forge-path: {}", remote.path_with_namespace);
        } else {
            doctor_println!("forge-remote: <unresolved>");
        }
        let observation = &snapshot.observation;
        degraded |= snapshot.authenticated == Some(false)
            || observation.identity.is_none()
            || observation.error.is_some();
        if let Some(identity) = &observation.identity {
            doctor_println!("provider: {}", identity.provider.label());
            doctor_println!("project-id: {}", identity.project_id);
            doctor_println!("project-path: {}", identity.path_with_namespace);
            doctor_println!("project-url: {}", identity.web_url);
        } else {
            doctor_println!("provider: unavailable");
        }
        doctor_println!("freshness: {}", observation.freshness.label());
        for (capability, state) in &observation.capabilities {
            doctor_println!("capability.{}: {}", capability.label(), state.label());
        }
        doctor_println!("recent-issues: {}", observation.issues.len());
        doctor_println!("open-merge-requests: {}", observation.change_requests.len());
        doctor_println!("recent-pipelines: {}", observation.pipelines.len());
        doctor_println!("issue-boards: {}", snapshot.boards.len());
        if let Some(error) = &observation.error {
            doctor_println!("error: {error}");
        }
    } else if scope == Some("terminal") {
        let capabilities = codex_tui::pty::capabilities();
        doctor_println!("terminal-backend: {}", capabilities.backend);
        doctor_println!("terminal-platform: {}", capabilities.platform);
        doctor_println!("default-program: {}", capabilities.default_program);
        doctor_println!("resize: {}", capabilities.resize);
        doctor_println!("input: {}", capabilities.input);
        doctor_println!("event-queue-capacity: {}", capabilities.bounded_event_queue);
        doctor_println!(
            "default-scrollback-bytes: {}",
            capabilities.default_scrollback_bytes
        );
    } else if scope == Some("presets") {
        let cwd = std::env::current_dir()?;
        let context = git::probe_context(
            codex_tui::domain::ThreadId::new("doctor"),
            cwd.to_string_lossy().into_owned(),
        )
        .await?;
        let Some(repo) = context.repo else {
            doctor_println!("launch-config: unavailable");
            doctor_println!("error: current directory is not inside a Git repository");
            return Ok(codex_tui::headless::EXIT_DEGRADED);
        };
        let repo_root = std::path::Path::new(&repo.primary_root);
        let config_path = repo_root.join(codex_tui::launch::REPO_CONFIG_FILE);
        doctor_println!("launch-config: {}", config_path.display());
        match codex_tui::launch::RepoLaunchConfig::load(repo_root) {
            Ok(config) => {
                doctor_println!("launch-config-version: {}", config.version);
                doctor_println!("launch-presets: {}", config.launches.len());
                for preset in config.launches {
                    doctor_println!(
                        "preset: {} · cwd={:?} · argv={}",
                        preset.name,
                        preset.cwd,
                        preset
                            .argv
                            .iter()
                            .map(|arg| format!("{arg:?}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                }
            }
            Err(error) => {
                degraded = true;
                doctor_println!("launch-config: DEGRADED · {error:#}");
            }
        }
    } else if scope == Some("codex") {
        match app_server::probe_target(target).await {
            Ok(snapshot) => {
                degraded |= !snapshot.status.connected || snapshot.status.error.is_some();
                print_backend_status(&snapshot.status);
                doctor_println!(
                    "codex-home: {}",
                    snapshot.status.codex_home.as_deref().unwrap_or("<unknown>")
                );
                doctor_println!("threads: {}", snapshot.threads.len());

                let mut local = 0usize;
                let mut foreign_windows = 0usize;
                let mut foreign_unix = 0usize;
                let mut stale = 0usize;
                let mut relative = 0usize;
                let mut empty = 0usize;
                let mut local_samples = Vec::new();
                let mut foreign_samples = Vec::new();

                for thread in &snapshot.threads {
                    match classify_cwd(&thread.metadata.cwd) {
                        CwdLocality::LocalDirectory => {
                            local += 1;
                            if local_samples.len() < 5 {
                                local_samples.push(display_cwd(&thread.metadata.cwd).to_string());
                            }
                        }
                        CwdLocality::ForeignWindows => {
                            foreign_windows += 1;
                            if foreign_samples.len() < 5 {
                                foreign_samples.push(display_cwd(&thread.metadata.cwd).to_string());
                            }
                        }
                        CwdLocality::ForeignUnix => {
                            foreign_unix += 1;
                            if foreign_samples.len() < 5 {
                                foreign_samples.push(display_cwd(&thread.metadata.cwd).to_string());
                            }
                        }
                        CwdLocality::NativeMissing => stale += 1,
                        CwdLocality::Relative => relative += 1,
                        CwdLocality::Empty => empty += 1,
                    }
                }

                doctor_println!("threads-local: {local}");
                doctor_println!("threads-foreign-windows: {foreign_windows}");
                doctor_println!("threads-foreign-unix: {foreign_unix}");
                doctor_println!("threads-stale: {stale}");
                doctor_println!("threads-relative: {relative}");
                doctor_println!("threads-empty-cwd: {empty}");
                for cwd in local_samples {
                    doctor_println!("local-cwd: {cwd}");
                }
                for cwd in foreign_samples {
                    doctor_println!("foreign-cwd: {cwd}");
                }
            }
            Err(error) => {
                degraded = true;
                doctor_println!("backend: codex-app-server");
                doctor_println!("connected: false");
                doctor_println!("error: {error:#}");
            }
        }
    } else {
        doctor_println!(
            "hint: run `codex-tui doctor codex`, `doctor git`, `doctor forge`, `doctor store`, `doctor presets`, or `doctor terminal`"
        );
    }
    Ok(if degraded {
        codex_tui::headless::EXIT_DEGRADED
    } else {
        codex_tui::headless::EXIT_OK
    })
}

fn print_backend_status(status: &BackendStatus) {
    doctor_println!("backend: {}", status.source);
    doctor_println!("connected: {}", status.connected);
    doctor_println!(
        "version: {}",
        status.version.as_deref().unwrap_or("unknown")
    );
    doctor_println!(
        "platform: {}",
        status.platform.as_deref().unwrap_or("unknown")
    );
    doctor_println!("capabilities: {}", status.capabilities.join(", "));
    if !status.optional_capabilities_missing.is_empty() {
        doctor_println!(
            "optional-missing: {}",
            status.optional_capabilities_missing.join(", ")
        );
    }
    if let Some(error) = &status.error {
        doctor_println!("error: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_output_escapes_untrusted_terminal_controls_without_changing_source() {
        let hostile = "repo\u{001b}[2J\u{202e}\n中文";
        let line = doctor_line(format_args!("cwd: {hostile}"));
        assert_eq!(line, "cwd: repo\u{fffd}[2J\u{fffd} 中文");
        assert_eq!(hostile, "repo\u{001b}[2J\u{202e}\n中文");
        assert_eq!(
            doctor_line(format_args!("name: {}", "正常中文")),
            "name: 正常中文"
        );
    }

    #[test]
    fn doctor_plain_text_lines_all_pass_through_the_output_guard() {
        let source = include_str!("runtime_doctor.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        let raw_prints = production
            .lines()
            .filter(|line| line.trim_start().starts_with("println!("))
            .count();
        assert_eq!(raw_prints, 1);
        assert!(production.matches("doctor_println!(").count() >= 40);
    }
}
