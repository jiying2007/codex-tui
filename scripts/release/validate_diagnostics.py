#!/usr/bin/env python3
"""Validate diagnostic identity, coverage and invariants without granting stable authority."""
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
import re

TIMINGS = ("p50Ms", "p95Ms", "p99Ms", "maxMs")
INTERACTION_FIXTURES = {
    "registry-input-and-render-10k", "board-navigation-and-render-10k",
    "thread-revision-and-render-10k", "planning-ui-snapshot-10k",
    "planning-worker-compute-10k", "planning-ui-commit-10k",
}
SCALE_TIMINGS = ("planningReconcile", "recentProjection", "allHistoryProjection",
                 "searchProjection", "hostLocalProjection")
SCALE_PHASES = {"setup", "threadProjection", "supplementalProjection", "sort",
                "collisionIndex", "workCardIndex", "workCardsCommit", "indexCommit",
                "rebuildTotal", "selectionRefresh", "total"}
SCHEMAS = {"performance": "codex-tui/performance/v2", "render": "codex-tui/render-performance/v1",
           "interaction": "codex-tui/interaction-performance/v1", "scale": "codex-tui/scale-evidence/v4",
           "soak": "codex-tui/soak-evidence/v1"}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def integer(value, label: str, minimum: int = 0, maximum=None) -> int:
    require(type(value) is int and value >= minimum, label + " must be an integer >= " + str(minimum))
    require(maximum is None or value <= maximum, label + " exceeds its bound")
    return value


def number(value, label: str) -> float:
    require(type(value) in (int, float) and math.isfinite(value) and value >= 0,
            label + " must be finite and nonnegative (not a boolean)")
    return value


def distribution(value, label: str) -> None:
    require(isinstance(value, dict), label + " must be a timing object")
    values = [number(value.get(key), label + "." + key) for key in TIMINGS]
    require(values == sorted(values), label + " percentiles must be ordered")


def identity(report: dict, kind: str, commit: str) -> None:
    require(isinstance(report, dict), "report must be an object")
    require(bool(re.fullmatch(r"[0-9a-f]{40}", commit)), "expected commit must be a full lowercase SHA")
    require(report.get("schema") == SCHEMAS[kind], "unexpected " + kind + " schema")
    require(report.get("sourceSha") == commit, kind + " sourceSha mismatch or missing")


def validate_scale(report: dict, commit: str, rows: int = 50_000) -> None:
    identity(report, "scale", commit)
    require(integer(report.get("rows"), "rows", 1) == rows, "scale rows mismatch")
    integer(report.get("warmupIterations"), "warmupIterations", 5)
    integer(report.get("iterations"), "iterations", 50)
    number(report.get("registryConstructMs"), "registryConstructMs")
    for name in SCALE_TIMINGS:
        distribution(report.get(name), name)
    phases = report.get("planningPhases")
    require(isinstance(phases, dict) and set(phases) == SCALE_PHASES, "scale phase coverage mismatch")
    for name, timing in phases.items():
        distribution(timing, "planningPhases." + name)


def validate_performance(report: dict, kind: str, commit: str) -> None:
    identity(report, kind, commit)
    require(report.get("sampleQualified") is True, "samples are not qualified")
    integer(report.get("warmupIterations"), "warmupIterations", 20)
    integer(report.get("iterations"), "iterations", 200)
    require(isinstance(report.get("source"), str) and bool(report["source"].strip()), "source label missing")
    if kind == "performance":
        require(integer(report.get("rows"), "rows", 1) == 10_000, "performance rows mismatch")
        require(report.get("fixture") == "resident-planning-10k", "performance fixture mismatch")
        distribution(report, "performance")
        return
    require(integer(report.get("viewportWidth"), "viewportWidth", 1) == 160 and
            integer(report.get("viewportHeight"), "viewportHeight", 1) == 40, "viewport mismatch")
    expected = INTERACTION_FIXTURES if kind == "interaction" else {"board-render-10k", "thread-render-10k"}
    fixtures = report.get("fixtures")
    require(isinstance(fixtures, list) and len(fixtures) == len(expected), "fixture count mismatch")
    require(all(isinstance(fixture, dict) and isinstance(fixture.get("fixture"), str) for fixture in fixtures),
            "malformed fixture")
    require({fixture["fixture"] for fixture in fixtures} == expected, "fixture coverage or uniqueness mismatch")
    if kind == "interaction":
        require(integer(report.get("rows"), "rows", 1) == 10_000, "interaction rows mismatch")
        require(report.get("authority") == "synthetic-cpu-path-diagnostic-not-terminal-slo", "diagnostic authority mismatch")
        require(integer(report.get("modelInferenceCalls"), "modelInferenceCalls") == 0, "model inference is not allowed")
        for field in ("humanTimeSavings", "modelTokenSavings"):
            require(field in report and report[field] is None, "unsupported savings claim: " + field)
    for fixture in fixtures:
        if kind == "render":
            require(integer(fixture.get("rows"), "fixture.rows", 1) == 10_000, "render rows mismatch")
        distribution(fixture, fixture["fixture"])


