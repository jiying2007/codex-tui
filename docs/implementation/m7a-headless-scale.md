# M7a: Headless read-only CLI and scale baselines

Status: implementation slice for v0.7.x / M7a
Parent: #21
Issue: #22

## Scope

M7a establishes the measurement and automation baseline before richer productivity work or an embedded Terminal Drawer.

Delivered in this slice:

- read-only headless thread snapshot: `codex-tui headless threads`;
- read-only headless work snapshot: `codex-tui headless work`;
- stable JSON output with `--json`;
- deterministic local fixtures with `--fake` and `--fixture-10k`;
- explicit headless exit codes;
- compatibility doctor: `codex-tui doctor compat [--json]`;
- deterministic 10,000-thread fixture generation;
- Divan benchmark harness for resident 10k planning filters;
- package line moves to v0.7.0.

## Headless safety contract

Headless M7a is deliberately read-only.

It does not expose:

- prompt submission;
- approvals;
- Git/worktree mutation;
- forge mutation;
- terminal execution;
- batch mutation.

The normal real `threads` and `work` commands explicitly probe Codex because the user requested a Codex/work snapshot. They do not implicitly probe Git or a forge provider.

`--fake` and `--fixture-10k` do not contact Codex, Git, GitLab or GitHub. The 10k fixture also avoids reading the user's LocalStore so benchmark/demo output is deterministic.

JSON output intentionally excludes prompt bodies, drafts, notes, comment bodies, approval payloads and cwd paths.

## Exit codes

- `0`: requested snapshot/doctor completed without degradation;
- `2`: invalid headless usage;
- `3`: requested source or compatibility dependency is degraded/unavailable;
- `1`: unexpected process/runtime failure through the normal Rust error path.

A degraded command still emits a structured/text snapshot before returning code 3.

## Compatibility doctor

`doctor compat` reports:

- OS and architecture;
- SQLite schema/integrity;
- Codex App Server connectivity/version/capability count;
- Git CLI availability;
- `glab` availability;
- `gh` availability.

The forge clients are checked only with their local version commands. No remote authentication or repository API call is performed by the compatibility doctor.

## Scale baseline

The deterministic 10k fixture uses stable IDs:

- first: `thread-scale-00000`
- last: `thread-scale-09999`

Run the benchmark harness locally:

```bash
cargo bench --bench scale
```

Current architecture SLO mapping remains the accepted M7 target:

- resident metadata filter/search p95 <= 50 ms at 10,000 rows;
- normal interaction p95 <= 50 ms, p99 <= 100 ms;
- mature benchmark regressions >20% require explicit review.

Hosted CI compiles/tests the benchmark target but does not use noisy hosted-runner timings as a pass/fail SLO. Performance gating becomes authoritative only after repeated developer-machine/retained-runner baselines exist.

## Deferred

M7a intentionally does not add:

- SQLite FTS;
- GitHub mutations;
- richer SavedView grammar/batch-local productivity actions (M7b);
- PTY/Terminal Drawer (M7c);
- stable/preview release hardening (M7d).

This preserves the decision to measure scale first and add Terminal Drawer only after the M6 core and M7 baselines are established.
