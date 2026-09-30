# M7d2: compatibility matrix and Doctor evidence contract

Status: implementation slice under #38 / #40

M7d2 turns compatibility diagnostics into a versioned machine contract. It does not add a daemon, remote inventory service, or hidden forge probe.

## Compat v2

`codex-tui doctor compat --json` emits:

- schema: `codex-tui/compat/v2`
- product version
- Rust MSRV
- OS and architecture
- overall readiness
- required failures
- required degraded components
- optional unavailable components
- structured component records
- canonical platform matrix
- retained release-evidence requirements
- Forge runtime authority policy

The text form is a human-readable rendering of the same report.

## Readiness semantics

Components are classified independently from their current state.

### Required

- SQLite local store
- Codex App Server
- Git CLI
- native PTY capability

States:

- **available** — capability is usable
- **degraded** — usable but its health contract reports a material limitation
- **unavailable** — required capability cannot be used

Overall readiness:

- **ready** — all required components are available
- **degraded** — no required component is unavailable, but at least one is degraded
- **blocked** — at least one required component is unavailable

`doctor compat` returns exit code 3 only for **blocked**. A degraded report is still machine-readable and returns success so diagnostics can be consumed without treating every optional/degraded feature as process failure.

### Optional

- `glab`
- `gh`

Their absence is reported in `optionalUnavailable` and never blocks the core product. The client becomes relevant only when the selected repository/provider needs that Forge capability.

This corrects the old v1 behavior where the absence of both Forge CLIs globally degraded compatibility even for a developer not using Forge integration.

## No implicit Forge network probe

`doctor compat` performs:

- local SQLite health
- Codex App Server probe
- local Git/glab/gh version probes
- compiled PTY capability inspection

It does **not** resolve a repository remote, authenticate to a Forge, or issue GitLab/GitHub API calls.

Detailed repository Forge diagnostics remain explicit:

```bash
codex-tui doctor forge
```

That command is the runtime authority for the selected repository's provider identity, authentication, server metadata and capability probes.

## GitLab version, edition and tier

For GitLab:

- `/version` may expose server version and an enterprise/community edition indicator;
- when the current non-admin authentication exposes that metadata, Doctor records it;
- subscription tier is **not inferred from the version number**;
- M7d2 does not call administrator-only license endpoints to discover tier;
- if tier is not safely discoverable, Doctor prints `<unknown/not-discoverable>`.

Actual feature support remains authoritative through runtime capability probes. Premium/Ultimate-only behavior is therefore capability-gated rather than version-table-gated.

## GitHub

GitHub.com continues to use `gh` under the existing ForgeProvider contract. `doctor compat` only checks the local client. Repository-specific auth/provider data belongs to `doctor forge`.

## PTY compatibility

The required `terminal-pty` component records:

- backend
- platform
- default-program support
- input support
- resize support
- bounded event queue
- default scrollback byte budget

This complements the retained PTY lifecycle CI from M7c3.

## Retained evidence

Some release evidence cannot be honestly produced by Doctor.

The v2 report makes this explicit:

### terminal-restoration

- scope: real controlling TTY on each supported platform
- authority: retained interactive smoke receipt
- stable-required: yes
- doctor-observable: no

This covers raw mode, alternate screen, cursor restoration and parent-terminal usability.

### pty-lifecycle

- scope: canonical Ubuntu/macOS/Windows CI
- authority: `cargo test --all-targets --all-features`
- stable-required: yes
- doctor-observable: no

M7d3 will consume these evidence requirements as release gates; M7d2 only defines the contract.

## Canonical matrix

The machine report names Linux, macOS and Windows as canonical CI platforms. A test binds that matrix to the actual CI workflow runner names so documentation cannot silently drift away from repository gates.

The Rust MSRV is similarly asserted against `Cargo.toml`.

## Forge transport policy

The report carries policy metadata rather than starting network work:

- GitLab transport: `glab`
- GitHub transport: `gh`
- default remote probe: false
- capability authority: runtime probes
- native GitLab transport: evidence-gated by the frozen final implementation choices

The previously accepted hard triggers for native GitLab REST/GraphQL remain unchanged.

## Non-goals

M7d2 does not add:

- an online compatibility service
- native GitLab transport
- hard-coded GitLab version/tier tables
- admin-only license discovery
- background remote probing
- a second release database
- FTS or new product functionality
