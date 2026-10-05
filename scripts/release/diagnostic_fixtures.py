#!/usr/bin/env python3
"""Synthetic validator fixtures only. These are never real release evidence."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

from _compat import write_text_lf
from validate_diagnostics import INTERACTION_FIXTURES, SCALE_PHASES, SCALE_TIMINGS, SCHEMAS

COMMIT = "0123456789abcdef0123456789abcdef01234567"
PRIVACY = ["no-environment-variables", "no-authentication-tokens", "no-prompts-or-transcripts",
           "no-comment-bodies", "no-repository-or-file-paths", "no-raw-errors"]


def timing():
    return {"p50Ms": 1.0, "p95Ms": 2.0, "p99Ms": 3.0, "maxMs": 4.0}


def fixture(kind, commit=COMMIT):
    report = {"schema": SCHEMAS[kind], "sourceSha": commit}
    if kind == "soak":
        report.update(rows=50000, cycles=256, actionsApplied=1313, effectsEmitted=256,
                      churnBatches=16, planningReconciles=17, uiOnlyPlanningReconciles=0,
                      maxConversations=16, conversationCacheLimit=16, maxGitReviews=4,
                      gitReviewCacheLimit=4, maxWorkCards=50000, finalWorkCards=50000,
                      registrySnapshotPublicationUpperBound=26, longestCycleStallMs=4.0,
                      structuralPass=True, requestedDurationSeconds=0.0, elapsedMs=4000,
                      durationQualified=False, resourceSampleLimit=128,
                      rssStartKib=100, rssPeakKib=110, rssEndKib=105,
                      resourceSamples=[{"cycle": 1, "elapsedMs": 100, "rssKib": 110, "cpuTicks": 1},
                                       {"cycle": 256, "elapsedMs": 4000, "rssKib": 105, "cpuTicks": 3}])
    elif kind == "scale":
        report.update(rows=50000, warmupIterations=5, iterations=50, registryConstructMs=1.0,
                      planningPhases={phase: timing() for phase in sorted(SCALE_PHASES)})
        report.update({name: timing() for name in SCALE_TIMINGS})
    else:
        report.update(warmupIterations=20, iterations=200, sampleQualified=True,
                      source="synthetic-validator-fixture-not-release-evidence")
        if kind == "performance":
            report.update(rows=10000, fixture="resident-planning-10k", **timing())
        else:
            report.update(viewportWidth=160, viewportHeight=40)
            names = INTERACTION_FIXTURES if kind == "interaction" else {"board-render-10k", "thread-render-10k"}
            report["fixtures"] = [dict(fixture=name, **timing()) for name in sorted(names)]
            if kind == "render":
                for entry in report["fixtures"]:
                    entry["rows"] = 10000
            else:
                report.update(rows=10000, authority="synthetic-cpu-path-diagnostic-not-terminal-slo",
                              modelInferenceCalls=0, humanTimeSavings=None, modelTokenSavings=None)
    return report


def snapshot_entry(raw):
    checksum = 0xcbf29ce484222325
    for byte in raw:
        checksum = ((checksum ^ byte) * 0x100000001b3) & 0xffffffffffffffff
    return {"file": "snapshot.json", "algorithm": "fnv1a64", "checksum": "%016x" % checksum,
            "bytes": len(raw)}


def write_fixtures(output: Path):
    output.mkdir(parents=True, exist_ok=True)
    for kind in SCHEMAS:
        write_text_lf(output / (kind + ".json"), json.dumps(fixture(kind)) + "\n")
    snapshot = {"schema": "codex-tui/support-bundle/v1",
                "build": {"productVersion": "1.4.0", "sourceSha": COMMIT}}
    path = output / "support-snapshot.json"
    write_text_lf(path, json.dumps(snapshot) + "\n")
    manifest = {"schema": "codex-tui/support-bundle/v1", "privacy": PRIVACY,
                "files": [snapshot_entry(path.read_bytes())]}
    write_text_lf(output / "support-manifest.json", json.dumps(manifest) + "\n")
    matrix = {"schema": "codex-tui/failure-matrix/v2",
              "cases": [{"id": "fixture", "evidence": ["fixture::test"]}]}
    write_text_lf(output / "failure-matrix.json", json.dumps(matrix) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    write_fixtures(parser.parse_args().output)
