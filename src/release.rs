use crate::{
    compat::COMPAT_SCHEMA,
    release_benchmark::{PERFORMANCE_FIXTURE, RETAINED_MIN_ITERATIONS},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const RELEASE_VERIFY_SCHEMA: &str = "codex-tui/release-verification/v1";
pub const RELEASE_EVIDENCE_SCHEMA: &str = "codex-tui/release-evidence/v4";
pub const AUTOMATED_QUALIFICATION_SCHEMA: &str = "codex-tui/automated-qualification/v3";
pub const STABLE_CRITERIA_SCHEMA: &str = "codex-tui/stable-criteria/v2";
pub const PRIMARY_STABLE_PLATFORM: &str = "linux";
pub const SECONDARY_PLATFORMS: [&str; 2] = ["macos", "windows"];
pub const EXIT_RELEASE_BLOCKED: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseChannel {
    Preview,
    Stable,
}

impl ReleaseChannel {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "preview" => Some(Self::Preview),
            "stable" => Some(Self::Stable),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformTerminalReceipt {
    pub source_sha: String,
    pub status: String,
    pub terminal: String,
    pub observed_at: String,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCompatReceipt {
    pub source_sha: String,
    pub status: String,
    pub report_sha256: String,
    pub observed_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomatedQualificationGates {
    pub failure_matrix: String,
    pub scale_evidence: String,
    pub soak_structural: String,
    pub ui_contract: String,
    pub state_migration_recovery: String,
    pub support_bundle_redaction: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomatedQualificationArtifacts {
    pub failure_matrix_sha256: String,
    pub scale_evidence_sha256: String,
    pub soak_evidence_sha256: String,
    pub support_manifest_sha256: String,
    pub support_snapshot_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomatedQualificationReceipt {
    pub schema: String,
    pub source_sha: String,
    pub observed_at: String,
    pub gates: AutomatedQualificationGates,
    pub artifacts: AutomatedQualificationArtifacts,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceReceipt {
    pub platform: String,
    pub fixture: String,
    pub source_sha: String,
    pub iterations: usize,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub source: String,
    pub observed_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseEvidenceReceipt {
    pub schema: String,
    pub version: String,
    pub commit_sha: String,
    pub canonical_ci_run: u64,
    pub compat_schema: String,
    pub primary_platform: String,
    pub secondary_platforms: Vec<String>,
    pub compatibility: BTreeMap<String, PlatformCompatReceipt>,
    pub terminal_restoration: BTreeMap<String, PlatformTerminalReceipt>,
    pub automated_qualification: AutomatedQualificationReceipt,
    pub performance: PerformanceReceipt,
}

#[derive(Clone, Debug)]
pub struct ReleaseVerifyOptions {
    pub channel: ReleaseChannel,
    pub tag: String,
    pub commit_sha: String,
    pub evidence_path: Option<PathBuf>,
    pub publish: bool,
    pub repo_root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseVerification {
    pub schema: String,
    pub channel: ReleaseChannel,
    pub tag: String,
    pub version: String,
    pub commit_sha: String,
    pub publish: bool,
    pub valid: bool,
    pub blockers: Vec<String>,
    pub cargo_lock_present: bool,
    pub changelog_version_present: bool,
    pub project_license_present: bool,
    pub stable_criteria_present: bool,
    pub evidence_status: String,
}

pub fn verify(options: &ReleaseVerifyOptions) -> ReleaseVerification {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let mut blockers = Vec::new();

    if !valid_commit_sha(&options.commit_sha) {
        blockers.push("commit SHA must be exactly 40 hexadecimal characters".into());
    }

    match options.channel {
        ReleaseChannel::Preview => {
            if !valid_preview_tag(&options.tag, &version) {
                blockers.push(format!(
                    "preview tag must match v{version}-preview.N with N >= 1"
                ));
            }
        }
        ReleaseChannel::Stable => {
            if !stable_version_allowed(&version) {
                blockers.push(
                    "stable channel requires package major version >= 1; v1.0.0 is the first stable product line"
                        .into(),
                );
            }
            if options.tag != format!("v{version}") {
                blockers.push(format!("stable tag must exactly match v{version}"));
            }
        }
    }

    let cargo_lock_present = options.repo_root.join("Cargo.lock").is_file();
    if !cargo_lock_present {
        blockers.push("Cargo.lock is required for release candidates".into());
    }

    let changelog_version_present = fs::read_to_string(options.repo_root.join("CHANGELOG.md"))
        .ok()
        .is_some_and(|text| text.contains(&format!("## [{version}]")));
    if !changelog_version_present {
        blockers.push(format!("CHANGELOG.md must contain ## [{version}]"));
    }

    let project_license_present = options.repo_root.join("LICENSE").is_file()
        || options.repo_root.join("LICENSE.txt").is_file()
        || options.repo_root.join("LICENSE.md").is_file();
    if options.publish && !project_license_present {
        blockers.push(
            "publishing is blocked until a project-level LICENSE file is explicitly declared"
                .into(),
        );
    }

    let criteria_file = stable_criteria_filename(&version);
    let criteria_path = options.repo_root.join(&criteria_file);
    let stable_criteria_present = criteria_path.is_file();
    if !stable_criteria_present {
        blockers.push(format!(
            "{criteria_file} is required for release candidates"
        ));
    } else if let Err(error) = validate_stable_criteria(&criteria_path, &version) {
        blockers.push(format!("{criteria_file} is invalid: {error:#}"));
    }

    let evidence_status = if options.channel == ReleaseChannel::Stable {
        match options.evidence_path.as_deref() {
            None => {
                blockers.push("stable channel requires --evidence <receipt.json>".into());
                "missing".into()
            }
            Some(path) => match validate_evidence(path, &version, &options.commit_sha) {
                Ok(()) => "verified".into(),
                Err(error) => {
                    blockers.push(format!("stable evidence invalid: {error:#}"));
                    "invalid".into()
                }
            },
        }
    } else if options.evidence_path.is_some() {
        match options
            .evidence_path
            .as_deref()
            .map(|path| validate_evidence(path, &version, &options.commit_sha))
        {
            Some(Ok(())) => "verified-optional".into(),
            Some(Err(error)) => {
                blockers.push(format!("provided preview evidence invalid: {error:#}"));
                "invalid".into()
            }
            None => "not-required".into(),
        }
    } else {
        "not-required".into()
    };

    ReleaseVerification {
        schema: RELEASE_VERIFY_SCHEMA.into(),
        channel: options.channel,
        tag: options.tag.clone(),
        version,
        commit_sha: options.commit_sha.clone(),
        publish: options.publish,
        valid: blockers.is_empty(),
        blockers,
        cargo_lock_present,
        changelog_version_present,
        project_license_present,
        stable_criteria_present,
        evidence_status,
    }
}

pub fn validate_evidence(path: &Path, version: &str, commit_sha: &str) -> Result<()> {
    let bytes = fs::read(path)
        .with_context(|| format!("read release evidence receipt {}", path.display()))?;
    let receipt: ReleaseEvidenceReceipt = serde_json::from_slice(&bytes)
        .with_context(|| format!("decode release evidence receipt {}", path.display()))?;

    anyhow::ensure!(
        receipt.schema == RELEASE_EVIDENCE_SCHEMA,
        "receipt schema must be {RELEASE_EVIDENCE_SCHEMA}"
    );
    anyhow::ensure!(receipt.version == version, "receipt version mismatch");
    anyhow::ensure!(
        receipt.commit_sha.eq_ignore_ascii_case(commit_sha),
        "receipt commit SHA mismatch"
    );
    anyhow::ensure!(
        receipt.canonical_ci_run > 0,
        "receipt canonical CI run must be nonzero"
    );
    anyhow::ensure!(
        receipt.compat_schema == COMPAT_SCHEMA,
        "receipt compatibility schema must be {COMPAT_SCHEMA}"
    );
    anyhow::ensure!(
        receipt.primary_platform == PRIMARY_STABLE_PLATFORM,
        "stable primary platform must be {PRIMARY_STABLE_PLATFORM}"
    );
    anyhow::ensure!(
        receipt.secondary_platforms
            == SECONDARY_PLATFORMS
                .iter()
                .map(|platform| (*platform).to_string())
                .collect::<Vec<_>>(),
        "secondary platform policy mismatch"
    );

    validate_automated_qualification(&receipt.automated_qualification, commit_sha)?;

    validate_platform_evidence(
        PRIMARY_STABLE_PLATFORM,
        &receipt.compatibility,
        &receipt.terminal_restoration,
        true,
        commit_sha,
    )?;
    for platform in SECONDARY_PLATFORMS {
        validate_platform_evidence(
            platform,
            &receipt.compatibility,
            &receipt.terminal_restoration,
            false,
            commit_sha,
        )?;
    }

    anyhow::ensure!(
        receipt.performance.platform == PRIMARY_STABLE_PLATFORM,
        "stable performance evidence must be captured on {PRIMARY_STABLE_PLATFORM}"
    );
    anyhow::ensure!(
        receipt.performance.fixture == PERFORMANCE_FIXTURE,
        "stable performance fixture must be {PERFORMANCE_FIXTURE}"
    );
    anyhow::ensure!(
        receipt.performance.source_sha.eq_ignore_ascii_case(commit_sha),
        "stable performance evidence source SHA must match the release commit"
    );
    anyhow::ensure!(
        receipt.performance.iterations >= RETAINED_MIN_ITERATIONS,
        "stable performance evidence requires at least {RETAINED_MIN_ITERATIONS} iterations"
    );
    anyhow::ensure!(
        receipt.performance.p95_ms.is_finite() && receipt.performance.p95_ms >= 0.0,
        "performance p95 must be a finite nonnegative number"
    );
    anyhow::ensure!(
        receipt.performance.p99_ms.is_finite() && receipt.performance.p99_ms >= 0.0,
        "performance p99 must be a finite nonnegative number"
    );
    anyhow::ensure!(
        !receipt.performance.source.trim().is_empty(),
        "performance source must not be empty"
    );
    anyhow::ensure!(
        !receipt.performance.observed_at.trim().is_empty(),
        "performance observation timestamp must not be empty"
    );

    Ok(())
}

fn validate_automated_qualification(
    receipt: &AutomatedQualificationReceipt,
    commit_sha: &str,
) -> Result<()> {
    anyhow::ensure!(
        receipt.schema == AUTOMATED_QUALIFICATION_SCHEMA,
        "automated qualification schema must be {AUTOMATED_QUALIFICATION_SCHEMA}"
    );
    anyhow::ensure!(
        receipt.source_sha.eq_ignore_ascii_case(commit_sha),
        "automated qualification source SHA mismatch"
    );
    anyhow::ensure!(
        !receipt.observed_at.trim().is_empty(),
        "automated qualification timestamp must not be empty"
    );
    for (name, status) in [
        ("failure-matrix", receipt.gates.failure_matrix.as_str()),
        ("scale-evidence", receipt.gates.scale_evidence.as_str()),
        ("soak-structural", receipt.gates.soak_structural.as_str()),
        ("ui-contract", receipt.gates.ui_contract.as_str()),
        (
            "state-migration-recovery",
            receipt.gates.state_migration_recovery.as_str(),
        ),
        (
            "support-bundle-redaction",
            receipt.gates.support_bundle_redaction.as_str(),
        ),
    ] {
        anyhow::ensure!(
            status.eq_ignore_ascii_case("pass"),
            "automated qualification gate {name} must PASS"
        );
    }
    anyhow::ensure!(
        valid_sha256(&receipt.artifacts.failure_matrix_sha256),
        "Failure Matrix SHA-256 must be 64 hexadecimal characters"
    );
    anyhow::ensure!(
        valid_sha256(&receipt.artifacts.scale_evidence_sha256),
        "scale evidence SHA-256 must be 64 hexadecimal characters"
    );
    anyhow::ensure!(
        valid_sha256(&receipt.artifacts.soak_evidence_sha256),
        "soak evidence SHA-256 must be 64 hexadecimal characters"
    );
    anyhow::ensure!(
        valid_sha256(&receipt.artifacts.support_manifest_sha256),
        "support manifest SHA-256 must be 64 hexadecimal characters"
    );
    anyhow::ensure!(
        valid_sha256(&receipt.artifacts.support_snapshot_sha256),
        "support snapshot SHA-256 must be 64 hexadecimal characters"
    );
    Ok(())
}

fn validate_platform_evidence(
    platform: &str,
    compatibility: &BTreeMap<String, PlatformCompatReceipt>,
    terminal_restoration: &BTreeMap<String, PlatformTerminalReceipt>,
    required: bool,
    commit_sha: &str,
) -> Result<()> {
    let compat = compatibility.get(platform);
    let terminal = terminal_restoration.get(platform);

    if !required && compat.is_none() && terminal.is_none() {
        return Ok(());
    }

    let compat =
        compat.with_context(|| format!("missing compatibility evidence for {platform}"))?;
    anyhow::ensure!(
        compat.status.eq_ignore_ascii_case("ready"),
        "compatibility evidence for {platform} must be READY"
    );
    anyhow::ensure!(
        compat.source_sha.eq_ignore_ascii_case(commit_sha),
        "compatibility evidence source SHA for {platform} must match the release commit"
    );
    anyhow::ensure!(
        valid_sha256(&compat.report_sha256),
        "compatibility report SHA-256 for {platform} must be 64 hexadecimal characters"
    );
    anyhow::ensure!(
        !compat.observed_at.trim().is_empty(),
        "compatibility observation timestamp for {platform} must not be empty"
    );

    let terminal = terminal
        .with_context(|| format!("missing terminal restoration evidence for {platform}"))?;
    anyhow::ensure!(
        terminal.status.eq_ignore_ascii_case("pass"),
        "terminal restoration evidence for {platform} must PASS"
    );
    anyhow::ensure!(
        terminal.source_sha.eq_ignore_ascii_case(commit_sha),
        "terminal restoration evidence source SHA for {platform} must match the release commit"
    );
    anyhow::ensure!(
        !terminal.terminal.trim().is_empty(),
        "terminal identifier for {platform} must not be empty"
    );
    anyhow::ensure!(
        !terminal.observed_at.trim().is_empty(),
        "observation timestamp for {platform} must not be empty"
    );
    Ok(())
}

fn validate_stable_criteria(path: &Path, version: &str) -> Result<()> {
    let bytes =
        fs::read(path).with_context(|| format!("read stable criteria {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("decode stable criteria {}", path.display()))?;
    anyhow::ensure!(
        value.get("schema").and_then(serde_json::Value::as_str) == Some(STABLE_CRITERIA_SCHEMA),
        "criteria schema must be {STABLE_CRITERIA_SCHEMA}"
    );
    anyhow::ensure!(
        value
            .get("stableVersion")
            .and_then(serde_json::Value::as_str)
            == Some(version),
        "criteria stableVersion must be {version}"
    );
    Ok(())
}

pub fn stable_criteria_filename(version: &str) -> String {
    let mut parts = version.split('.');
    let major = parts.next().unwrap_or("0");
    let minor = parts.next().unwrap_or("0");
    format!("release/v{major}.{minor}-criteria.json")
}

pub fn valid_preview_tag(tag: &str, version: &str) -> bool {
    let Some(sequence) = tag.strip_prefix(&format!("v{version}-preview.")) else {
        return false;
    };
    !sequence.is_empty()
        && sequence.chars().all(|ch| ch.is_ascii_digit())
        && sequence.parse::<u64>().is_ok_and(|value| value > 0)
}

pub fn stable_version_allowed(version: &str) -> bool {
    if version.contains('-') || version.contains('+') {
        return false;
    }
    let mut parts = version.split('.');
    let Some(major) = parts.next().and_then(|part| part.parse::<u64>().ok()) else {
        return false;
    };
    major >= 1 && parts.count() == 2
}

pub fn valid_commit_sha(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

pub fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

pub fn print_text(report: &ReleaseVerification) {
    println!("schema: {}", report.schema);
    println!("channel: {:?}", report.channel);
    println!("tag: {}", report.tag);
    println!("version: {}", report.version);
    println!("commit: {}", report.commit_sha);
    println!("publish: {}", report.publish);
    println!("cargo-lock: {}", report.cargo_lock_present);
    println!("changelog-version: {}", report.changelog_version_present);
    println!("project-license: {}", report.project_license_present);
    println!("stable-criteria: {}", report.stable_criteria_present);
    println!("evidence: {}", report.evidence_status);
    println!("valid: {}", report.valid);
    for blocker in &report.blockers {
        println!("BLOCKED: {blocker}");
    }
}

pub fn run_cli(args: &[String]) -> Result<i32> {
    match args.first().map(String::as_str) {
        Some("benchmark") => return crate::release_benchmark::run_cli(&args[1..]),
        Some("scale") => return crate::scale_evidence::run_cli(&args[1..]),
        Some("failure-matrix") => return crate::hardening::run_cli(&args[1..]),
        Some("verify") => {}
        _ => {
            anyhow::bail!("usage: codex-tui release <verify|benchmark|scale|failure-matrix> ...");
        }
    }

    let mut channel = None;
    let mut tag = None;
    let mut commit_sha = None;
    let mut evidence_path = None;
    let mut publish = false;
    let mut json = false;

    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--channel" => {
                index += 1;
                let value = args.get(index).context("--channel requires a value")?;
                channel = ReleaseChannel::parse(value);
                anyhow::ensure!(channel.is_some(), "channel must be preview or stable");
            }
            "--tag" => {
                index += 1;
                tag = Some(args.get(index).context("--tag requires a value")?.clone());
            }
            "--commit" => {
                index += 1;
                commit_sha = Some(
                    args.get(index)
                        .context("--commit requires a value")?
                        .clone(),
                );
            }
            "--evidence" => {
                index += 1;
                evidence_path = Some(PathBuf::from(
                    args.get(index).context("--evidence requires a value")?,
                ));
            }
            "--publish" => publish = true,
            "--json" => json = true,
            other => anyhow::bail!("unknown release verify option: {other}"),
        }
        index += 1;
    }

    let report = verify(&ReleaseVerifyOptions {
        channel: channel.context("--channel is required")?,
        tag: tag.context("--tag is required")?,
        commit_sha: commit_sha.context("--commit is required")?,
        evidence_path,
        publish,
        repo_root: std::env::current_dir().context("resolve repository root")?,
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text(&report);
    }

    Ok(if report.valid {
        0
    } else {
        EXIT_RELEASE_BLOCKED
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sha() -> String {
        "0123456789abcdef0123456789abcdef01234567".into()
    }

    fn automated_receipt() -> AutomatedQualificationReceipt {
        AutomatedQualificationReceipt {
            schema: AUTOMATED_QUALIFICATION_SCHEMA.into(),
            source_sha: sha(),
            observed_at: "2026-10-01T00:00:00Z".into(),
            gates: AutomatedQualificationGates {
                failure_matrix: "pass".into(),
                scale_evidence: "pass".into(),
                soak_structural: "pass".into(),
                ui_contract: "pass".into(),
                state_migration_recovery: "pass".into(),
                support_bundle_redaction: "pass".into(),
            },
            artifacts: AutomatedQualificationArtifacts {
                failure_matrix_sha256: "a".repeat(64),
                scale_evidence_sha256: "b".repeat(64),
                soak_evidence_sha256: "c".repeat(64),
                support_manifest_sha256: "d".repeat(64),
                support_snapshot_sha256: "e".repeat(64),
            },
        }
    }

    fn repo_with_lock_and_changelog() -> tempfile::TempDir {
        let root = tempdir().expect("tempdir");
        fs::write(root.path().join("Cargo.lock"), "# lock").expect("lock");
        fs::write(
            root.path().join("CHANGELOG.md"),
            format!("# Changelog\n\n## [{}]\n", env!("CARGO_PKG_VERSION")),
        )
        .expect("changelog");
        fs::create_dir_all(root.path().join("release")).expect("release dir");
        fs::write(
            root.path()
                .join(stable_criteria_filename(env!("CARGO_PKG_VERSION"))),
            format!(
                r#"{{"schema":"{}","stableVersion":"{}"}}"#,
                STABLE_CRITERIA_SCHEMA,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .expect("criteria");
        root
    }

    #[test]
    fn release_contract_rejects_mismatched_stable_criteria_version() {
        let root = repo_with_lock_and_changelog();
        fs::write(
            root.path()
                .join(stable_criteria_filename(env!("CARGO_PKG_VERSION"))),
            format!(
                r#"{{"schema":"{}","stableVersion":"9.9.9"}}"#,
                STABLE_CRITERIA_SCHEMA
            ),
        )
        .expect("criteria");

        let report = verify(&ReleaseVerifyOptions {
            channel: ReleaseChannel::Preview,
            tag: format!("v{}-preview.1", env!("CARGO_PKG_VERSION")),
            commit_sha: sha(),
            evidence_path: None,
            publish: false,
            repo_root: root.path().to_path_buf(),
        });
        assert!(!report.valid);
        assert!(
            report
                .blockers
                .iter()
                .any(|blocker| blocker.contains("criteria stableVersion"))
        );
    }

    #[test]
    fn preview_contract_accepts_numbered_current_version_tag() {
        assert!(valid_preview_tag(
            &format!("v{}-preview.1", env!("CARGO_PKG_VERSION")),
            env!("CARGO_PKG_VERSION")
        ));
        assert!(!valid_preview_tag(
            &format!("v{}-preview.0", env!("CARGO_PKG_VERSION")),
            env!("CARGO_PKG_VERSION")
        ));
    }

    #[test]
    fn current_package_and_first_stable_line_are_eligible_but_prereleases_are_not() {
        assert!(stable_version_allowed(env!("CARGO_PKG_VERSION")));
        assert!(stable_version_allowed("1.0.0"));
        assert!(stable_version_allowed("2.3.4"));
        assert!(!stable_version_allowed("1.0.0-rc.1"));
    }

    #[test]
    fn candidate_preview_does_not_require_project_license() {
        let root = repo_with_lock_and_changelog();
        let report = verify(&ReleaseVerifyOptions {
            channel: ReleaseChannel::Preview,
            tag: format!("v{}-preview.1", env!("CARGO_PKG_VERSION")),
            commit_sha: sha(),
            evidence_path: None,
            publish: false,
            repo_root: root.path().into(),
        });
        assert!(report.valid, "{:?}", report.blockers);
        assert!(!report.project_license_present);
    }

    #[test]
    fn apache_license_allows_preview_publish_gate_to_advance() {
        let root = repo_with_lock_and_changelog();
        fs::write(root.path().join("LICENSE"), "Apache License\nVersion 2.0").expect("license");
        let report = verify(&ReleaseVerifyOptions {
            channel: ReleaseChannel::Preview,
            tag: format!("v{}-preview.1", env!("CARGO_PKG_VERSION")),
            commit_sha: sha(),
            evidence_path: None,
            publish: true,
            repo_root: root.path().into(),
        });
        assert!(report.valid, "{:?}", report.blockers);
        assert!(report.project_license_present);
    }

    #[test]
    fn publishing_without_explicit_project_license_fails_closed() {
        let root = repo_with_lock_and_changelog();
        let report = verify(&ReleaseVerifyOptions {
            channel: ReleaseChannel::Preview,
            tag: format!("v{}-preview.1", env!("CARGO_PKG_VERSION")),
            commit_sha: sha(),
            evidence_path: None,
            publish: true,
            repo_root: root.path().into(),
        });
        assert!(!report.valid);
        assert!(
            report
                .blockers
                .iter()
                .any(|blocker| blocker.contains("LICENSE"))
        );
    }

    #[test]
    fn stable_evidence_requires_linux_but_not_secondary_real_world_receipts() {
        let root = repo_with_lock_and_changelog();
        let evidence = root.path().join("evidence.json");
        fs::write(
            &evidence,
            serde_json::to_vec_pretty(&ReleaseEvidenceReceipt {
                schema: RELEASE_EVIDENCE_SCHEMA.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                commit_sha: sha(),
                canonical_ci_run: 123,
                compat_schema: COMPAT_SCHEMA.into(),
                primary_platform: PRIMARY_STABLE_PLATFORM.into(),
                secondary_platforms: SECONDARY_PLATFORMS
                    .iter()
                    .map(|platform| (*platform).to_string())
                    .collect(),
                compatibility: BTreeMap::from([(
                    "linux".into(),
                    PlatformCompatReceipt {
                        source_sha: sha(),
                        status: "ready".into(),
                        report_sha256: "a".repeat(64),
                        observed_at: "2026-09-30T00:00:00Z".into(),
                    },
                )]),
                terminal_restoration: BTreeMap::from([(
                    "linux".into(),
                    PlatformTerminalReceipt {
                        source_sha: sha(),
                        status: "pass".into(),
                        terminal: "xterm".into(),
                        observed_at: "2026-09-30T00:00:00Z".into(),
                        notes: None,
                    },
                )]),
                automated_qualification: automated_receipt(),
                performance: PerformanceReceipt {
                    source_sha: sha(),
                    platform: PRIMARY_STABLE_PLATFORM.into(),
                    fixture: PERFORMANCE_FIXTURE.into(),
                    iterations: RETAINED_MIN_ITERATIONS,
                    p95_ms: 40.0,
                    p99_ms: 80.0,
                    source: "retained-linux".into(),
                    observed_at: "2026-09-30T00:00:00Z".into(),
                },
            })
            .expect("evidence json"),
        )
        .expect("evidence");

        validate_evidence(&evidence, env!("CARGO_PKG_VERSION"), &sha())
            .expect("Linux Tier 1 evidence should satisfy stable retained evidence");
    }

    #[test]
    fn optional_secondary_evidence_must_be_complete_when_provided() {
        let root = repo_with_lock_and_changelog();
        let evidence = root.path().join("evidence.json");
        fs::write(
            &evidence,
            serde_json::to_vec_pretty(&ReleaseEvidenceReceipt {
                schema: RELEASE_EVIDENCE_SCHEMA.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                commit_sha: sha(),
                canonical_ci_run: 123,
                compat_schema: COMPAT_SCHEMA.into(),
                primary_platform: PRIMARY_STABLE_PLATFORM.into(),
                secondary_platforms: SECONDARY_PLATFORMS
                    .iter()
                    .map(|platform| (*platform).to_string())
                    .collect(),
                compatibility: BTreeMap::from([
                    (
                        "linux".into(),
                        PlatformCompatReceipt {
                            source_sha: sha(),
                            status: "ready".into(),
                            report_sha256: "a".repeat(64),
                            observed_at: "2026-09-30T00:00:00Z".into(),
                        },
                    ),
                    (
                        "macos".into(),
                        PlatformCompatReceipt {
                            source_sha: sha(),
                            status: "ready".into(),
                            report_sha256: "b".repeat(64),
                            observed_at: "2026-09-30T00:00:00Z".into(),
                        },
                    ),
                ]),
                terminal_restoration: BTreeMap::from([(
                    "linux".into(),
                    PlatformTerminalReceipt {
                        source_sha: sha(),
                        status: "pass".into(),
                        terminal: "xterm".into(),
                        observed_at: "2026-09-30T00:00:00Z".into(),
                        notes: None,
                    },
                )]),
                automated_qualification: automated_receipt(),
                performance: PerformanceReceipt {
                    source_sha: sha(),
                    platform: PRIMARY_STABLE_PLATFORM.into(),
                    fixture: PERFORMANCE_FIXTURE.into(),
                    iterations: RETAINED_MIN_ITERATIONS,
                    p95_ms: 40.0,
                    p99_ms: 80.0,
                    source: "retained-linux".into(),
                    observed_at: "2026-09-30T00:00:00Z".into(),
                },
            })
            .expect("evidence json"),
        )
        .expect("evidence");

        let error = validate_evidence(&evidence, env!("CARGO_PKG_VERSION"), &sha())
            .expect_err("partial macOS evidence must fail");
        assert!(format!("{error:#}").contains("macos"));
    }

    #[test]
    fn stable_evidence_rejects_cross_sha_real_environment_receipts() {
        let root = repo_with_lock_and_changelog();
        let evidence = root.path().join("evidence.json");
        fs::write(
            &evidence,
            serde_json::to_vec_pretty(&ReleaseEvidenceReceipt {
                schema: RELEASE_EVIDENCE_SCHEMA.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                commit_sha: sha(),
                canonical_ci_run: 123,
                compat_schema: COMPAT_SCHEMA.into(),
                primary_platform: PRIMARY_STABLE_PLATFORM.into(),
                secondary_platforms: SECONDARY_PLATFORMS
                    .iter()
                    .map(|platform| (*platform).to_string())
                    .collect(),
                compatibility: BTreeMap::from([(
                    "linux".into(),
                    PlatformCompatReceipt {
                        source_sha: "1123456789abcdef0123456789abcdef01234567".into(),
                        status: "ready".into(),
                        report_sha256: "a".repeat(64),
                        observed_at: "2026-10-01T00:00:00Z".into(),
                    },
                )]),
                terminal_restoration: BTreeMap::from([(
                    "linux".into(),
                    PlatformTerminalReceipt {
                        source_sha: sha(),
                        status: "pass".into(),
                        terminal: "xterm".into(),
                        observed_at: "2026-10-01T00:00:00Z".into(),
                        notes: None,
                    },
                )]),
                automated_qualification: automated_receipt(),
                performance: PerformanceReceipt {
                    source_sha: sha(),
                    platform: PRIMARY_STABLE_PLATFORM.into(),
                    fixture: PERFORMANCE_FIXTURE.into(),
                    iterations: RETAINED_MIN_ITERATIONS,
                    p95_ms: 1.0,
                    p99_ms: 2.0,
                    source: "fixture".into(),
                    observed_at: "2026-10-01T00:00:00Z".into(),
                },
            })
            .expect("evidence json"),
        )
        .expect("evidence");

        let error = validate_evidence(&evidence, env!("CARGO_PKG_VERSION"), &sha())
            .expect_err("cross-SHA compatibility evidence must fail closed");
        assert!(format!("{error:#}").contains("source SHA"));
    }

    #[test]
    fn commit_sha_contract_is_exact() {
        assert!(valid_commit_sha(&sha()));
        assert!(!valid_commit_sha("deadbeef"));
        assert!(!valid_commit_sha(
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
        ));
    }

    #[test]
    fn stable_evidence_rejects_too_few_performance_samples() {
        let root = repo_with_lock_and_changelog();
        let evidence = root.path().join("evidence.json");
        let terminal = |name: &str| PlatformTerminalReceipt {
            source_sha: sha(),
            status: "pass".into(),
            terminal: name.into(),
            observed_at: "2026-09-30T00:00:00Z".into(),
            notes: None,
        };
        let compatibility = |hash: char| PlatformCompatReceipt {
            source_sha: sha(),
            status: "ready".into(),
            report_sha256: hash.to_string().repeat(64),
            observed_at: "2026-09-30T00:00:00Z".into(),
        };
        fs::write(
            &evidence,
            serde_json::to_vec_pretty(&ReleaseEvidenceReceipt {
                schema: RELEASE_EVIDENCE_SCHEMA.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                commit_sha: sha(),
                canonical_ci_run: 123,
                compat_schema: COMPAT_SCHEMA.into(),
                primary_platform: PRIMARY_STABLE_PLATFORM.into(),
                secondary_platforms: SECONDARY_PLATFORMS
                    .iter()
                    .map(|platform| (*platform).to_string())
                    .collect(),
                compatibility: BTreeMap::from([("linux".into(), compatibility('a'))]),
                terminal_restoration: BTreeMap::from([("linux".into(), terminal("xterm"))]),
                automated_qualification: automated_receipt(),
                performance: PerformanceReceipt {
                    source_sha: sha(),
                    platform: PRIMARY_STABLE_PLATFORM.into(),
                    fixture: PERFORMANCE_FIXTURE.into(),
                    iterations: RETAINED_MIN_ITERATIONS - 1,
                    p95_ms: 1.0,
                    p99_ms: 2.0,
                    source: "test".into(),
                    observed_at: "2026-09-30T00:00:00Z".into(),
                },
            })
            .expect("evidence json"),
        )
        .expect("evidence");

        let error = validate_evidence(&evidence, env!("CARGO_PKG_VERSION"), &sha())
            .expect_err("small performance sample must fail");
        assert!(format!("{error:#}").contains("at least"));
    }

    #[test]
    fn sha256_contract_is_exact() {
        assert!(valid_sha256(&"a".repeat(64)));
        assert!(!valid_sha256(&"a".repeat(63)));
        assert!(!valid_sha256(&"z".repeat(64)));
    }
}
