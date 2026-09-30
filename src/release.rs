use crate::compat::COMPAT_SCHEMA;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const RELEASE_VERIFY_SCHEMA: &str = "codex-tui/release-verification/v1";
pub const RELEASE_EVIDENCE_SCHEMA: &str = "codex-tui/release-evidence/v1";
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
    pub status: String,
    pub terminal: String,
    pub observed_at: String,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCompatReceipt {
    pub status: String,
    pub report_sha256: String,
    pub observed_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceReceipt {
    pub fixture: String,
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
    pub compatibility: BTreeMap<String, PlatformCompatReceipt>,
    pub terminal_restoration: BTreeMap<String, PlatformTerminalReceipt>,
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

    let stable_criteria_present = options
        .repo_root
        .join("release/v1.0-criteria.json")
        .is_file();
    if !stable_criteria_present {
        blockers.push("release/v1.0-criteria.json is required for release candidates".into());
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
    for platform in ["linux", "macos", "windows"] {
        let compatibility = receipt
            .compatibility
            .get(platform)
            .with_context(|| format!("missing compatibility evidence for {platform}"))?;
        anyhow::ensure!(
            compatibility.status.eq_ignore_ascii_case("ready"),
            "compatibility evidence for {platform} must be READY"
        );
        anyhow::ensure!(
            valid_sha256(&compatibility.report_sha256),
            "compatibility report SHA-256 for {platform} must be 64 hexadecimal characters"
        );
        anyhow::ensure!(
            !compatibility.observed_at.trim().is_empty(),
            "compatibility observation timestamp for {platform} must not be empty"
        );

        let evidence = receipt
            .terminal_restoration
            .get(platform)
            .with_context(|| format!("missing terminal restoration evidence for {platform}"))?;
        anyhow::ensure!(
            evidence.status.eq_ignore_ascii_case("pass"),
            "terminal restoration evidence for {platform} must PASS"
        );
        anyhow::ensure!(
            !evidence.terminal.trim().is_empty(),
            "terminal identifier for {platform} must not be empty"
        );
        anyhow::ensure!(
            !evidence.observed_at.trim().is_empty(),
            "observation timestamp for {platform} must not be empty"
        );
    }

    anyhow::ensure!(
        receipt.performance.fixture == "resident-planning-10k",
        "stable performance fixture must be resident-planning-10k"
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
        receipt.performance.p95_ms <= 50.0,
        "stable performance p95 exceeds 50 ms"
    );
    anyhow::ensure!(
        receipt.performance.p99_ms <= 100.0,
        "stable performance p99 exceeds 100 ms"
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
    if args.first().map(String::as_str) != Some("verify") {
        anyhow::bail!(
            "usage: codex-tui release verify --channel <preview|stable> --tag <tag> --commit <sha> [--evidence <path>] [--publish] [--json]"
        );
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
                commit_sha = Some(args.get(index).context("--commit requires a value")?.clone());
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

    fn repo_with_lock_and_changelog() -> tempfile::TempDir {
        let root = tempdir().expect("tempdir");
        fs::write(root.path().join("Cargo.lock"), "# lock").expect("lock");
        fs::write(
            root.path().join("CHANGELOG.md"),
            format!("# Changelog\n\n## [{}]\n", env!("CARGO_PKG_VERSION")),
        )
        .expect("changelog");
        fs::create_dir_all(root.path().join("release")).expect("release dir");
        fs::write(root.path().join("release/v1.0-criteria.json"), "{}").expect("criteria");
        root
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
    fn current_pre_v1_package_cannot_be_stable() {
        assert!(!stable_version_allowed(env!("CARGO_PKG_VERSION")));
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
    fn stable_evidence_requires_all_platform_terminal_receipts() {
        let root = repo_with_lock_and_changelog();
        let evidence = root.path().join("evidence.json");
        fs::write(
            &evidence,
            serde_json::to_vec_pretty(&ReleaseEvidenceReceipt {
                schema: RELEASE_EVIDENCE_SCHEMA.into(),
                version: "1.0.0".into(),
                commit_sha: sha(),
                canonical_ci_run: 123,
                compat_schema: COMPAT_SCHEMA.into(),
                compatibility: BTreeMap::from([
                    (
                        "linux".into(),
                        PlatformCompatReceipt {
                            status: "ready".into(),
                            report_sha256: "a".repeat(64),
                            observed_at: "2026-09-30T00:00:00Z".into(),
                        },
                    ),
                    (
                        "macos".into(),
                        PlatformCompatReceipt {
                            status: "ready".into(),
                            report_sha256: "b".repeat(64),
                            observed_at: "2026-09-30T00:00:00Z".into(),
                        },
                    ),
                    (
                        "windows".into(),
                        PlatformCompatReceipt {
                            status: "ready".into(),
                            report_sha256: "c".repeat(64),
                            observed_at: "2026-09-30T00:00:00Z".into(),
                        },
                    ),
                ]),
                terminal_restoration: BTreeMap::from([(
                    "linux".into(),
                    PlatformTerminalReceipt {
                        status: "pass".into(),
                        terminal: "xterm".into(),
                        observed_at: "2026-09-30T00:00:00Z".into(),
                        notes: None,
                    },
                )]),
                performance: PerformanceReceipt {
                    fixture: "resident-planning-10k".into(),
                    p95_ms: 40.0,
                    p99_ms: 80.0,
                    source: "retained-runner".into(),
                    observed_at: "2026-09-30T00:00:00Z".into(),
                },
            })
            .expect("evidence json"),
        )
        .expect("evidence");

        let error = validate_evidence(&evidence, "1.0.0", &sha()).expect_err("missing platforms");
        assert!(format!("{error:#}").contains("macos"));
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
    fn sha256_contract_is_exact() {
        assert!(valid_sha256(&"a".repeat(64)));
        assert!(!valid_sha256(&"a".repeat(63)));
        assert!(!valid_sha256(&"z".repeat(64)));
    }
}
