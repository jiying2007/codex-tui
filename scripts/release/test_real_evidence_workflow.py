from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class StableRealEvidenceWorkflowContract(unittest.TestCase):
    def test_hash_only_dispatch_inputs_are_removed(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        dispatch = text.split("  workflow_dispatch:\n", 1)[1].split("\npermissions:", 1)[0]
        self.assertIn("stable_real_evidence_bundle:", dispatch)
        for legacy in (
            "linux_compat_sha256:",
            "linux_compat_observed_at:",
            "linux_terminal_sha256:",
            "linux_terminal_observed_at:",
            "macos_source_sha:",
            "macos_compat_sha256:",
            "macos_compat_observed_at:",
            "macos_terminal_sha256:",
            "macos_terminal_observed_at:",
            "windows_source_sha:",
            "windows_compat_sha256:",
            "windows_compat_observed_at:",
            "windows_terminal_sha256:",
            "windows_terminal_observed_at:",
            "performance_report_sha256:",
            "performance_iterations:",
            "performance_p95_ms:",
            "performance_p99_ms:",
            "performance_source:",
            "performance_observed_at:",
        ):
            self.assertNotIn(legacy, dispatch)

    def test_stable_gate_revalidates_raw_bundle_before_evidence_v6(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        gate = text.split("\n  gate:\n", 1)[1].split("\n  package:\n", 1)[0]
        evidence_step = gate.split(
            "- name: Verify canonical CI and create stable evidence", 1
        )[1].split("- name: Verify release contract", 1)[0]
        verify_index = evidence_step.index("real_evidence_bundle.py verify")
        create_index = evidence_step.index("create_evidence.py")
        self.assertLess(verify_index, create_index)
        self.assertIn("REAL_EVIDENCE_BUNDLE: ${{ inputs.stable_real_evidence_bundle }}", evidence_step)
        self.assertIn("qualification/real-evidence-summary.json", evidence_step)
        self.assertIn("--real-evidence-summary", evidence_step)

    def test_gate_retains_safe_summary_not_raw_payload(self):
        text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        gate = text.split("\n  gate:\n", 1)[1].split("\n  package:\n", 1)[0]
        copy = gate.split("- name: Verify release contract", 1)[1]
        self.assertIn("gate/real-evidence-summary.json", copy)
        self.assertNotIn("gate/stable-real-evidence.bundle", copy)


if __name__ == "__main__":
    unittest.main()
