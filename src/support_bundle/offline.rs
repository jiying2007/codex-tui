//! Offline means no child process, network transport, or external capability probe.
use super::*;

pub(super) fn collect() -> SupportSnapshot {
    let store = collect_store();
    let mut components = Vec::new();
    for (id, requirement) in [
        ("codex-app-server", Requirement::Required),
        ("git", Requirement::Required),
        ("terminal-pty", Requirement::Required),
        ("gh", Requirement::Optional),
        ("glab", Requirement::Optional),
    ] {
        components.push(SafeCompatComponent {
            id: id.into(),
            requirement,
            state: ComponentState::Unavailable,
            version: None,
            facts: BTreeMap::from([("probeStatus".into(), "not-probed-offline".into())]),
        });
    }
    let sqlite_ok = store.available && store.integrity.as_deref() == Some("ok");
    components.push(SafeCompatComponent {
        id: "sqlite".into(),
        requirement: Requirement::Required,
        state: if sqlite_ok {
            ComponentState::Available
        } else {
            ComponentState::Unavailable
        },
        version: store
            .schema_version
            .map(|version| format!("schema-{version}")),
        facts: BTreeMap::from([("probeStatus".into(), "local-metadata-only".into())]),
    });
    let mut required_failures = vec![
        "codex-app-server".into(),
        "git".into(),
        "terminal-pty".into(),
    ];
    if !sqlite_ok {
        required_failures.push("sqlite".into());
    }
    let reasons = vec!["external-probes-not-run-offline".into()];
    SupportSnapshot {
        schema: SUPPORT_BUNDLE_SCHEMA,
        collection_mode: "offline",
        real_codex_qualification: false,
        generated_at_unix_ms: now_unix_ms(),
        build: BuildSnapshot {
            product_version: env!("CARGO_PKG_VERSION"),
            source_sha: source_sha(),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        },
        compat: CompatSnapshot {
            readiness: "blocked".into(),
            required_failures,
            required_degraded: vec![],
            optional_unavailable: vec!["gh".into(), "glab".into()],
            components,
        },
        store,
        // Counts/booleans are placeholders, not observations: error_code binds their scope.
        git: GitSnapshot {
            repository: false,
            worktree: false,
            dirty: false,
            changed_files: 0,
            ahead: 0,
            behind: 0,
            error_code: Some("not-probed-offline".into()),
        },
        forge: ForgeSnapshot {
            client_name: None,
            client_version: None,
            authenticated: None,
            server_version: None,
            server_edition: None,
            server_tier: None,
            provider: None,
            freshness: "not-probed".into(),
            capabilities: BTreeMap::new(),
            recent_issues: 0,
            open_change_requests: 0,
            recent_pipelines: 0,
            issue_boards: 0,
            error_code: Some("not-probed-offline".into()),
        },
        degraded_reasons: reasons.clone(),
        errors: ErrorSnapshot {
            window: "bundle-run",
            max_entries: MAX_ERROR_CODES,
            codes: reasons,
        },
    }
}
