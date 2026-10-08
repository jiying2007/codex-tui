use crate::{
    compat::{self, ComponentState, Requirement},
    domain::ThreadId,
    forge::{self, CapabilityState, ForgeProviderKind},
    git,
    sqlite_store::SqliteStore,
    store::LocalStore,
};
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const SUPPORT_BUNDLE_SCHEMA: &str = "codex-tui/support-bundle/v1";
const MAX_ERROR_CODES: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildSnapshot {
    pub product_version: &'static str,
    pub source_sha: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeCompatComponent {
    pub id: String,
    pub requirement: Requirement,
    pub state: ComponentState,
    pub version: Option<String>,
    pub facts: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatSnapshot {
    pub readiness: String,
    pub required_failures: Vec<String>,
    pub required_degraded: Vec<String>,
    pub optional_unavailable: Vec<String>,
    pub components: Vec<SafeCompatComponent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreSnapshot {
    pub available: bool,
    pub schema_version: Option<i64>,
    pub integrity: Option<String>,
    pub operator_pins: usize,
    pub operator_aliases: usize,
    pub operator_drafts: usize,
    pub marked_unread: usize,
    pub work_cards: usize,
    pub scratch: usize,
    pub saved_views: usize,
    pub notes: usize,
    pub bookmarks: usize,
    pub hot_slots: usize,
    pub managed_worktrees: usize,
    pub recent_operation_receipts: usize,
    pub recent_forge_receipts: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSnapshot {
    pub repository: bool,
    pub worktree: bool,
    pub dirty: bool,
    pub changed_files: usize,
    pub ahead: u32,
    pub behind: u32,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeSnapshot {
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    pub authenticated: Option<bool>,
    pub server_version: Option<String>,
    pub server_edition: Option<String>,
    pub server_tier: Option<String>,
    pub provider: Option<ForgeProviderKind>,
    pub freshness: String,
    pub capabilities: BTreeMap<String, String>,
    pub recent_issues: usize,
    pub open_change_requests: usize,
    pub recent_pipelines: usize,
    pub issue_boards: usize,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorSnapshot {
    pub window: &'static str,
    pub max_entries: usize,
    pub codes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportSnapshot {
    pub schema: &'static str,
    pub generated_at_unix_ms: u64,
    pub build: BuildSnapshot,
    pub compat: CompatSnapshot,
    pub store: StoreSnapshot,
    pub git: GitSnapshot,
    pub forge: ForgeSnapshot,
    pub degraded_reasons: Vec<String>,
    pub errors: ErrorSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChecksum {
    pub file: String,
    pub algorithm: &'static str,
    pub checksum: String,
    pub bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleManifest {
    pub schema: &'static str,
    pub generated_at_unix_ms: u64,
    pub files: Vec<FileChecksum>,
    pub privacy: Vec<&'static str>,
}

pub async fn collect() -> SupportSnapshot {
    let cwd = std::env::current_dir().ok();
    let cwd_string = cwd
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();

    let git_future = git::probe_context(ThreadId::new("doctor-bundle"), cwd_string.clone());
    let forge_future = forge::doctor(cwd_string);
    let (compat_report, git_result, forge_snapshot) =
        tokio::join!(compat::probe(), git_future, forge_future);

    let compat = CompatSnapshot {
        readiness: format!("{:?}", compat_report.readiness).to_ascii_lowercase(),
        required_failures: compat_report.required_failures.clone(),
        required_degraded: compat_report.required_degraded.clone(),
        optional_unavailable: compat_report.optional_unavailable.clone(),
        components: compat_report
            .components
            .into_iter()
            .map(|component| SafeCompatComponent {
                id: component.id,
                requirement: component.requirement,
                state: component.state,
                version: component.version,
                facts: safe_component_facts(component.facts),
            })
            .collect(),
    };

    let store = collect_store();
    let git = match git_result {
        Ok(context) => GitSnapshot {
            repository: context.is_repository,
            worktree: context.worktree.is_some(),
            dirty: context.dirty,
            changed_files: context.changes.len(),
            ahead: context.ahead,
            behind: context.behind,
            error_code: context.error.map(|_| "git-probe-degraded".into()),
        },
        Err(_) => GitSnapshot {
            repository: false,
            worktree: false,
            dirty: false,
            changed_files: 0,
            ahead: 0,
            behind: 0,
            error_code: Some("git-probe-unavailable".into()),
        },
    };

    let observation = forge_snapshot.observation;
    let forge_error = observation
        .error
        .as_ref()
        .map(|_| "forge-probe-degraded".to_string());
    let provider = observation
        .identity
        .as_ref()
        .map(|identity| identity.provider);
    let forge = ForgeSnapshot {
        client_name: forge_snapshot.client_name,
        client_version: forge_snapshot.client_version,
        authenticated: forge_snapshot.authenticated,
        server_version: forge_snapshot.server_version,
        server_edition: forge_snapshot.server_edition,
        server_tier: forge_snapshot.server_tier,
        provider,
        freshness: observation.freshness.label().into(),
        capabilities: observation
            .capabilities
            .into_iter()
            .map(|(capability, state)| {
                (
                    capability.label().to_string(),
                    capability_state_label(state).into(),
                )
            })
            .collect(),
        recent_issues: observation.issues.len(),
        open_change_requests: observation.change_requests.len(),
        recent_pipelines: observation.pipelines.len(),
        issue_boards: forge_snapshot.boards.len(),
        error_code: forge_error,
    };

    let mut degraded_reasons = Vec::new();
    degraded_reasons.extend(
        compat
            .required_failures
            .iter()
            .map(|id| format!("required-unavailable:{id}")),
    );
    degraded_reasons.extend(
        compat
            .required_degraded
            .iter()
            .map(|id| format!("required-degraded:{id}")),
    );
    if !store.available {
        degraded_reasons.push("store-unavailable".into());
    }
    if let Some(code) = &git.error_code {
        degraded_reasons.push(code.clone());
    }
    if let Some(code) = &forge.error_code {
        degraded_reasons.push(code.clone());
    }
    degraded_reasons.sort();
    degraded_reasons.dedup();

    let mut codes = degraded_reasons.clone();
    codes.truncate(MAX_ERROR_CODES);

    SupportSnapshot {
        schema: SUPPORT_BUNDLE_SCHEMA,
        generated_at_unix_ms: now_unix_ms(),
        build: BuildSnapshot {
            product_version: env!("CARGO_PKG_VERSION"),
            source_sha: source_sha(),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        },
        compat,
        store,
        git,
        forge,
        degraded_reasons,
        errors: ErrorSnapshot {
            window: "bundle-run",
            max_entries: MAX_ERROR_CODES,
            codes,
        },
    }
}

pub async fn create(output: Option<PathBuf>) -> Result<PathBuf> {
    let snapshot = collect().await;
    let output = output.unwrap_or_else(default_output_path);
    write_bundle(&output, &snapshot)?;
    Ok(output)
}

pub fn write_bundle(output: &Path, snapshot: &SupportSnapshot) -> Result<()> {
    anyhow::ensure!(
        !output.exists(),
        "support bundle destination already exists: {}",
        output.display()
    );
    fs::create_dir_all(output)
        .with_context(|| format!("create support bundle {}", output.display()))?;

    let privacy = concat!(
        "This bundle is intentionally metadata-only. It does not collect environment variables, ",
        "authentication tokens, prompts, transcripts, comment bodies, Git paths, repository names, ",
        "branch names, remote URLs, file names, or raw probe errors.\n"
    );

    let files = [
        ("snapshot.json", serde_json::to_vec_pretty(snapshot)?),
        ("PRIVACY.txt", privacy.as_bytes().to_vec()),
    ];

    let mut checksums = Vec::new();
    for (name, bytes) in files {
        let path = output.join(name);
        fs::write(&path, &bytes).with_context(|| format!("write {}", path.display()))?;
        checksums.push(FileChecksum {
            file: name.into(),
            algorithm: "fnv1a64",
            checksum: format!("{:016x}", fnv1a64(&bytes)),
            bytes: bytes.len(),
        });
    }

    let manifest = BundleManifest {
        schema: SUPPORT_BUNDLE_SCHEMA,
        generated_at_unix_ms: snapshot.generated_at_unix_ms,
        files: checksums,
        privacy: vec![
            "no-environment-variables",
            "no-authentication-tokens",
            "no-prompts-or-transcripts",
            "no-comment-bodies",
            "no-repository-or-file-paths",
            "no-raw-errors",
        ],
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    fs::write(output.join("manifest.json"), manifest_bytes)
        .context("write support bundle manifest")?;
    Ok(())
}

fn collect_store() -> StoreSnapshot {
    let unavailable = || StoreSnapshot {
        available: false,
        schema_version: None,
        integrity: None,
        operator_pins: 0,
        operator_aliases: 0,
        operator_drafts: 0,
        marked_unread: 0,
        work_cards: 0,
        scratch: 0,
        saved_views: 0,
        notes: 0,
        bookmarks: 0,
        hot_slots: 0,
        managed_worktrees: 0,
        recent_operation_receipts: 0,
        recent_forge_receipts: 0,
    };

    let Ok(store) = SqliteStore::discover() else {
        return unavailable();
    };
    let Ok(health) = store.health() else {
        return unavailable();
    };
    let Ok(operator) = store.load_state() else {
        return unavailable();
    };
    let Ok(planning) = store.load_planning_snapshot() else {
        return unavailable();
    };

    StoreSnapshot {
        available: true,
        schema_version: Some(health.schema_version),
        integrity: Some(health.integrity),
        operator_pins: operator.pins.len(),
        operator_aliases: operator.aliases.len(),
        operator_drafts: operator
            .thread_ui
            .values()
            .filter(|state| !state.draft.is_empty())
            .count(),
        marked_unread: operator.marked_unread.len(),
        work_cards: planning.cards.len(),
        scratch: planning.scratch.len(),
        saved_views: planning.saved_views.len(),
        notes: planning.notes.len(),
        bookmarks: planning.bookmarks.len(),
        hot_slots: planning.hot_slots.len(),
        managed_worktrees: store
            .load_managed_worktrees()
            .map(|records| records.len())
            .unwrap_or(0),
        recent_operation_receipts: store
            .load_recent_operation_receipts(10)
            .map(|records| records.len())
            .unwrap_or(0),
        recent_forge_receipts: store
            .load_recent_forge_mutation_receipts(10)
            .map(|records| records.len())
            .unwrap_or(0),
    }
}

fn safe_component_facts(facts: BTreeMap<String, String>) -> BTreeMap<String, String> {
    const SAFE_KEYS: &[&str] = &[
        "integrity",
        "platform",
        "capabilityCount",
        "optionalCapabilitiesMissing",
        "boundedEventQueue",
        "defaultScrollbackBytes",
    ];
    facts
        .into_iter()
        .filter(|(key, _)| SAFE_KEYS.contains(&key.as_str()))
        .collect()
}

const fn capability_state_label(state: CapabilityState) -> &'static str {
    match state {
        CapabilityState::Available => "available",
        CapabilityState::Unavailable => "unavailable",
        CapabilityState::Unknown => "unknown",
    }
}

const fn source_sha() -> &'static str {
    match option_env!("CODEX_TUI_GIT_SHA") {
        Some(value) => value,
        None => match option_env!("GITHUB_SHA") {
            Some(value) => value,
            None => "unknown",
        },
    }
}

fn default_output_path() -> PathBuf {
    PathBuf::from(format!("codex-tui-support-{}", now_unix_ms()))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fixture() -> SupportSnapshot {
        SupportSnapshot {
            schema: SUPPORT_BUNDLE_SCHEMA,
            generated_at_unix_ms: 1,
            build: BuildSnapshot {
                product_version: "1.4.0",
                source_sha: "abc123",
                os: "linux",
                arch: "x86_64",
            },
            compat: CompatSnapshot {
                readiness: "degraded".into(),
                required_failures: vec![],
                required_degraded: vec!["codex-app-server".into()],
                optional_unavailable: vec!["glab".into()],
                components: vec![],
            },
            store: StoreSnapshot {
                available: true,
                schema_version: Some(4),
                integrity: Some("ok".into()),
                operator_pins: 1,
                operator_aliases: 1,
                operator_drafts: 1,
                marked_unread: 1,
                work_cards: 1,
                scratch: 1,
                saved_views: 1,
                notes: 1,
                bookmarks: 1,
                hot_slots: 1,
                managed_worktrees: 1,
                recent_operation_receipts: 1,
                recent_forge_receipts: 1,
            },
            git: GitSnapshot {
                repository: true,
                worktree: true,
                dirty: true,
                changed_files: 2,
                ahead: 1,
                behind: 0,
                error_code: None,
            },
            forge: ForgeSnapshot {
                client_name: Some("gh".into()),
                client_version: Some("gh version".into()),
                authenticated: Some(true),
                server_version: None,
                server_edition: None,
                server_tier: None,
                provider: Some(ForgeProviderKind::GitHub),
                freshness: "fresh".into(),
                capabilities: BTreeMap::new(),
                recent_issues: 2,
                open_change_requests: 1,
                recent_pipelines: 1,
                issue_boards: 0,
                error_code: None,
            },
            degraded_reasons: vec!["required-degraded:codex-app-server".into()],
            errors: ErrorSnapshot {
                window: "bundle-run",
                max_entries: MAX_ERROR_CODES,
                codes: vec!["required-degraded:codex-app-server".into()],
            },
        }
    }

    #[test]
    fn written_bundle_contains_checksums_and_no_user_content_fields() {
        let root = tempdir().expect("tempdir");
        let output = root.path().join("bundle");
        write_bundle(&output, &fixture()).expect("bundle");

        let snapshot = fs::read_to_string(output.join("snapshot.json")).expect("snapshot");
        let manifest = fs::read_to_string(output.join("manifest.json")).expect("manifest");
        assert!(snapshot.contains(SUPPORT_BUNDLE_SCHEMA));
        assert!(manifest.contains("fnv1a64"));
        for forbidden in [
            "prompt",
            "transcript",
            "commentBody",
            "remoteUrl",
            "repositoryPath",
            "branchName",
            "environment",
            "token",
        ] {
            assert!(
                !snapshot.contains(forbidden),
                "support snapshot must not contain forbidden field {forbidden}"
            );
        }
    }

    #[test]
    fn checksums_are_stable() {
        assert_eq!(fnv1a64(b"codex-tui"), 0xf7de92263fb27a93);
    }
}