def validate_soak(report: dict, commit: str, rows: int = 50_000,
                  minimum_cycles: int = 256, minimum_duration: float = 0) -> None:
    integer(rows, "required rows", 1, 50_000)
    integer(minimum_cycles, "minimum cycles", 1, 10_000)
    number(minimum_duration, "minimum duration")
    require(minimum_duration <= 3600, "minimum duration exceeds limit")
    identity(report, "soak", commit)
    require(integer(report.get("rows"), "rows", 1) == rows, "soak rows mismatch")
    cycles = integer(report.get("cycles"), "cycles", minimum_cycles)
    require(report.get("structuralPass") is True, "structural gate did not pass")
    require(integer(report.get("uiOnlyPlanningReconciles"), "uiOnlyPlanningReconciles") == 0,
            "UI-only churn triggered reconciliation")
    churn = integer(report.get("churnBatches"), "churnBatches")
    require(churn == (cycles + 15) // 16, "churn coverage mismatch")
    require(integer(report.get("planningReconciles"), "planningReconciles") == 1 + churn,
            "planning reconciliation count mismatch")
    require(integer(report.get("actionsApplied"), "actionsApplied") == 1 + 5 * cycles + 2 * churn,
            "applied action count mismatch")
    integer(report.get("effectsEmitted"), "effectsEmitted", cycles)
    for maximum, limit, expected in (("maxConversations", "conversationCacheLimit", 16),
                                     ("maxGitReviews", "gitReviewCacheLimit", 4)):
        require(integer(report.get(limit), limit, 1) == expected, "unexpected cache limit: " + limit)
        integer(report.get(maximum), maximum, 0, expected)
    for field in ("maxWorkCards", "finalWorkCards"):
        require(integer(report.get(field), field, 1, rows) == rows, "work-card coverage mismatch")
    integer(report.get("registrySnapshotPublicationUpperBound"), "registrySnapshotPublicationUpperBound", 1)
    number(report.get("longestCycleStallMs"), "longestCycleStallMs")
    requested = number(report.get("requestedDurationSeconds"), "requestedDurationSeconds")
    require(requested <= 3600 and requested >= minimum_duration, "requested duration below requirement or above limit")
    elapsed = integer(report.get("elapsedMs"), "elapsedMs")
    require(report.get("durationQualified") is (requested > 0 and elapsed >= requested * 1000),
            "duration qualification contradicts elapsed time")
    require(minimum_duration == 0 or report.get("durationQualified") is True, "sustained duration not qualified")
    require(integer(report.get("resourceSampleLimit"), "resourceSampleLimit", 1) == 128,
            "resource sample bound mismatch")
    samples = report.get("resourceSamples")
    require(isinstance(samples, list) and 1 <= len(samples) <= 128, "resource sample coverage mismatch")
    previous_time, previous_cycle, previous_cpu = 0, 0, 0
    for sample in samples:
        require(isinstance(sample, dict), "malformed resource sample")
        previous_time = integer(sample.get("elapsedMs"), "sample.elapsedMs", previous_time, elapsed)
        previous_cycle = integer(sample.get("cycle"), "sample.cycle", previous_cycle, cycles)
        if sample.get("cpuTicks") is not None:
            previous_cpu = integer(sample["cpuTicks"], "sample.cpuTicks", previous_cpu)
        if sample.get("rssKib") is not None:
            integer(sample["rssKib"], "sample.rssKib", 1)
    require(previous_cycle == cycles, "resource samples missing final cycle")
    for field in ("rssStartKib", "rssPeakKib", "rssEndKib"):
        require(field in report, "missing resource field: " + field)
        if report[field] is not None:
            integer(report[field], field, 1)
    if minimum_duration > 0:
        require(len(samples) >= 2 and samples[-1]["elapsedMs"] > samples[0]["elapsedMs"],
                "sustained soak needs temporally separated resource samples")


def validate_support_snapshot(manifest: dict, snapshot_path: Path) -> None:
    entries = manifest.get("files")
    require(isinstance(entries, list), "support manifest has no file entries")
    matches = [entry for entry in entries if isinstance(entry, dict) and entry.get("file") == "snapshot.json"]
    require(len(matches) == 1, "support snapshot manifest entry must be unique")
    raw = snapshot_path.read_bytes()
    checksum = 0xcbf29ce484222325
    for byte in raw:
        checksum = ((checksum ^ byte) * 0x100000001b3) & 0xffffffffffffffff
    entry = matches[0]
    require(entry.get("algorithm") == "fnv1a64" and entry.get("checksum") == "%016x" % checksum,
            "support snapshot checksum mismatch")
    require(integer(entry.get("bytes"), "snapshot bytes") == len(raw), "support snapshot size mismatch")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=sorted(SCHEMAS))
    parser.add_argument("path", type=Path)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--rows", type=int, default=50_000)
    parser.add_argument("--minimum-cycles", type=int, default=256)
    parser.add_argument("--minimum-duration", type=float, default=0)
    args = parser.parse_args()
    report = json.loads(args.path.read_text(encoding="utf-8"))
    try:
        if args.kind == "scale":
            validate_scale(report, args.commit, args.rows)
        elif args.kind == "soak":
            validate_soak(report, args.commit, args.rows, args.minimum_cycles, args.minimum_duration)
        else:
            validate_performance(report, args.kind, args.commit)
    except ValueError as error:
        parser.exit(1, str(error) + "\n")
    print("VERIFIED " + args.kind + " diagnostic; no stable publication authority")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
