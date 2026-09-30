use crate::{app_server, pty, sqlite_store::SqliteStore};
use serde::Serialize;
use std::collections::BTreeMap;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tokio::time::timeout;

pub const COMPAT_SCHEMA: &str = "codex-tui/compat/v2";
pub const RUST_MSRV: &str = "1.88";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Requirement {
    Required,
    Optional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentState {
    Available,
    Degraded,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Readiness {
    Ready,
    Degraded,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatComponent {
    pub id: String,
    pub requirement: Requirement,
    pub state: ComponentState,
    pub version: Option<String>,
    pub detail: String,
    pub facts: BTreeMap<String, String>,
}

impl CompatComponent {
    fn available(
        id: &str,
        requirement: Requirement,
        version: Option<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            requirement,
            state: ComponentState::Available,
            version,
            detail: detail.into(),
            facts: BTreeMap::new(),
        }
    }

    fn unavailable(id: &str, requirement: Requirement, detail: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            requirement,
            state: ComponentState::Unavailable,
            version: None,
            detail: detail.into(),
            facts: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityMatrix {
    pub canonical_ci_os: Vec<String>,
    pub required_components: Vec<String>,
    pub optional_components: Vec<String>,
    pub stable_retained_evidence: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetainedEvidenceRequirement {
    pub id: String,
    pub scope: String,
    pub authority: String,
    pub required_for_stable: bool,
    pub observable_by_doctor: bool,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeRuntimeAuthority {
    pub default_remote_probe: bool,
    pub detailed_command: String,
    pub gitlab_transport: String,
    pub github_transport: String,
    pub server_metadata_authority: String,
    pub tier_policy: String,
    pub capability_authority: String,
    pub native_gitlab_transport_policy: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatReport {
    pub schema: String,
    pub generated_at_unix_ms: u64,
    pub product_version: String,
    pub rust_msrv: String,
    pub os: String,
    pub arch: String,
    pub readiness: Readiness,
    pub required_failures: Vec<String>,
    pub required_degraded: Vec<String>,
    pub optional_unavailable: Vec<String>,
    pub components: Vec<CompatComponent>,
    pub matrix: CompatibilityMatrix,
    pub retained_evidence: Vec<RetainedEvidenceRequirement>,
    pub forge_runtime: ForgeRuntimeAuthority,
}

impl CompatReport {
    pub fn is_blocked(&self) -> bool {
        self.readiness == Readiness::Blocked
    }

    pub fn component(&self, id: &str) -> Option<&CompatComponent> {
        self.components.iter().find(|component| component.id == id)
    }
}

pub async fn probe() -> CompatReport {
    let sqlite = match SqliteStore::discover().and_then(|store| store.health()) {
        Ok(health) if health.integrity.eq_ignore_ascii_case("ok") => {
            let mut component = CompatComponent::available(
                "sqlite",
                Requirement::Required,
                Some(format!("schema-{}", health.schema_version)),
                "SQLite integrity check passed",
            );
            component.facts.insert("integrity".into(), health.integrity);
            component.facts.insert(
                "legacyImport".into(),
                health.legacy_import.unwrap_or_else(|| "unknown".into()),
            );
            component
        }
        Ok(health) => {
            let mut component = CompatComponent::available(
                "sqlite",
                Requirement::Required,
                Some(format!("schema-{}", health.schema_version)),
                format!("SQLite integrity={}", health.integrity),
            );
            component.state = ComponentState::Degraded;
            component.facts.insert("integrity".into(), health.integrity);
            component
        }
        Err(error) => CompatComponent::unavailable(
            "sqlite",
            Requirement::Required,
            format!("SQLite unavailable: {error:#}"),
        ),
    };

    let codex = match app_server::probe(None).await {
        Ok(snapshot) if snapshot.status.connected => {
            let mut component = CompatComponent::available(
                "codex-app-server",
                Requirement::Required,
                snapshot.status.version.clone(),
                format!(
                    "connected; {} capability/capabilities",
                    snapshot.status.capabilities.len()
                ),
            );
            component.facts.insert(
                "platform".into(),
                snapshot.status.platform.unwrap_or_else(|| "unknown".into()),
            );
            component.facts.insert(
                "capabilityCount".into(),
                snapshot.status.capabilities.len().to_string(),
            );
            component.facts.insert(
                "optionalCapabilitiesMissing".into(),
                snapshot.status.optional_capabilities_missing.join(","),
            );
            if let Some(error) = snapshot.status.error {
                component.state = ComponentState::Degraded;
                component.detail = format!("connected with degraded status: {error}");
            }
            component
        }
        Ok(snapshot) => CompatComponent::unavailable(
            "codex-app-server",
            Requirement::Required,
            snapshot
                .status
                .error
                .unwrap_or_else(|| "Codex App Server is not connected".into()),
        ),
        Err(error) => CompatComponent::unavailable(
            "codex-app-server",
            Requirement::Required,
            format!("Codex App Server unavailable: {error:#}"),
        ),
    };

    let git = probe_binary("git", &["--version"], "git", Requirement::Required).await;
    let glab = probe_binary("glab", &["version"], "glab", Requirement::Optional).await;
    let gh = probe_binary("gh", &["--version"], "gh", Requirement::Optional).await;

    let capabilities = pty::capabilities();
    let pty_state = if capabilities.default_program && capabilities.input && capabilities.resize {
        ComponentState::Available
    } else {
        ComponentState::Degraded
    };
    let mut terminal = CompatComponent {
        id: "terminal-pty".into(),
        requirement: Requirement::Required,
        state: pty_state,
        version: Some(capabilities.backend.into()),
        detail: format!(
            "platform={} default_program={} input={} resize={}",
            capabilities.platform,
            capabilities.default_program,
            capabilities.input,
            capabilities.resize
        ),
        facts: BTreeMap::new(),
    };
    terminal.facts.insert(
        "boundedEventQueue".into(),
        capabilities.bounded_event_queue.to_string(),
    );
    terminal.facts.insert(
        "defaultScrollbackBytes".into(),
        capabilities.default_scrollback_bytes.to_string(),
    );

    build_report(vec![sqlite, codex, git, terminal, glab, gh])
}

pub fn build_report(components: Vec<CompatComponent>) -> CompatReport {
    let required_failures = components
        .iter()
        .filter(|component| {
            component.requirement == Requirement::Required
                && component.state == ComponentState::Unavailable
        })
        .map(|component| component.id.clone())
        .collect::<Vec<_>>();
    let required_degraded = components
        .iter()
        .filter(|component| {
            component.requirement == Requirement::Required
                && component.state == ComponentState::Degraded
        })
        .map(|component| component.id.clone())
        .collect::<Vec<_>>();
    let optional_unavailable = components
        .iter()
        .filter(|component| {
            component.requirement == Requirement::Optional
                && component.state != ComponentState::Available
        })
        .map(|component| component.id.clone())
        .collect::<Vec<_>>();

    let readiness = if !required_failures.is_empty() {
        Readiness::Blocked
    } else if !required_degraded.is_empty() {
        Readiness::Degraded
    } else {
        Readiness::Ready
    };

    CompatReport {
        schema: COMPAT_SCHEMA.into(),
        generated_at_unix_ms: now_unix_ms(),
        product_version: env!("CARGO_PKG_VERSION").into(),
        rust_msrv: RUST_MSRV.into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        readiness,
        required_failures,
        required_degraded,
        optional_unavailable,
        components,
        matrix: CompatibilityMatrix {
            canonical_ci_os: vec!["linux".into(), "macos".into(), "windows".into()],
            required_components: vec![
                "sqlite".into(),
                "codex-app-server".into(),
                "git".into(),
                "terminal-pty".into(),
            ],
            optional_components: vec!["glab".into(), "gh".into()],
            stable_retained_evidence: vec!["terminal-restoration".into(), "pty-lifecycle".into()],
        },
        retained_evidence: vec![
            RetainedEvidenceRequirement {
                id: "terminal-restoration".into(),
                scope: "per-platform-real-controlling-tty".into(),
                authority: "interactive-smoke-receipt".into(),
                required_for_stable: true,
                observable_by_doctor: false,
                status: "external-required".into(),
            },
            RetainedEvidenceRequirement {
                id: "pty-lifecycle".into(),
                scope: "canonical-ci-linux-macos-windows".into(),
                authority: "cargo-test-all-targets-all-features".into(),
                required_for_stable: true,
                observable_by_doctor: false,
                status: "repository-ci".into(),
            },
        ],
        forge_runtime: ForgeRuntimeAuthority {
            default_remote_probe: false,
            detailed_command: "codex-tui doctor forge".into(),
            gitlab_transport: "glab".into(),
            github_transport: "gh".into(),
            server_metadata_authority: "runtime-doctor-when-repository-resolves".into(),
            tier_policy: "unknown-unless-discoverable-with-current-non-admin-auth".into(),
            capability_authority: "runtime-capability-probes".into(),
            native_gitlab_transport_policy: "evidence-gated-by-final-implementation-choices".into(),
        },
    }
}

pub fn print_text(report: &CompatReport) {
    println!("schema: {}", report.schema);
    println!("version: {}", report.product_version);
    println!("rust-msrv: {}", report.rust_msrv);
    println!("platform: {}/{}", report.os, report.arch);
    println!("readiness: {:?}", report.readiness);

    for component in &report.components {
        println!(
            "{}: {:?} · {:?} · {}",
            component.id, component.requirement, component.state, component.detail
        );
        if let Some(version) = &component.version {
            println!("  version: {version}");
        }
        for (key, value) in &component.facts {
            println!("  {key}: {value}");
        }
    }

    if !report.required_failures.is_empty() {
        println!("required-failures: {}", report.required_failures.join(","));
    }
    if !report.required_degraded.is_empty() {
        println!("required-degraded: {}", report.required_degraded.join(","));
    }
    if !report.optional_unavailable.is_empty() {
        println!(
            "optional-unavailable: {}",
            report.optional_unavailable.join(",")
        );
    }
    println!(
        "forge-runtime-doctor: {} · remote-probe-default={}",
        report.forge_runtime.detailed_command, report.forge_runtime.default_remote_probe
    );
    for evidence in &report.retained_evidence {
        println!(
            "retained-evidence.{}: {} · required-for-stable={} · doctor-observable={}",
            evidence.id,
            evidence.status,
            evidence.required_for_stable,
            evidence.observable_by_doctor
        );
    }
}

async fn probe_binary(
    name: &str,
    args: &[&str],
    id: &str,
    requirement: Requirement,
) -> CompatComponent {
    let mut command = Command::new(name);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let output = match timeout(Duration::from_secs(3), command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            return CompatComponent::unavailable(
                id,
                requirement,
                format!("{name} unavailable: {error}"),
            );
        }
        Err(_) => {
            return CompatComponent::unavailable(
                id,
                requirement,
                format!("{name} timed out after 3s"),
            );
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let first_line = stdout
        .lines()
        .chain(stderr.lines())
        .find(|line| !line.trim().is_empty())
        .unwrap_or(if output.status.success() {
            "available"
        } else {
            "command failed"
        })
        .trim()
        .chars()
        .take(200)
        .collect::<String>();

    if output.status.success() {
        CompatComponent::available(id, requirement, Some(first_line.clone()), first_line)
    } else {
        CompatComponent::unavailable(id, requirement, first_line)
    }
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

    fn component(id: &str, requirement: Requirement, state: ComponentState) -> CompatComponent {
        CompatComponent {
            id: id.into(),
            requirement,
            state,
            version: None,
            detail: "test".into(),
            facts: BTreeMap::new(),
        }
    }

    #[test]
    fn optional_clients_do_not_block_core_readiness() {
        let report = build_report(vec![
            component("sqlite", Requirement::Required, ComponentState::Available),
            component(
                "codex-app-server",
                Requirement::Required,
                ComponentState::Available,
            ),
            component("git", Requirement::Required, ComponentState::Available),
            component(
                "terminal-pty",
                Requirement::Required,
                ComponentState::Available,
            ),
            component("glab", Requirement::Optional, ComponentState::Unavailable),
            component("gh", Requirement::Optional, ComponentState::Unavailable),
        ]);
        assert_eq!(report.readiness, Readiness::Ready);
        assert_eq!(report.optional_unavailable, vec!["glab", "gh"]);
        assert!(!report.is_blocked());
    }

    #[test]
    fn missing_required_component_blocks_readiness() {
        let report = build_report(vec![
            component("sqlite", Requirement::Required, ComponentState::Available),
            component("git", Requirement::Required, ComponentState::Unavailable),
        ]);
        assert_eq!(report.readiness, Readiness::Blocked);
        assert_eq!(report.required_failures, vec!["git"]);
        assert!(report.is_blocked());
    }

    #[test]
    fn degraded_required_component_is_distinct_from_blocked() {
        let report = build_report(vec![component(
            "sqlite",
            Requirement::Required,
            ComponentState::Degraded,
        )]);
        assert_eq!(report.readiness, Readiness::Degraded);
        assert!(report.required_failures.is_empty());
        assert_eq!(report.required_degraded, vec!["sqlite"]);
    }

    #[test]
    fn schema_matrix_and_msrv_are_locked() {
        let report = build_report(vec![]);
        assert_eq!(report.schema, COMPAT_SCHEMA);
        assert_eq!(report.rust_msrv, RUST_MSRV);
        assert_eq!(
            report.matrix.canonical_ci_os,
            vec!["linux", "macos", "windows"]
        );
        assert!(
            report
                .matrix
                .required_components
                .contains(&"terminal-pty".into())
        );

        let cargo_toml = include_str!("../Cargo.toml");
        assert!(cargo_toml.contains(&format!("rust-version = \"{RUST_MSRV}\"")));

        let ci = include_str!("../.github/workflows/ci.yml");
        for runner in ["ubuntu-latest", "macos-latest", "windows-latest"] {
            assert!(
                ci.contains(runner),
                "compat matrix drifted from CI: {runner}"
            );
        }
    }

    #[test]
    fn json_contract_exposes_release_evidence_and_forge_authority() {
        let report = build_report(vec![]);
        let json = serde_json::to_value(report).expect("serialize compat report");
        assert_eq!(json["schema"], COMPAT_SCHEMA);
        assert_eq!(
            json["forgeRuntime"]["defaultRemoteProbe"],
            serde_json::Value::Bool(false)
        );
        assert_eq!(
            json["retainedEvidence"][0]["observableByDoctor"],
            serde_json::Value::Bool(false)
        );
    }
}
