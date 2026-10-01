use serde::Serialize;

pub const FAILURE_MATRIX_SCHEMA: &str = "codex-tui/failure-matrix/v1";

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
}

pub const FAILURE_MATRIX: &[FailureCase] = &[
    FailureCase {
        id: "app-server-exit-during-hydration",
        domain: FailureDomain::AppServer,
        injection: "app-server exits while paged registry hydration is active",
        expected: QualificationState::Degraded,
        max_recovery_ms: 6_000,
        writes_allowed: false,
    },
    FailureCase {
        id: "rpc-invalid-json",
        domain: FailureDomain::Rpc,
        injection: "app-server emits a non-JSON protocol line",
        expected: QualificationState::Degraded,
        max_recovery_ms: 1_000,
        writes_allowed: false,
    },
    FailureCase {
        id: "rpc-response-timeout",
        domain: FailureDomain::Rpc,
        injection: "app-server accepts a request but does not answer before the request deadline",
        expected: QualificationState::Degraded,
        max_recovery_ms: 6_000,
        writes_allowed: false,
    },
    FailureCase {
        id: "git-cwd-disappears",
        domain: FailureDomain::Git,
        injection: "repository cwd disappears before or during a probe",
        expected: QualificationState::Degraded,
        max_recovery_ms: 4_000,
        writes_allowed: false,
    },
    FailureCase {
        id: "forge-unauthenticated",
        domain: FailureDomain::Forge,
        injection: "forge client is installed but authentication is unavailable or expired",
        expected: QualificationState::Degraded,
        max_recovery_ms: 6_000,
        writes_allowed: false,
    },
    FailureCase {
        id: "forge-command-timeout",
        domain: FailureDomain::Forge,
        injection: "gh/glab child process does not complete before the command deadline",
        expected: QualificationState::Degraded,
        max_recovery_ms: 12_000,
        writes_allowed: false,
    },
    FailureCase {
        id: "sqlite-busy-or-write-failure",
        domain: FailureDomain::Store,
        injection: "SQLite remains locked or a durable write cannot complete",
        expected: QualificationState::Degraded,
        max_recovery_ms: 3_000,
        writes_allowed: false,
    },
    FailureCase {
        id: "sqlite-corrupt",
        domain: FailureDomain::Store,
        injection: "state-v2.sqlite3 is corrupt or not a SQLite database",
        expected: QualificationState::Blocked,
        max_recovery_ms: 3_000,
        writes_allowed: false,
    },
    FailureCase {
        id: "legacy-state-truncated",
        domain: FailureDomain::Store,
        injection: "state-v1.json is truncated before first SQLite import",
        expected: QualificationState::Blocked,
        max_recovery_ms: 3_000,
        writes_allowed: false,
    },
    FailureCase {
        id: "forward-store-schema",
        domain: FailureDomain::Store,
        injection: "SQLite or operator state schema is newer than this binary understands",
        expected: QualificationState::Blocked,
        max_recovery_ms: 3_000,
        writes_allowed: false,
    },
    FailureCase {
        id: "pty-child-abnormal-exit",
        domain: FailureDomain::Terminal,
        injection: "terminal child cannot start or exits unexpectedly",
        expected: QualificationState::Degraded,
        max_recovery_ms: 3_000,
        writes_allowed: false,
    },
    FailureCase {
        id: "bounded-queue-backpressure",
        domain: FailureDomain::Registry,
        injection: "producer outruns a bounded actor or semantic event queue",
        expected: QualificationState::Degraded,
        max_recovery_ms: 1_000,
        writes_allowed: false,
    },
    FailureCase {
        id: "registry-churn-during-hydration",
        domain: FailureDomain::Registry,
        injection: "thread create/update/archive/delete events overlap paged hydration or reconcile",
        expected: QualificationState::Ready,
        max_recovery_ms: 6_000,
        writes_allowed: false,
    },
];

pub fn failure_case(id: &str) -> Option<&'static FailureCase> {
    FAILURE_MATRIX.iter().find(|case| case.id == id)
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
            assert!(case.max_recovery_ms > 0, "{} needs a bounded deadline", case.id);
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
