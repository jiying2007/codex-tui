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

pub(crate) async fn doctor(scope: Option<&str>, target_override: Option<&str>) -> Result<i32> {
    let mut degraded = false;
    let store = SqliteStore::discover()?;
    let config = store.load_config()?;
    let target = ResolvedAppServerTarget::resolve(&config.app_server, target_override)?;

    println!("codex-tui {}", env!("CARGO_PKG_VERSION"));
    println!("config: {}", store.config_path().display());
    println!("state: {}", store.db_path().display());
    println!("mouse: {}", config.ui.mouse);
    println!(
        "language: {} -> {}",
        config.ui.language.as_str(),
        config.ui.language.resolve().as_str()
    );
    println!("notifications: {}", config.notifications.mode.label());
    println!("app-server-target: {}", target.name);
    println!("app-server-endpoint: {}", target.diagnostic_endpoint());
    println!(
        "presentation: {} · background-redraw-min={}ms",
        config.ui.presentation.label(),
        config.ui.presentation.background_redraw_interval_ms()
    );

    match store.load_state() {
        Ok(state) => println!("operator-schemaVersion: {}", state.schema_version),
        Err(error) => {
            degraded = true;
            println!("operator-state: DEGRADED · {error:#}");
        }
    }

    if scope == Some("store") {
        match store.health() {
            Ok(health) => {
                println!("store-backend: sqlite");
                println!("store-schema: {}", health.schema_version);
                println!("integrity: {}", health.integrity);
                println!(
                    "legacy-import: {}",
                    health.legacy_import.as_deref().unwrap_or("unknown")
                );
                match store.load_planning_snapshot() {
                    Ok(snapshot) => {
                        println!("work-cards: {}", snapshot.cards.len());
                        println!("scratch: {}", snapshot.scratch.len());
                        println!("saved-views: {}", snapshot.saved_views.len());
                    }
                    Err(error) => {
                        degraded = true;
                        println!("planning: DEGRADED · {error:#}");
                    }
                }
                match store.load_managed_worktrees() {
                    Ok(worktrees) => println!("managed-worktrees: {}", worktrees.len()),
                    Err(error) => {
                        degraded = true;
                        println!("managed-worktrees: DEGRADED · {error:#}");
                    }
                }
                match store.load_recent_operation_receipts(10) {
                    Ok(receipts) => {
                        println!("recent-operation-receipts: {}", receipts.len());
                        for receipt in receipts {
                            println!(
                                "receipt: {} · {} · {:?}",
                                receipt.operation_id,
                                receipt.plan.kind.label(),
                                receipt.state
                            );
                        }
                    }
                    Err(error) => {
                        degraded = true;
                        println!("operation-receipts: DEGRADED · {error:#}");
                    }
                }
                match store.load_recent_forge_mutation_receipts(10) {
                    Ok(receipts) => {
                        println!("recent-forge-mutation-receipts: {}", receipts.len());
                        for receipt in receipts {
                            println!(
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
                        println!("forge-mutation-receipts: DEGRADED · {error:#}");
                    }
                }
            }
            Err(error) => {
                degraded = true;
                println!("store-backend: sqlite");
                println!("integrity: DEGRADED");
                println!("error: {error:#}");
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
        println!("git-repository: {}", context.is_repository);
        println!("cwd: {}", context.cwd);
        if let Some(repo) = context.repo {
            println!("git-common-dir: {}", repo.git_common_dir);
            println!("repo-root: {}", repo.primary_root);
        }
        if let Some(worktree) = context.worktree {
            println!("worktree: {}", worktree.canonical_path);
        }
        println!(
            "branch: {}",
            context.branch.as_deref().unwrap_or("<detached/none>")
        );
        println!("dirty: {}", context.dirty);
        println!("changed-files: {}", context.changes.len());
        if let Some(error) = context.error {
            println!("error: {error}");
        }
    } else if scope == Some("forge") {
        let cwd = std::env::current_dir()?;
        let snapshot = forge::doctor(cwd.to_string_lossy().into_owned()).await;
        println!(
            "forge-client: {}",
            snapshot.client_name.as_deref().unwrap_or("<unresolved>")
        );
        println!(
            "forge-client-version: {}",
            snapshot
                .client_version
                .as_deref()
                .unwrap_or("<unavailable>")
        );
        println!(
            "forge-authenticated: {}",
            snapshot
                .authenticated
                .map(|value| value.to_string())
                .as_deref()
                .unwrap_or("<unknown>")
        );
        println!(
            "forge-server-version: {}",
            snapshot.server_version.as_deref().unwrap_or("<unknown>")
        );
        println!(
            "forge-server-edition: {}",
            snapshot.server_edition.as_deref().unwrap_or("<unknown>")
        );
        println!(
            "forge-server-tier: {}",
            snapshot
                .server_tier
                .as_deref()
                .unwrap_or("<unknown/not-discoverable>")
        );
        if let Some(remote) = &snapshot.remote {
            println!("forge-remote: {}", remote.remote_name);
            println!("forge-host: {}", remote.host);
            println!("forge-path: {}", remote.path_with_namespace);
        } else {
            println!("forge-remote: <unresolved>");
        }
        let observation = &snapshot.observation;
        degraded |= snapshot.authenticated == Some(false)
            || observation.identity.is_none()
            || observation.error.is_some();
        if let Some(identity) = &observation.identity {
            println!("provider: {}", identity.provider.label());
            println!("project-id: {}", identity.project_id);
            println!("project-path: {}", identity.path_with_namespace);
            println!("project-url: {}", identity.web_url);
        } else {
            println!("provider: unavailable");
        }
        println!("freshness: {}", observation.freshness.label());
        for (capability, state) in &observation.capabilities {
            println!("capability.{}: {}", capability.label(), state.label());
        }
        println!("recent-issues: {}", observation.issues.len());
        println!("open-merge-requests: {}", observation.change_requests.len());
        println!("recent-pipelines: {}", observation.pipelines.len());
        println!("issue-boards: {}", snapshot.boards.len());
        if let Some(error) = &observation.error {
            println!("error: {error}");
        }
    } else if scope == Some("terminal") {
        let capabilities = codex_tui::pty::capabilities();
        println!("terminal-backend: {}", capabilities.backend);
        println!("terminal-platform: {}", capabilities.platform);
        println!("default-program: {}", capabilities.default_program);
        println!("resize: {}", capabilities.resize);
        println!("input: {}", capabilities.input);
        println!("event-queue-capacity: {}", capabilities.bounded_event_queue);
        println!(
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
            println!("launch-config: unavailable");
            println!("error: current directory is not inside a Git repository");
            return Ok(codex_tui::headless::EXIT_DEGRADED);
        };
        let repo_root = std::path::Path::new(&repo.primary_root);
        let config_path = repo_root.join(codex_tui::launch::REPO_CONFIG_FILE);
        println!("launch-config: {}", config_path.display());
        match codex_tui::launch::RepoLaunchConfig::load(repo_root) {
            Ok(config) => {
                println!("launch-config-version: {}", config.version);
                println!("launch-presets: {}", config.launches.len());
                for preset in config.launches {
                    println!(
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
                println!("launch-config: DEGRADED · {error:#}");
            }
        }
    } else if scope == Some("codex") {
        match app_server::probe_target(target).await {
            Ok(snapshot) => {
                degraded |= !snapshot.status.connected || snapshot.status.error.is_some();
                print_backend_status(&snapshot.status);
                println!(
                    "codex-home: {}",
                    snapshot.status.codex_home.as_deref().unwrap_or("<unknown>")
                );
                println!("threads: {}", snapshot.threads.len());

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

                println!("threads-local: {local}");
                println!("threads-foreign-windows: {foreign_windows}");
                println!("threads-foreign-unix: {foreign_unix}");
                println!("threads-stale: {stale}");
                println!("threads-relative: {relative}");
                println!("threads-empty-cwd: {empty}");
                for cwd in local_samples {
                    println!("local-cwd: {cwd}");
                }
                for cwd in foreign_samples {
                    println!("foreign-cwd: {cwd}");
                }
            }
            Err(error) => {
                degraded = true;
                println!("backend: codex-app-server");
                println!("connected: false");
                println!("error: {error:#}");
            }
        }
    } else {
        println!(
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
    println!("backend: {}", status.source);
    println!("connected: {}", status.connected);
    println!(
        "version: {}",
        status.version.as_deref().unwrap_or("unknown")
    );
    println!(
        "platform: {}",
        status.platform.as_deref().unwrap_or("unknown")
    );
    println!("capabilities: {}", status.capabilities.join(", "));
    if !status.optional_capabilities_missing.is_empty() {
        println!(
            "optional-missing: {}",
            status.optional_capabilities_missing.join(", ")
        );
    }
    if let Some(error) = &status.error {
        println!("error: {error}");
    }
}
