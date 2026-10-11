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
                    receipt_sha256: "c".repeat(64),
                    observed_at: "2026-09-30T00:00:00Z".into(),
                    notes: None,
                },
            )]),
            automated_qualification: automated_receipt(),
            performance: PerformanceReceipt {
                source_sha: sha(),
                report_sha256: "d".repeat(64),
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
                    receipt_sha256: "c".repeat(64),
                    observed_at: "2026-09-30T00:00:00Z".into(),
                    notes: None,
                },
            )]),
            automated_qualification: automated_receipt(),
            performance: PerformanceReceipt {
                source_sha: sha(),
                report_sha256: "d".repeat(64),
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
                    receipt_sha256: "c".repeat(64),
                    observed_at: "2026-10-01T00:00:00Z".into(),
                    notes: None,
                },
            )]),
            automated_qualification: automated_receipt(),
            performance: PerformanceReceipt {
                source_sha: sha(),
                report_sha256: "d".repeat(64),
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
    let terminal = || PlatformTerminalReceipt {
        source_sha: sha(),
        status: "pass".into(),
        receipt_sha256: "c".repeat(64),
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
            terminal_restoration: BTreeMap::from([("linux".into(), terminal())]),
            automated_qualification: automated_receipt(),
            performance: PerformanceReceipt {
                source_sha: sha(),
                report_sha256: "d".repeat(64),
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
