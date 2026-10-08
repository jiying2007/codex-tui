#!/usr/bin/env python3
"""Fail closed when undeployed product-version migration code returns.

This validates existing runtime and release authorities; it does not perform
a migration, create a competing plan, or substitute for real qualification.
"""
from __future__ import annotations

import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
PATHS = {
    "store": "src/store.rs",
    "sqlite": "src/sqlite_store.rs",
    "schema": "src/sqlite_schema.rs",
    "recovery": "src/sqlite_store/recovery.rs",
    "support": "src/support_bundle.rs",
    "compat": "src/compat.rs",
    "failure": "src/hardening.rs",
    "tests": "tests/state_migration_qualification.rs",
    "matrix": "tests/failure_matrix.rs",
    "plan": "scripts/release/validate_v1_4_plan.py",
    "qualification": "scripts/release/create_development_qualification.py",
    "ratchet": "scripts/architecture/check_module_ratchet.py",
    "ci": ".github/workflows/ci.yml",
    "development": ".github/workflows/development-qualification.yml",
    "release": ".github/workflows/release.yml",
    "criteria": "release/v1.4-criteria.json",
    "completion": "release/v1.4-completion.json",
}

def inspect(root: pathlib.Path = ROOT) -> list[str]:
    sources = {}
    failures = []
    for key, relative in PATHS.items():
        try:
            sources[key] = (root / relative).read_text(encoding="utf-8")
        except OSError as error:
            failures.append(f"{relative}: not readable: {error}")
    if failures:
        return failures

    def require(label: str, token: str, explanation: str) -> None:
        if token not in sources[label]:
            failures.append(f"{PATHS[label]}: {explanation}")

    def forbid(label: str, token: str, explanation: str) -> None:
        if token in sources[label]:
            failures.append(f"{PATHS[label]}: {explanation}")

    require("store", "pub fn load_config(&self)", "TOML config loader missing")
    forbid("store", "impl LocalStore for FileStore", "old JSON state backend returned")
    forbid("store", "pub fn state_path(&self)", "obsolete JSON state file authority returned")
    for forbidden in ("migrate_legacy_state", "legacy_backup_path", "legacy_state_path",
                      "LEGACY_IMPORT_KEY", "from_legacy"):
        forbid("sqlite", forbidden, "undeployed JSON migration returned")
    for label in ("support", "compat"):
        for forbidden in ("legacy_import", "legacyImport"):
            forbid(label, forbidden, "obsolete migration diagnostic returned")
    require("sqlite", 'config_store.data_dir().join("state-v2.sqlite3")',
            "current SQLite root changed")
    require("schema", "version == 0 || version == DB_SCHEMA_VERSION",
            "fresh/current-only SQLite admission missing")
    require("schema", "PRAGMA user_version = 4;", "first-install schema v4 missing")
    for obsolete in ("if version == 1", "if version == 2", "if version == 3",
                     "PRAGMA user_version = 1;", "PRAGMA user_version = 2;",
                     "PRAGMA user_version = 3;", "CREATE TABLE metadata"):
        forbid("schema", obsolete, "undeployed SQLite upgrade path returned")
    require("recovery", "version == DB_SCHEMA_VERSION",
            "old-schema recovery rejection missing")
    require("recovery", "validate_operator_envelope", "operator-state safety missing")
    for test in ("first_install_and_reopen_preserve_sqlite_operator_and_planning_state",
                 "undeployed_sqlite_schema_versions_are_rejected_unchanged",
                 "obsolete_recovery_image_does_not_replace_current_state"):
        require("tests", "fn " + test, "first-deployment safety test missing: " + test)
    require("matrix", "fn truncated_operator_envelope_is_refused_without_replacement",
            "live operator-corruption safety test missing")
    require("failure", "operator-state-truncated", "current corruption gate missing")
    forbid("failure", "legacy-state-truncated", "obsolete JSON failure gate returned")
    forbid("tests", "fixtures/v1.0", "v1.0 product upgrade fixture returned")
    require("plan", "completion=load(V14_COMPLETION)", "current v1.4 authority missing")
    forbid("plan", "V13_COMPLETION", "undeployed predecessor became a live gate")
    require("qualification", 'return pathlib.Path("release/v1.4-plan.json")',
            "current plan must be the only qualification default")
    require("qualification", 'return pathlib.Path("release/v1.4-completion.json")',
            "current completion must be the only qualification default")
    for obsolete in ("V12_PLAN_SCHEMA", "V13_PLAN_SCHEMA", "V12_COMPLETION_SCHEMA",
                     "V13_COMPLETION_SCHEMA", "predecessor-development-completion",
                     'return "in-progress"'):
        forbid("qualification", obsolete, "undeployed qualification fallback returned")
    for obsolete in (
        "scripts/release/validate_v1_2_plan.py",
        "scripts/release/validate_v1_2_completion.py",
        "scripts/release/validate_v1_3_plan.py",
        "scripts/release/validate_v1_3_completion.py",
    ):
        if (root / obsolete).is_file():
            failures.append(obsolete + ": historical executable qualifier still active")
    require("ratchet", 'DEFAULT_PLAN = pathlib.Path("release/v1.4-plan.json")',
            "single module LOC authority missing")
    forbid("ci", "Enforce v1.3 stable-predecessor module ratchet",
           "duplicate undeployed numeric LOC ceiling returned")
    for label in ("ci", "development", "release"):
        require(label, "scripts/architecture/check_first_deployment.py",
                "first-deployment qualification guard missing")
        require(label, "scripts/architecture/check_upstream_convergence.py",
                "upstream authority guard missing")
        require(label, "scripts/architecture/check_module_ratchet.py release/v1.4-plan.json",
                "single active module LOC ratchet missing")
    try:
        criteria = json.loads(sources["criteria"])
        completion = json.loads(sources["completion"])
    except (ValueError, TypeError) as error:
        failures.append("release: malformed active completion/criteria: " + str(error))
        return failures
    gates = criteria.get("requiredGates")
    if not isinstance(gates, list):
        failures.append("release/v1.4-criteria.json: required gates missing")
    else:
        current = next((item for item in gates
                        if isinstance(item, dict)
                        and item.get("id") == "state-migration-recovery"), None)
        if not isinstance(current, dict) or "direct first-install SQLite v4" not in str(current.get("evidence", "")):
            failures.append("release/v1.4-criteria.json: fresh-store release qualification lost")
    if completion.get("stableReady") is not False:
        failures.append("release/v1.4-completion.json: development falsely claimed stable PASS")
    if completion.get("publicationAllowed") is not False:
        failures.append("release/v1.4-completion.json: development bypassed publication authorization")
    return failures

def main() -> int:
    root = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT
    problems = inspect(root)
    if problems:
        raise SystemExit("first-deployment contract failed:\n" + "\n".join(problems))
    print("PASS first-deployment contract: unused migrations remain retired; safety gates active")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
