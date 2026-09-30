# M7b1: richer SavedView query language

Status: implementation slice under #24 / #25

M7b1 extends the resident, in-memory SavedView filter without adding SQLite FTS or new external probes.

## Grammar

Existing filters remain whitespace-separated AND expressions.

Additional syntax:

- quoted values: `project:"audio pipeline"`
- unary negation: `-stage:done`
- combined example: `project:kws branch:research forge:gitlab mr:open -attention:pipeline-failed`

Quotes may appear inside a structured token, so `project:"audio pipeline"` remains one term. A backslash inside a quoted value escapes the following character. Unterminated quotes or escapes fail closed.

## Structured fields

| Field | Semantics |
|---|---|
| `status:needs-you` | unsnoozed card with attention |
| `status:snoozed` | currently snoozed |
| `status:active` | not Done and not snoozed |
| `stage:<stage>` | Inbox / Ready / Working / Review / Done |
| `project:<text>` | alias of workspace substring matching |
| `workspace:<text>` | workspace substring matching |
| `branch:<text>` | already-observed Git branch substring |
| `forge:gitlab` / `forge:github` | already-observed forge provider |
| `mr:<state>` / `cr:<state>` | open/opened, closed, merged, draft, none |
| `goal:<text>` | Goal objective/status |
| `tag:<tag>` | exact local tag, case-insensitive |
| `attention:<label>` | exact attention label; also any/none |
| `source:scratch` | ScratchWork anchor |
| `source:thread` / `source:codex` | Codex thread anchor |
| `source:forge` | forge issue or card with observed forge linkage |
| `pinned:true|false` | local WorkCard overlay pin |
| `snoozed:true|false` | derived current snooze state |

A structured token with an unknown field or invalid value is false. This is deliberate: a typo such as `stage:reviwe` cannot broaden a selection.

## Free text

Unqualified text searches the resident projection only:

- title
- workspace
- stage
- Goal objective
- attention labels
- observed Git branch
- observed forge provider
- observed change-request state
- local tags

No command execution, network access, history load, or FTS lookup occurs during query evaluation.

## Source scope

`SavedView.source_scope` is now enforced before the query:

- all / empty
- scratch
- thread / codex
- forge

Unknown scopes fail closed.

## Projection metadata

WorkCardProjection now carries only already-observed searchable metadata:

- `branch`
- `forge_provider`
- `change_request_state`
- `change_request_draft`

This does not create a new authority. Values are projections from existing Git/Forge observations and disappear when those observations are absent.

## Performance

The Divan scale target now includes a 10,000-card richer query:

```bash
cargo bench --bench scale
```

The accepted resident metadata filter/search target remains p95 <= 50 ms. Hosted CI compiles and tests the benchmark target but does not treat noisy hosted timings as a pass/fail gate.

## Deferred

M7b1 does not add:

- batch mutation (M7b2)
- launch presets (M7b3)
- transcript FTS
- Terminal Drawer / PTY (M7c)
