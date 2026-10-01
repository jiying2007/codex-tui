# Registry scale evidence decision

Status: Accepted  
Date: 2026-10-01  
Decision commit: `7fbe7c8048c16a7716d3bb73bcb931d7831e141d`  
Evidence workflow: `scale-evidence` run `36858852899`  
Evidence artifact: `scale-evidence-7fbe7c8048c16a7716d3bb73bcb931d7831e141d`  
Artifact id: `11160671213`  
Artifact digest: `sha256:0b3c57651017ddd98858599c941b8db6e8dc2e152535a37087e523e0467dfb36`

## Decision

Keep the complete hydrated Registry resident in memory for the current 1.1 line.

Do **not** introduce true Registry paging or a SQLite-backed Registry index based on the current scale evidence. The existing recent-100 presentation, cooperative App Server hydration/reconciliation, bounded queues, and conversation/review LRUs remain the preferred architecture.

The next scale optimization, if one becomes necessary, should target planning reconciliation before changing Registry storage.

## Final-main evidence

The measurements below were captured on GitHub Actions Ubuntu 24.04 from the exact main commit above. Each projection used 5 warmup iterations and 50 measured iterations. Peak RSS is from `/usr/bin/time -v` around the complete `codex-tui release scale` process.

| Metric | 10k rows | 50k rows |
| --- | ---: | ---: |
| Registry construction | 11.206 ms | 35.355 ms |
| Planning reconciliation | 17.002 ms | 85.798 ms |
| Recent projection p95 / p99 | 1.742 / 2.134 ms | 15.304 / 16.701 ms |
| All-history projection p95 / p99 | 0.120 / 0.123 ms | 0.576 / 0.589 ms |
| Search projection p95 / p99 | 3.668 / 3.940 ms | 18.120 / 18.477 ms |
| Host-local projection p95 / p99 | 0.168 / 0.177 ms | 0.751 / 0.819 ms |
| Peak RSS | 23,548 KB (~23.0 MiB) | 85,868 KB (~83.9 MiB) |

The retained workflow artifact also contains the full JSON reports and `/usr/bin/time -v` receipts.

## Interpretation

At 50k rows, ordinary Registry projections remain below 20 ms p99 in this retained run and the process peak RSS remains below 100 MiB. Those results do not show a storage-layer scale failure that would justify the complexity, migration surface, and consistency costs of paging or a second Registry index.

Planning reconciliation is the clear scale watch item: 50k takes about 85.8 ms. This is approximately linear with row count and is materially larger than the interactive Registry projections, but it is not evidence that the resident Registry representation itself must move to SQLite.

If real terminal traces later show planning reconciliation becoming user-visible, prefer incremental reconciliation, cheaper projection construction, or moving non-interactive reconciliation work off the foreground path before changing Registry storage.

## Revisit conditions

Re-run this decision rather than changing architecture speculatively when one or more of these conditions becomes true:

- real user histories commonly approach or exceed the 50k fixture;
- retained Registry projection p95/p99 approaches or exceeds the existing 50 ms / 100 ms stable interaction envelope (used here as an advisory comparison, not a new release gate);
- resident Registry memory becomes an operational problem on supported Tier 1 hosts;
- real traces show planning reconciliation blocking input/render responsiveness;
- a future App Server contract provides server-side search/index/paging semantics that materially reduce client complexity.

Until then, adding Registry paging or a SQLite-backed Registry index is considered over-design.

## Reproduction

Build a release binary, then run:

```bash
./target/release/codex-tui release scale \
  --rows 10000 \
  --warmup 5 \
  --iterations 50 \
  --source local \
  --json

./target/release/codex-tui release scale \
  --rows 50000 \
  --warmup 5 \
  --iterations 50 \
  --source local \
  --json
```

For retained Linux peak-RSS evidence, use the repository's `.github/workflows/scale-evidence.yml`.
