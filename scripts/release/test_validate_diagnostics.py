import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from diagnostic_fixtures import COMMIT, fixture, write_fixtures
from validate_diagnostics import (SCHEMAS, validate_performance, validate_scale, validate_soak,
                                  validate_support_snapshot)


class DiagnosticContracts(unittest.TestCase):
    def validate(self, kind, report):
        if kind == "scale":
            validate_scale(report, COMMIT)
        elif kind == "soak":
            validate_soak(report, COMMIT)
        else:
            validate_performance(report, kind, COMMIT)

    def test_complete_fixtures_are_accepted(self):
        for kind in SCHEMAS:
            with self.subTest(kind=kind):
                self.validate(kind, fixture(kind))

    def test_missing_or_cross_sha_and_wrong_schema_always_fail(self):
        for kind in SCHEMAS:
            for field, value in (("sourceSha", None), ("sourceSha", "f" * 40), ("schema", "old")):
                with self.subTest(kind=kind, field=field, value=value):
                    report = fixture(kind)
                    report[field] = value
                    with self.assertRaises(ValueError):
                        self.validate(kind, report)

    def test_invalid_timings_and_boolean_numbers_are_rejected(self):
        for kind in ("performance", "render", "interaction", "scale"):
            for bad in (True, False, "1", -1, float("nan"), float("inf"), 8):
                with self.subTest(kind=kind, bad=bad):
                    report = fixture(kind)
                    timing = report if kind == "performance" else (report["planningReconcile"] if kind == "scale" else report["fixtures"][0])
                    timing["p50Ms"] = bad
                    with self.assertRaises(ValueError):
                        self.validate(kind, report)

    def test_fixture_identity_count_and_viewport_are_exact(self):
        for kind in ("render", "interaction"):
            for mutation in (lambda r: r["fixtures"].append(copy.deepcopy(r["fixtures"][0])),
                             lambda r: r["fixtures"][0].update(fixture="unrelated"),
                             lambda r: r["fixtures"].__setitem__(0, copy.deepcopy(r["fixtures"][1])),
                             lambda r: r.update(viewportWidth=80),
                             lambda r: r.update(sampleQualified=1),
                             lambda r: r.update(iterations=199),
                             lambda r: r.update(warmupIterations=19)):
                report = fixture(kind)
                mutation(report)
                with self.assertRaises(ValueError):
                    self.validate(kind, report)

    def test_scale_phase_coverage_is_complete_and_typed(self):
        for mutation in (lambda r: r["planningPhases"].pop("sort"),
                         lambda r: r["planningPhases"].update(extra={}),
                         lambda r: r["planningPhases"]["sort"].update(p99Ms=True),
                         lambda r: r.update(rows=10000),
                         lambda r: r.update(iterations=49)):
            report = fixture("scale")
            mutation(report)
            with self.assertRaises(ValueError):
                validate_scale(report, COMMIT)

    def test_interaction_cannot_claim_inference_or_savings(self):
        for field, value in (("authority", "stable"), ("modelInferenceCalls", True),
                             ("modelInferenceCalls", 1), ("humanTimeSavings", 1),
                             ("modelTokenSavings", 1), ("rows", 1)):
            report = fixture("interaction")
            report[field] = value
            with self.assertRaises(ValueError):
                self.validate("interaction", report)

    def test_soak_pass_flag_cannot_override_counter_or_resource_failures(self):
        mutations = [lambda r: r.update(structuralPass=1), lambda r: r.update(cycles=255),
                     lambda r: r.update(uiOnlyPlanningReconciles=1), lambda r: r.update(churnBatches=0),
                     lambda r: r.update(planningReconciles=0), lambda r: r.update(actionsApplied=0),
                     lambda r: r.update(effectsEmitted=0), lambda r: r.update(maxConversations=17),
                     lambda r: r.update(gitReviewCacheLimit=100), lambda r: r.update(maxWorkCards=1),
                     lambda r: r.update(finalWorkCards=1), lambda r: r.update(durationQualified=True),
                     lambda r: r.update(requestedDurationSeconds=float("nan")),
                     lambda r: r.update(resourceSamples=[]), lambda r: r.update(resourceSampleLimit=1000),
                     lambda r: r["resourceSamples"][-1].update(cycle=255),
                     lambda r: r["resourceSamples"][-1].update(elapsedMs=50),
                     lambda r: r["resourceSamples"][-1].update(cpuTicks=0),
                     lambda r: r["resourceSamples"][-1].update(rssKib=-1)]
        for index, mutation in enumerate(mutations):
            with self.subTest(index=index):
                report = fixture("soak")
                mutation(report)
                with self.assertRaises(ValueError):
                    validate_soak(report, COMMIT)

    def test_sustained_minimum_is_external_to_report_claim(self):
        report = fixture("soak")
        with self.assertRaises(ValueError):
            validate_soak(report, COMMIT, minimum_duration=300)
        report.update(requestedDurationSeconds=300, elapsedMs=300000, durationQualified=True)
        report["resourceSamples"][-1]["elapsedMs"] = 300000
        validate_soak(report, COMMIT, minimum_duration=300)
        report.update(requestedDurationSeconds=1, elapsedMs=1000)
        with self.assertRaises(ValueError):
            validate_soak(report, COMMIT, minimum_duration=300)

    def test_support_snapshot_is_bound_to_unique_manifest_entry(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            write_fixtures(root)
            manifest = json.loads((root / "support-manifest.json").read_text())
            path = root / "support-snapshot.json"
            validate_support_snapshot(manifest, path)
            for mutation in (lambda m: m.update(files=[]), lambda m: m["files"].append(m["files"][0]),
                             lambda m: m["files"][0].update(bytes=True),
                             lambda m: m["files"][0].update(checksum="0" * 16)):
                altered = copy.deepcopy(manifest)
                mutation(altered)
                with self.assertRaises(ValueError):
                    validate_support_snapshot(altered, path)
            path.write_bytes(path.read_bytes() + b" ")
            with self.assertRaises(ValueError):
                validate_support_snapshot(manifest, path)

    def test_optimized_python_cannot_disable_validation(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary, "bad.json")
            report = fixture("scale")
            report["sourceSha"] = "f" * 40
            path.write_text(json.dumps(report))
            command = [sys.executable, "-O", str(Path(__file__).with_name("validate_diagnostics.py")),
                       "scale", str(path), "--commit", COMMIT]
            result = subprocess.run(command, capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("sourceSha", result.stderr)

    def test_qualification_builder_rejects_cross_sha_and_mismatched_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "qualified.json"
            command = [sys.executable, str(Path(__file__).with_name("create_automated_qualification.py")),
                       "--commit", COMMIT, "--output", str(output)]
            for flag, filename in (("failure-matrix", "failure-matrix"), ("scale", "scale"),
                                   ("soak", "soak"), ("support-manifest", "support-manifest"),
                                   ("support-snapshot", "support-snapshot")):
                command += ["--" + flag, str(root / (filename + ".json"))]
            write_fixtures(root)
            result = subprocess.run(command, capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(output.is_file())
            output.unlink()
            for target in ("scale", "soak", "support-snapshot"):
                with self.subTest(target=target):
                    write_fixtures(root)
                    path = root / (target + ".json")
                    report = json.loads(path.read_text())
                    if target == "support-snapshot":
                        report["build"]["sourceSha"] = "f" * 40
                    else:
                        report["sourceSha"] = "f" * 40
                    path.write_text(json.dumps(report))
                    result = subprocess.run(command, capture_output=True, text=True, timeout=30)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse(output.exists(), result.stdout)
