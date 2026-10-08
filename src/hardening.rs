use anyhow::{Result, bail};
use serde::Serialize;

pub const FAILURE_MATRIX_SCHEMA: &str = "codex-tui/failure-matrix/v2";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailureDomain {
    AppServer,
    Rpc,
    Git,
    Forge,
    Store,
    Terminal,
    Registry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationState {
    Ready,
    Degraded,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureCase {
    pub id: &'static str,
    pub domain: FailureDomain,
    pub injection: &'static str,
    pub expected: QualificationState,
    pub max_recovery_ms: u64,
    pub writes_allowed: bool,
    pub evidence: &'static [&'static str],
}

pub const FAILURE_MATRIX: &[FailureCase] = &[
    FailureCase {
        id: "app-server-exit-during-hydration",
        domain: FailureDomain::AppServer,
        injection: "app-server exits while paged registry hydration is active",
        expected: QualificationState::Degraded,
        max_recovery_ms: 6_000,
        writes_allowed: false,
        evidence: &[
            "app_server::tests::rpc_eof_is_reported_as_closed_during_request",
            "app_server::tests::startup_registry_hydration_is_pagewise_and_command_priority",
        ],
    },
    FailureCase {
        id: "rpc-invalid-json",
        domain: FailureDomain::Rpc,
        injection: "app-server emits a non-JSON protocol line",
        expected: QualificationState::Degraded,
        max_recovery_ms: 1_000,
        writes_allowed: false,
        evidence: &["app_server::tests::malformed_rpc_wire_line_is_rejected_fail_closed"],
    },
    FailureCase {
        id: "rpc-response-timeout",
        domain: FailureDomain::Rpc,
        injection: "app-server accepts a request but does not answer before the request deadline",
        expected: QualificationState::Degraded,
        max_recovery_ms: 6_000,
        writes_allowed: false,
        evidence: &["app_server::tests::rpc_deadline_fails_closed_without_waiting_forever"],
    },
    FailureCase {
        id: "git-cwd-disappears",
        domain: FailureDomain::Git,
        injection: "repository cwd disappears before or during a probe",
        expected: QualificationState::Degraded,
        max_recovery_ms: 4_000,
        writes_allowed: false,
        evidence: &["missing_git_cwd_fails_closed_within_the_probe_deadline"],
    },
    FailureCase {
        id: "forge-unauthenticated",
        domain: FailureDomain::Forge,
        injection: "forge client is installed but authentication is unavailable or expired",
        expected: QualificationState::Degraded,
        max_recovery_ms: 6_000,
        writes_allowed: false,
        evidence: &["forge::tests::unauthenticated_custom_forge_host_fails_closed"],
    },
    FailureCase {
        id: "forge-command-timeout",
        domain: FailureDomain::Forge,
        injection: "gh/glab child process does not complete before the command deadline",
        expected: QualificationState::Degraded,
        max_recovery_ms: 12_000,
        writes_allowed: false,
        evidence: &["forge::tests::forge_command_deadline_fails_closed_without_hanging"],
    },
    FailureCase {
        id: "sqlite-busy-or-write-failure",
        domain: FailureDomain::Store,
        injection: "SQLite remains locked or a durable write cannot complete",
        expected: QualificationState::Degraded,
        max_recovery_ms: 3_000,
        writes_allowed: false,
        evidence: &["sqlite_busy_write_fails_within_bounded_deadline_without_partial_state"],
    },
    FailureCase {
        id: "sqlite-corrupt",
        domain: FailureDomain::Store,
        injection: "state-v2.sqlite3 is corrupt or not a SQLite database",
        expected: QualificationState::Blocked,
        max_recovery_ms: 3_000,
        writes_allowed: false,
        evidence: &["corrupt_sqlite_is_not_reinitialized_or_overwritten"],
    },
    FailureCase {
        id: "operator-state-truncated",
        domain: FailureDomain::Store,
        injection: "SQLite operator_state envelope contains truncated JSON",
        expected: QualificationState::Blocked,
        max_recovery_ms: 3_000,
        writes_allowed: false,
        evidence: &["truncated_operator_envelope_is_refused_without_replacement"],
    },
    FailureCase {
        id: "forward-store-schema",
        domain: FailureDomain::Store,
        injection: "SQLite or operator state schema is newer than this binary understands",
        expected: QualificationState::Blocked,
        max_recovery_ms: 3_000,
        writes_allowed: false,
        evidence: &[
            "forward_sqlite_schema_is_refused_without_downgrade",
            "forward_operator_state_schema_is_refused_without_replacement",
        ],
    },
    FailureCase {
        id: "pty-child-abnormal-exit",
        domain: FailureDomain::Terminal,
        injection: "terminal child cannot start or exits unexpectedly",
        expected: QualificationState::Degraded,
        max_recovery_ms: 3_000,
        writes_allowed: false,
        evidence: &["invalid_pty_cwd_becomes_an_error_event_without_blocking_the_caller"],
    },
    FailureCase {
        id: "bounded-queue-backpressure",
        domain: FailureDomain::Registry,
        injection: "producer outruns a bounded actor or semantic event queue",
        expected: QualificationState::Degraded,
        max_recovery_ms: 1_000,
        writes_allowed: false,
        evidence: &[
            "app_server::tests::app_server_command_queue_reports_backpressure",
            "app_server::tests::conversation_event_queue_backpressures_without_dropping",
        ],
    },
    FailureCase {
        id: "registry-churn-during-hydration",
        domain: FailureDomain::Registry,
        injection: "thread create/update/archive/delete events overlap paged hydration or reconcile",
        expected: QualificationState::Ready,
        max_recovery_ms: 6_000,
        writes_allowed: false,
        evidence: &[
            "app_server::tests::hydration_merge_preserves_newer_live_state_and_tombstones",
            "app_server::tests::hydration_tombstones_follow_archive_and_reappearance_notifications",
            "app_server::tests::reconcile_finalization_preserves_live_overrides_and_removals",
        ],
    },
];

pub fn failure_case(id: &str) -> Option<&'static FailureCase> {
    FAILURE_MATRIX.iter().find(|case| case.id == id)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FailureMatrixReport {
    schema: &'static str,
    cases: &'static [FailureCase],
}

pub fn run_cli(args: &[String]) -> Result<i32> {
    match args {
        [flag] if flag == "--json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&FailureMatrixReport {
                    schema: FAILURE_MATRIX_SCHEMA,
                    cases: FAILURE_MATRIX,
                })?
            );
            Ok(0)
        }
        [] => {
            println!("schema: {FAILURE_MATRIX_SCHEMA}");
            for case in FAILURE_MATRIX {
                println!(
                    "{}: {:?} · {} evidence item(s)",
                    case.id,
                    case.expected,
                    case.evidence.len()
                );
            }
            Ok(0)
        }
        _ => bail!("usage: codex-tui release failure-matrix [--json]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn failure_matrix_ids_are_unique_and_fail_closed_cases_do_not_allow_writes() {
        let mut ids = BTreeSet::new();
        for case in FAILURE_MATRIX {
            assert!(ids.insert(case.id), "duplicate failure case {}", case.id);
            assert!(
                case.max_recovery_ms > 0,
                "{} needs a bounded deadline",
                case.id
            );
            assert!(
                !case.evidence.is_empty(),
                "{} must name retained executable evidence",
                case.id
            );
            assert!(
                case.evidence
                    .iter()
                    .all(|evidence| !evidence.trim().is_empty()),
                "{} contains an empty evidence identifier",
                case.id
            );
            if case.expected != QualificationState::Ready {
                assert!(
                    !case.writes_allowed,
                    "{} must not write authoritative state while degraded/blocked",
                    case.id
                );
            }
        }
    }

    #[test]
    fn failure_matrix_covers_each_external_authority() {
        for domain in [
            FailureDomain::AppServer,
            FailureDomain::Rpc,
            FailureDomain::Git,
            FailureDomain::Forge,
            FailureDomain::Store,
            FailureDomain::Terminal,
            FailureDomain::Registry,
        ] {
            assert!(
                FAILURE_MATRIX.iter().any(|case| case.domain == domain),
                "missing failure coverage for {domain:?}"
            );
        }
    }
}
