from pathlib import Path
from lean_patch_support import replace

# First compile exposed two existing fixture callers that also require the target argument.
replace('src/headless/surfaces.rs', 'super::super::build_work_snapshot(true, false)', 'super::super::build_work_snapshot(true, false, None)')
replace('src/headless.rs', 'build_thread_snapshot(true, true)', 'build_thread_snapshot(true, true, None)')
# Compute each priority key once, rather than repeatedly looking up local overlays while sorting.
replace('src/app/attention.rs', 'indices.sort_by_key(|index| {', 'indices.sort_by_cached_key(|index| {')
# The architecture guard must enforce the revised, truthful target scope, not the old marketing label.
replace('scripts/architecture/check_upstream_convergence.py', '"cross-target-codex-registry",', '"selected-target-codex-registry",')
replace('scripts/architecture/check_upstream_convergence.py', '    if manifest.get("upstreamFirst") is not True:', '    if manifest.get("targetMode") != "one-selected-target-per-invocation":\n        failures.append("selected-target scope drifted; federation requires an explicit new decision")\n    if manifest.get("upstreamFirst") is not True:')
p=Path('scripts/release/test_upstream_convergence.py'); s=p.read_text(); marker='    def test_untracked_duplicate_subsystem_file_fails(self):'; assert marker in s; s=s.replace(marker, '''    def test_selected_target_scope_cannot_be_mislabeled_as_federation(self):
        self.manifest["targetMode"] = "federated"
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("selected-target scope drifted", result.stderr)

'''+marker); p.write_text(s)
# Empty templates remain NOT_MEASURED; real observations cannot claim a placeholder source identity.
replace('scripts/release/productivity_report.py', '    seen = set()\n', '    if pairs and data["measurementKind"] == "real":\n        require(data["candidateSourceSha"] != "0" * 40, "real observations require an actual source identity")\n        require(data["baselineId"] != "record-baseline-version", "record the actual baseline version")\n    seen = set()\n')
replace('scripts/release/test_productivity_report.py', 'mutations = [lambda d: d.update(candidateSourceSha="bad"),', 'mutations = [lambda d: d.update(candidateSourceSha="bad"),\n            lambda d: d.update(candidateSourceSha="0" * 40),')
