from lean_patch_support import write

write('scripts/release/productivity_report.py', r'''#!/usr/bin/env python3
"""Offline paired workflow evidence; no telemetry, network or publication authority."""
import argparse
import json
import math
import re
import statistics
from pathlib import Path

SCHEMA = "codex-tui/productivity-observations/v1"
TASKS = ("find-attention", "open-delivery", "resume-context")
ORDERS = ("baseline-first", "candidate-first")
MAX_BYTES = 1024 * 1024
MAX_PAIRS = 10000


def require(condition, message):
    if not condition:
        raise ValueError(message)


def fields(value, expected, label):
    require(isinstance(value, dict) and set(value) == set(expected), label + ": unexpected or missing fields")


def identifier(value, label):
    require(isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_.-]{1,80}", value) is not None,
            label + ": use a bounded anonymous identifier, not a URL, path or free text")


def sample(value):
    fields(value, ("seconds", "operations", "switches", "errors", "completed"), "sample")
    seconds = value["seconds"]
    require(type(seconds) in (int, float) and math.isfinite(seconds) and 0 < seconds <= 86400,
            "seconds must be finite and within (0, 86400]")
    for name in ("operations", "switches", "errors"):
        require(type(value[name]) is int and 0 <= value[name] <= 1000000, name + ": invalid count")
    require(type(value["completed"]) is bool, "completed must be boolean")


def median(values):
    return statistics.median(values) if values else None


def reduction(rows, metric):
    # A zero baseline cannot establish a relative reduction. Do not divide by zero
    # or silently replace it with an invented unit.
    values = [1 - row["candidate"][metric] / row["baseline"][metric]
              for row in rows if row["baseline"][metric] > 0]
    return {"medianPairedReduction": median(values), "eligiblePairs": len(values)}


def evaluate(data):
    fields(data, ("schema", "measurementKind", "candidateSourceSha", "baselineId", "environmentId", "pairs"), "input")
    require(data["schema"] == SCHEMA, "unsupported schema")
    require(data["measurementKind"] in ("real", "synthetic"), "measurementKind must be real or synthetic")
    require(isinstance(data["candidateSourceSha"], str) and re.fullmatch(r"[0-9a-f]{40}", data["candidateSourceSha"]), "candidateSourceSha must be 40 lowercase hex characters")
    identifier(data["baselineId"], "baselineId")
    identifier(data["environmentId"], "environmentId")
    pairs = data["pairs"]
    require(isinstance(pairs, list) and len(pairs) <= MAX_PAIRS, "pairs must be a bounded array")
    seen = set()
    for pair in pairs:
        fields(pair, ("pairId", "operatorId", "task", "order", "baseline", "candidate"), "pair")
        for name in ("pairId", "operatorId"):
            identifier(pair[name], name)
        require(pair["pairId"] not in seen, "duplicate pairId")
        seen.add(pair["pairId"])
        require(pair["task"] in TASKS and pair["order"] in ORDERS, "unsupported task or execution order")
        sample(pair["baseline"])
        sample(pair["candidate"])
    by_task = {}
    sufficient = len(pairs) >= 12
    for task in TASKS:
        rows = [row for row in pairs if row["task"] == task]
        order_counts = {order: sum(row["order"] == order for row in rows) for order in ORDERS}
        balanced = min(order_counts.values()) >= 2 and abs(order_counts[ORDERS[0]] - order_counts[ORDERS[1]]) <= 1
        sufficient = sufficient and len(rows) >= 4 and balanced
        by_task[task] = {"pairs": len(rows), "orderCounts": order_counts, "balanced": balanced,
                         "time": reduction(rows, "seconds"), "switches": reduction(rows, "switches"),
                         "operations": reduction(rows, "operations")}
    quality = {
        side: {"completed": sum(row[side]["completed"] for row in pairs),
               "errors": sum(row[side]["errors"] for row in pairs)}
        for side in ("baseline", "candidate")
    }
    no_quality_loss = bool(pairs) and all(
        int(row["candidate"]["completed"]) >= int(row["baseline"]["completed"])
        and row["candidate"]["errors"] <= row["baseline"]["errors"] for row in pairs)
    timing = reduction(pairs, "seconds")
    switches = reduction(pairs, "switches")
    metrics = (timing["medianPairedReduction"], switches["medianPairedReduction"])
    target_met = sufficient and no_quality_loss and any(value is not None and value >= 0.30 for value in metrics)
    real_complete = data["measurementKind"] == "real" and sufficient
    return {
        "schema": "codex-tui/productivity-report/v1",
        "candidateSourceSha": data["candidateSourceSha"],
        "baselineId": data["baselineId"], "environmentId": data["environmentId"],
        "authority": "operator-supplied-paired-observations-not-independent-certification",
        "evidenceStatus": "NOT_MEASURED" if not pairs else ("SYNTHETIC_ONLY" if data["measurementKind"] == "synthetic" else ("COMPLETE_SELF_REPORTED" if sufficient else "INSUFFICIENT_DATA")),
        "pairs": len(pairs), "operators": len(set(row["operatorId"] for row in pairs)),
        "measurementComplete": real_complete, "quality": quality, "noObservedQualityLoss": no_quality_loss,
        "time": timing, "switches": switches, "operations": reduction(pairs, "operations"),
        "byTask": by_task, "suggestedTargetsMet": bool(real_complete and target_met),
        "humanSecondsSavedMedian": median([row["baseline"]["seconds"] - row["candidate"]["seconds"] for row in pairs]) if real_complete else None,
        "modelTokenSavings": None, "stableReady": False, "publicationAllowed": False,
    }


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON field")
        result[key] = value
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", nargs="?", type=Path)
    parser.add_argument("--template", action="store_true")
    args = parser.parse_args(argv)
    if args.template:
        if args.input:
            parser.error("--template cannot be combined with an input")
        print(json.dumps({"schema": SCHEMA, "measurementKind": "real", "candidateSourceSha": "0" * 40,
                          "baselineId": "record-baseline-version", "environmentId": "anonymous-environment", "pairs": []}, indent=2))
        return 0
    if not args.input:
        parser.error("input or --template is required")
    try:
        with args.input.open("rb") as stream:
            raw = stream.read(MAX_BYTES + 1)
        require(len(raw) <= MAX_BYTES, "input exceeds size budget")
        report = evaluate(json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object))
    except (OSError, UnicodeError, ValueError, TypeError, RecursionError) as error:
        # A category is sufficient; do not echo private input or filesystem paths.
        print(json.dumps({"error": type(error).__name__, "evidenceStatus": "INVALID", "stableReady": False, "publicationAllowed": False}))
        return 2
    print(json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False))
    return 0 if report["measurementComplete"] else 3


if __name__ == "__main__":
    raise SystemExit(main())
''')
write('scripts/release/test_productivity_report.py', r'''import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("productivity_report.py")
SPEC = importlib.util.spec_from_file_location("productivity_report", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def observation(kind="real"):
    data = {"schema": MODULE.SCHEMA, "measurementKind": kind, "candidateSourceSha": "a" * 40,
            "baselineId": "codex-cli-with-glab", "environmentId": "ssh-linux-01", "pairs": []}
    for task in MODULE.TASKS:
        for index in range(4):
            data["pairs"].append({"pairId": task + "-" + str(index), "operatorId": "p01", "task": task,
                "order": MODULE.ORDERS[index % 2],
                "baseline": {"seconds": 100, "operations": 10, "switches": 6, "errors": 0, "completed": True},
                "candidate": {"seconds": 60, "operations": 6, "switches": 3, "errors": 0, "completed": True}})
    return data


class ProductivityReportTests(unittest.TestCase):
    def test_complete_paired_data_is_an_observation_not_release_authority(self):
        result = MODULE.evaluate(observation())
        self.assertTrue(result["measurementComplete"])
        self.assertTrue(result["suggestedTargetsMet"])
        self.assertAlmostEqual(result["time"]["medianPairedReduction"], 0.4)
        self.assertEqual(result["humanSecondsSavedMedian"], 40)
        self.assertFalse(result["stableReady"])
        self.assertFalse(result["publicationAllowed"])

    def test_synthetic_faster_data_never_grants_real_benefit(self):
        result = MODULE.evaluate(observation("synthetic"))
        self.assertEqual(result["evidenceStatus"], "SYNTHETIC_ONLY")
        self.assertFalse(result["measurementComplete"])
        self.assertFalse(result["suggestedTargetsMet"])
        self.assertIsNone(result["humanSecondsSavedMedian"])

    def test_failures_are_retained_and_block_the_efficiency_target(self):
        data = observation()
        data["pairs"][0]["candidate"]["completed"] = False
        data["pairs"][0]["candidate"]["errors"] = 1
        result = MODULE.evaluate(data)
        self.assertEqual(result["pairs"], 12)
        self.assertEqual(result["quality"]["candidate"]["completed"], 11)
        self.assertFalse(result["suggestedTargetsMet"])

    def test_empty_or_one_order_data_stays_unqualified(self):
        data = observation(); data["pairs"] = []
        self.assertEqual(MODULE.evaluate(data)["evidenceStatus"], "NOT_MEASURED")
        data = observation()
        for pair in data["pairs"]: pair["order"] = "baseline-first"
        self.assertFalse(MODULE.evaluate(data)["measurementComplete"])

    def test_zero_baseline_switches_do_not_invent_a_percentage(self):
        data = observation()
        for pair in data["pairs"]: pair["baseline"]["switches"] = 0
        self.assertIsNone(MODULE.evaluate(data)["switches"]["medianPairedReduction"])

    def test_invalid_types_numbers_duplicates_and_unbounded_input_are_rejected(self):
        mutations = [lambda d: d.update(candidateSourceSha="bad"),
            lambda d: d["pairs"][0]["baseline"].update(seconds=float("nan")),
            lambda d: d["pairs"][0]["baseline"].update(seconds=True),
            lambda d: d["pairs"][0]["candidate"].update(errors=-1),
            lambda d: d["pairs"][0].update(prompt="must not be collected"),
            lambda d: d["pairs"].append(copy.deepcopy(d["pairs"][0])),
            lambda d: d.update(environmentId="https://private.example.com")]
        for mutate in mutations:
            data = observation(); mutate(data)
            with self.assertRaises(ValueError): MODULE.evaluate(data)

    def test_cli_is_offline_bounded_and_keeps_raw_input_private(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input.json"
            path.write_text(json.dumps(observation("synthetic")), encoding="utf-8")
            result = subprocess.run([sys.executable, str(SCRIPT), str(path)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 3)
            self.assertEqual(json.loads(result.stdout)["evidenceStatus"], "SYNTHETIC_ONLY")
            path.write_text('{"secret":"private-data","secret":"duplicate"}', encoding="utf-8")
            result = subprocess.run([sys.executable, str(SCRIPT), str(path)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertNotIn("private-data", result.stdout + result.stderr)
            self.assertNotIn(str(path), result.stdout + result.stderr)
            result = subprocess.run([sys.executable, str(SCRIPT), "--template"], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0)
            self.assertEqual(json.loads(result.stdout)["pairs"], [])


if __name__ == "__main__":
    unittest.main()
''')
