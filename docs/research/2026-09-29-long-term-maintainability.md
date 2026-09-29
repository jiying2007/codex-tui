# Long-term maintainability research

Date: 2026-09-29
Status: research round 3
Scope: broaden codex-tui design beyond product features to long-term maintenance cost, compatibility, release engineering, terminal portability, security, testing, migrations, observability, accessibility, and extension policy.

## Executive conclusion

The dominant long-term cost of codex-tui will not be Rust or Ratatui.

It will be the number of compatibility surfaces we promise to preserve:

1. Codex App Server versions and experimental methods
2. local/daemon/remote runtime topologies
3. terminal emulators, tmux/Zellij/SSH/WSL/Windows
4. config and keymap schemas
5. local SQLite schema
6. worktree/git behavior
7. plugin/extension APIs
8. multiple agent adapters if scope expands beyond Codex
9. packaging/install/update mechanisms
10. user-facing state and data migration guarantees

Therefore the project should explicitly budget compatibility. Every new public surface must justify its permanent maintenance cost.

The maintainability strategy is:

- keep Codex as the only agent backend through the first mature release
- keep App Server as the only canonical agent boundary
- minimize public API/plugin surface
- use explicit compatibility layers for upstream protocol and local storage
- gate unstable upstream features
- treat terminal capability detection as a dedicated subsystem
- make config/DB migrations one-way, versioned, tested, and recoverable
- ship excellent diagnostics before adding more integrations
- automate release, compatibility and security checks early

## 1. Maintenance-cost model

Classify every capability by its ongoing compatibility burden.

### Tier 0 — internal, freely refactorable

Examples:

- component layout internals
- reducer implementation details
- render caches
- private domain types
- internal actions
- local algorithms

No compatibility promises.

### Tier 1 — persisted user state

Examples:

- SQLite schema
- local aliases/tags
- UI state
- drafts
- workspace mappings

Requires migrations and recovery but remains fully owned by codex-tui.

### Tier 2 — user configuration

Examples:

- config.toml
- keymap
- themes
- workspace policies

Requires deprecation policy and migration tooling.

### Tier 3 — upstream integration

Examples:

- App Server methods
- Codex local rollout fallback
- daemon topology

Requires version/capability matrix and graceful degradation.

### Tier 4 — public extension API

Examples:

- plugin ABI
- scripting API
- external control protocol

Most expensive surface.

Do not introduce Tier 4 before stable user needs justify it.

## 2. Upstream Codex protocol strategy

### Key finding: schema alone is not authority

Current Codex can generate JSON schema, but upstream bug reports show that generated schema and the actually running app-server may diverge under version skew or experimental surfaces.

Also, long-lived app-server daemons can survive a CLI update, leaving client and backend versions different.

Therefore compatibility checks must combine:

- initialize response
- target topology
- running backend fingerprint/version when available
- actual method probing
- observed capabilities
- schema snapshots used by CI, not blindly trusted at runtime

### Initialization fingerprint

Persist/display at connection time:

- client codex-tui version
- server user agent
- server CODEX_HOME
- server platform family/OS
- connection topology
- app-server version when separately probeable
- experimental-api opt-in state
- effective capability flags discovered

### Compatibility states

Use a richer state than a boolean:

~~~text
ExactTested
CompatibleTested
CompatibleUntested
Degraded
Incompatible
VersionSkew
~~~

Do not reject a backend solely because its version differs.

Do reject or disable a feature when the method/payload contract required by that feature is absent.

### Feature capability table

Each high-level feature declares its requirements.

Example:

~~~text
ProjectRegistry:
  preferred:
    project/list
  fallback:
    repo/cwd grouping

ThreadSearch:
  preferred:
    thread/search
  fallback:
    thread/list searchTerm
  fallback2:
    local metadata fuzzy search

NativeSections:
  preferred:
    threadSection/*
  fallback:
    local collections

RemoteDaemon:
  requires:
    supported remote/daemon topology
  otherwise:
    disabled
~~~

This makes feature degradation explicit and testable.

### Protocol snapshots

Keep upstream compatibility fixtures:

~~~text
compat/
  codex/
    0.154/
    0.155/
    0.156/
    current/
~~~

Store only the subset of schema/fixtures required by codex-tui.

CI tests adapter decoding against all supported fixture generations.

Do not expose generated App Server types directly to the UI.

## 3. Avoid exact CLI version binding

Nushell illustrates the maintenance cost of an extension protocol that requires exact matching plugin versions: every core update can require corresponding plugin updates.

codex-tui should avoid creating an equivalent coupling.

Policy:

- version is evidence
- capability is authority
- exact-version checks are reserved for known unsafe topologies such as a stale local managed daemon when correctness cannot be guaranteed

This is especially important for explicitly remote app-server targets, where compatible client/server version skew can be legitimate.

## 4. Experimental upstream API policy

Current Codex project APIs and several useful thread features are experimental.

Rule:

- stable baseline must work without any experimental API
- experimental APIs are adapters behind feature gates
- no persisted state may require an experimental identifier without a fallback mapping
- experimental failure must disable the feature, not corrupt the registry

Suggested feature state:

~~~text
AvailableStable
AvailableExperimental
Unavailable
Broken
~~~

Expose this in diagnostics.

## 5. Local database strategy

SQLite is appropriate for local control-plane metadata.

Mature CLI projects such as Agent Deck and Atuin show SQLite/WAL works well for durable local state, but migration complexity becomes real over time.

### Database ownership

Store:

- profiles
- project mappings
- worktrees managed by codex-tui
- aliases/tags/pins
- drafts
- attention acknowledgements
- UI state
- compatibility cache
- recent selections

Do not duplicate canonical Codex conversation history.

### Migration contract

Use monotonic schema versions:

~~~text
v1 -> v2 -> v3 ...
~~~

Each migration must be:

- transactional
- deterministic
- idempotent where practical
- tested from every supported historical schema
- backed up before destructive transformation
- never dependent on network access

### Recovery

At startup:

1. open DB
2. integrity check when previous shutdown was abnormal
3. create timestamped backup before migration
4. migrate
5. if migration fails, leave original recoverable and start in safe/read-only diagnostics mode

Provide:

~~~text
codex-tui doctor db
codex-tui db backup
codex-tui db restore
codex-tui db migrate --dry-run
~~~

### Cross-platform migration files

If SQL migration hashes are used, normalize line endings. SQLx documentation notes CRLF/LF can change migration hashes, so repository migration files should force LF through .gitattributes.

## 6. Configuration strategy

Configuration becomes permanent API.

Lazygit and Helix demonstrate that config/keymap changes remain visible for years and require migration/deprecation support.

### Keep the initial config intentionally small

Suggested v1 surface:

~~~toml
[ui]
density = "comfortable"
mouse = true
animations = true

[sessions]
default_grouping = "project"
attention_first = true

[terminal]
screen_mode = "auto"

[git]
worktree_policy = "prompt"

[keymap]
# optional overrides
~~~

Avoid exposing internal tuning knobs for caches, polling intervals and protocol details unless a real operational need exists.

### Schema

Ship a machine-readable config schema.

Unknown keys:

- warning by default
- error only in strict validation command

Provide:

~~~text
codex-tui config validate
codex-tui config explain <key>
codex-tui config migrate
~~~

### Deprecation lifecycle

For renamed config:

1. accept old and new key
2. warn with exact replacement
3. provide auto-migrate command
4. remove only in a documented major compatibility window

Do not keep indefinite hidden aliases in implementation code.

## 7. Keymap strategy

Keybindings are unusually expensive compatibility surface because terminal support differs by platform/emulator/multiplexer.

Lazygit currently has platform-specific defaults and lets users override the platform because SSH/container usage can make host OS and keyboard semantics differ.

codex-tui should have:

- command IDs as stable semantic names
- platform-aware defaults
- terminal-capability-aware availability
- user remapping
- visible help generated from the command registry
- conflict detection

Never hard-code behavior directly to raw key combinations across widgets.

### Key event normalization

Normalize:

~~~text
physical/raw event
  -> terminal capability adapter
  -> normalized KeyChord
  -> command registry
~~~

Support enhanced keyboard protocols opportunistically, with legacy fallback.

Do not require CSI-u/Kitty keyboard support for baseline operation.

## 8. Terminal compatibility is a subsystem, not a helper

OpenAI Codex issues show how much maintenance terminal detection creates:

- Windows Terminal rendering differences
- WSL host probes hanging startup
- tmux/Zellij scrollback differences
- palette changes after startup
- enhanced-key protocol differences
- SSH environment ambiguity
- mouse/copy conflicts
- alternate-screen behavior

Create a dedicated subsystem:

~~~text
terminal/
  identity.rs
  capabilities.rs
  keyboard.rs
  color.rs
  screen.rs
  mouse.rs
  clipboard.rs
  probes.rs
  quirks.rs
~~~

### Rule: all probes are bounded

Every subprocess, terminal query, or OS probe needs:

- timeout
- cancellation
- fallback
- tracing
- no blocking of initial usable UI where possible

Agent Deck suffered a multi-minute freeze because a status probe invoked lsof without timeout and DNS-suppression flags. Codex has also seen WSL terminal probes block for roughly a minute.

No unbounded shell-out belongs in UI startup or refresh paths.

### Terminal support tiers

Tier A CI:

- Windows Terminal / ConPTY
- macOS Terminal or iTerm-compatible behavior
- common Linux xterm-compatible terminal

Tier B regression fixtures/manual:

- tmux
- Zellij
- SSH
- WSL
- VS Code terminal
- Ghostty/Kitty/WezTerm enhanced keyboard

Tier C best effort:

- exotic/legacy TERM combinations

## 9. Text width and Unicode policy

Unicode width in terminals is not perfectly standardized, especially for emoji/grapheme clusters.

Policy:

- never make critical navigation depend on emoji width
- use plain ASCII fallback symbols
- test CJK, combining marks and representative emoji
- preserve buffer correctness even if an emulator draws a glyph wider than expected
- decorative glyphs are optional
- no Nerd Font dependency for semantic meaning

## 10. Accessibility

Accessibility is cheaper to build in early than retrofit.

OpenAI Codex has current user reports around VoiceOver, repeated redraw announcements and motion-heavy status text.

Build from the beginning:

### Reduced motion

Animations are optional effects.

~~~text
animations = true/false
~~~

All states must have static representations.

### Screen-reader/quiet mode

Potential later mode:

- no box-drawing dependency
- fewer full-screen redraws
- no shimmer
- semantic plain-text status
- predictable transcript output

### Color independence

Never encode state only by color.

Examples:

~~~text
! Needs input
● Working
○ Ready
× Error
~~~

with color as enhancement.

### Semantic color tokens

Use theme tokens, not hard-coded colors.

Terminal palette can change while the TUI runs, so palette state should be refreshable/invalidateable rather than startup-only global cache.

## 11. Rendering architecture maintenance

Ratatui's immediate-mode rendering is simple, but application architecture is the project's responsibility.

Recommended split inspired by Helix and Ratatui:

~~~text
domain/state
    ↓
view model
    ↓
components
    ↓
surface/render backend
~~~

Overlays/pickers should compose through a stack/compositor-like abstraction instead of special-case booleans in one giant app module.

### Module-size ratchet

Adopt a rule similar to current Codex development guidance:

- target implementation modules under ~500 LOC
- at ~800 LOC, new features normally require extraction
- tests may live in sibling test modules
- central app/reducer files are orchestration only

### No God AppState

Split state by ownership:

~~~text
AppState
  registry
  navigation
  workspace
  thread_views
  attention
  overlays
  terminal
~~~

Reducers/effects remain domain-scoped.

## 12. Async/background work

Yazi's guidance is useful: async I/O stays non-blocking and initialization order is explicit.

Policy:

- no hidden module initialization side effects
- application entrypoint constructs dependencies in explicit order
- every long operation is cancellable or bounded
- no task owns UI state directly
- effects return Actions
- coalesce render requests, not semantic events

### Backpressure

Classify channels:

Lossless/bounded:

- approvals
- errors
- status transitions
- completion
- user commands

Coalescible:

- render dirty
- spinner ticks
- streaming visual progress

Never let an unbounded stream of tool output or deltas allocate memory indefinitely.

## 13. Diagnostics are a first-class feature

Before adding many integrations, build a useful doctor.

Suggested commands:

~~~text
codex-tui doctor
codex-tui doctor --json
codex-tui doctor terminal
codex-tui doctor codex
codex-tui doctor db
codex-tui doctor git
~~~

Report:

- codex-tui version
- Codex CLI path/version
- connected app-server fingerprint
- CODEX_HOME
- protocol feature availability
- daemon version skew
- terminal identity/capabilities
- tmux/Zellij/SSH/WSL detection
- config validation
- DB schema/integrity
- project/thread counts
- recent adapter errors
- paths with secrets redacted

This pays back repeatedly in support cost.

## 14. Logging/observability

Use structured tracing.

Do not write logs into terminal stdout.

For async applications, avoid blocking UI/event-loop paths on file logging.

Fields:

- connection_id
- project_id
- thread_id
- turn_id
- RPC method
- latency
- terminal capability
- frame duration
- queue depth
- migration version

Sensitive prompt/tool content is excluded by default.

Provide explicit diagnostic verbosity opt-in when content is needed.

## 15. Plugin/extensibility policy

Do not build a general plugin system in v0.x.

Why:

- Nushell requires matching plugin protocol versions
- Yazi has repeatedly needed plugin migration guides and currently warns that many plugins only guarantee latest-version compatibility
- Zellij invests heavily in preserving old plugin wire protocol while evolving source APIs
- security/permission UX becomes another product

The cheapest extension system is initially:

1. stable command registry
2. custom shell commands/actions
3. external control CLI
4. declarative views/policies where safe

If a real plugin system is later justified, prefer:

- out-of-process or WASM isolation
- explicit protocol version
- capability permissions
- compatibility handshake
- no access to internal Rust types
- deny-by-default dangerous operations

Zellij demonstrates the value of permission-gated plugins, but also the UX complexity of permission prompts.

## 16. Public control protocol

A local control socket could be highly valuable later, as Workbench demonstrates.

But treat it as Tier-4 API.

Before stabilization:

- mark experimental
- namespaced methods
- explicit version handshake
- additive evolution
- event subscription separate from request/response
- permission/local-only boundary
- no hidden access to secrets

Do not make internal Action enum the wire protocol.

## 17. Multi-agent support: defer

Agent Deck demonstrates the real maintenance cost of generic multi-agent managers:

- agent-specific hooks
- pane scraping fallbacks
- process inspection
- different resume/fork semantics
- version-specific flags
- readiness pattern drift
- status detection failures
- adapter-specific environment/config behavior

For codex-tui:

### Until Codex-native product is mature

Support:

~~~text
Codex = first-class
Other agents = not supported
~~~

Do not advertise generic agent manager capability.

### Future

If another agent is added, it must implement a narrow backend contract and pass the same lifecycle conformance suite.

No backend-specific conditions in UI components.

## 18. Worktree/git boundary

Git should be a separate service abstraction, not scattered shell commands.

Suggested interface:

~~~text
GitService
  repo_identity
  worktrees
  status
  diff
  create_worktree
  remove_worktree
  branch_info
~~~

Operations that mutate worktrees/index are serialized per repository.

Every external git process:

- timeout/cancellation where safe
- explicit cwd
- structured error
- no shell string concatenation

Never assume the thread's cwd is repository root.

## 19. Persistence vs runtime state

Do not recreate the complexity seen in tmux-based managers by conflating:

- conversation persistence
- process persistence
- UI persistence

For Codex-native mode:

Conversation persistence = App Server/Codex storage.

Runtime persistence = App Server loaded thread/daemon topology.

UI persistence = codex-tui SQLite.

Keep these authorities separate.

## 20. Release engineering

Automate releases early.

Recommended outputs:

- Linux x86_64 GNU
- Linux x86_64 musl
- Linux arm64 where CI allows
- macOS x86_64
- macOS arm64
- Windows x86_64

Use reproducible CI-built archives with SHA256 manifests.

cargo-dist is a strong candidate for generating cross-platform installers/releases.

Do not make Homebrew/NPM/winget/etc all blocking launch requirements initially. Start with GitHub Release archives + installer scripts, then add package managers based on demand.

### Stable / preview channels

Recommended once user base grows:

- stable
- preview

Preview can test upcoming Codex protocol changes without destabilizing stable users.

## 21. Rust toolchain policy

Set package.rust-version.

Prefer stable Rust only.

Suggested policy:

- support a moving ~9-12 month stable Rust window, or
- if binary distribution is the overwhelming install path, document a somewhat newer MSRV but do not change it in patch releases

CI:

- MSRV
- stable
- optional beta

Nightly only for optional tooling, never required to build the application.

## 22. Dependency policy

Dependency count is maintenance surface.

Rules:

- prefer well-established crates
- avoid duplicate-purpose crates
- minimize git dependencies
- avoid nightly-only dependencies
- record why unusually large/system-sensitive dependencies exist
- feature-gate expensive platform-specific integrations

Security automation:

- cargo-deny for advisories/licenses/bans/sources
- RustSec/cargo-audit
- Dependabot/Renovate for controlled updates
- optional cargo-vet later if supply-chain assurance becomes a requirement
- zizmor or similar GitHub Actions static analysis

Use an explicit allowed-license policy because reference projects include MIT, Apache-2.0, AGPL and custom source-available code.

## 23. License/reference governance

Create a reference registry:

~~~text
docs/references/
  SOURCES.md
~~~

For each project:

- repo
- license
- what concepts are being referenced
- whether code reuse is permitted
- attribution requirements

HachimoDock is conceptual-only under its current license.

AGPL projects are architectural reference only unless the project intentionally accepts AGPL obligations.

MIT/Apache references can be reused with required notices, but independent implementation is still preferred for architecture-specific code.

## 24. Testing pyramid

### Pure tests

- reducer
- capability negotiation
- migration logic
- project identity
- attention state
- worktree policy
- keymap resolution

### Protocol fixture tests

Run against saved payload/schema generations.

### Snapshot tests

Ratatui TestBackend + insta.

Widths:

- 40
- 80
- 120
- 160

Themes:

- dark
- light
- no-color/limited color

Content:

- CJK
- combining
- emoji
- long paths
- huge tool output
- approval
- errors
- narrow panes

### PTY tests

Cover:

- startup/exit
- Ctrl-C
- panic cleanup
- resize
- paste
- mouse
- external editor
- suspend/resume
- tmux/Zellij fixtures where feasible

### Real Codex compatibility tests

A scheduled/non-PR matrix:

- minimum supported Codex
- previous stable
- current stable
- optional current prerelease

Tests should use isolated CODEX_HOME and mocked model traffic wherever possible.

Do not require paid live inference to certify basic UI/protocol compatibility.

## 25. Snapshot governance

Snapshots are review artifacts, not automatic truth.

Policy:

- UI-changing PR must include snapshot changes
- CI rejects unreviewed pending snapshots
- snapshot update is explicit
- large mechanical snapshot churn should trigger scrutiny

This mirrors the official Codex/Ratatui practice.

## 26. Performance budgets

Make budgets explicit.

Suggested targets to refine after measurement:

- usable shell visible quickly even if optional probes are still running
- registry initial metadata render under ~100 ms after backend response
- key input-to-frame p95 under ~50 ms under normal load
- streaming does not exceed configured frame rate
- 10k thread metadata registry remains interactive
- multi-MB tool output stays bounded/lazy
- background probes cannot stall the UI event loop

Benchmarks become regression gates only after stable baselines are measured.

## 27. Failure isolation

A failure in one project/thread must not take down the whole registry.

Per-thread error boundaries:

~~~text
ThreadHealthy
ThreadDegraded(reason)
ThreadUnreadable(reason)
~~~

Per-feature boundaries:

~~~text
ProjectsUnsupported
ThreadSearchUnsupported
TerminalProbeFailed
GitUnavailable
~~~

The TUI remains usable and explains degraded behavior.

## 28. Offline-first baseline

Core local registry/resume/navigation should work without web access beyond what Codex itself needs for inference.

Do not make:

- update checks
- analytics
- GitHub
- plugin registries
- remote metadata

part of startup's critical path.

## 29. Privacy/data handling

Local control DB can contain sensitive project names/paths and prompt drafts.

Rules:

- never store auth tokens
- never copy full transcripts by default
- diagnostic bundles redact home paths/session IDs unless user asks otherwise
- logs exclude prompts/tool outputs by default
- backups use user-only permissions
- local control socket is local-user only

## 30. Feature lifecycle

Every major feature should have an owner and lifecycle status:

~~~text
Experimental
Preview
Stable
Deprecated
Removed
~~~

Experimental features may change without migration promise.

Stable features require compatibility/deprecation policy.

This prevents every prototype from becoming permanent debt.

## 31. Architecture decision governance

ADRs should be used for decisions with long-lived cost.

New ADR candidates:

- ADR-020: Compatibility surfaces have explicit tiers.
- ADR-021: Capability negotiation outranks exact Codex version matching.
- ADR-022: Stable baseline does not require experimental App Server APIs.
- ADR-023: Local DB migrations are transactional, backed up and recoverable.
- ADR-024: Config/keymap are versioned public interfaces with deprecation policy.
- ADR-025: Terminal detection and quirks are isolated behind a capability subsystem.
- ADR-026: All external probes and subprocess reads are bounded.
- ADR-027: Accessibility state is semantic and independent of color/animation.
- ADR-028: No general plugin system before a stable first-party product.
- ADR-029: Codex remains the only first-class agent backend through the initial stable release.
- ADR-030: Public control protocols do not expose internal Action/domain types.
- ADR-031: Dependency, license and GitHub Actions supply chain are CI-governed.
- ADR-032: Releases are reproducible multi-platform artifacts with checksums.
- ADR-033: Feature lifecycle distinguishes experimental/preview/stable/deprecated.
- ADR-034: Diagnostics/doctor is part of the product, not a post-launch add-on.

## 32. Architecture shape after round 3

Recommended long-term structure:

~~~text
codex-tui
├─ app
│  ├─ actions
│  ├─ reducers
│  ├─ effects
│  └─ navigation
├─ domain
│  ├─ project
│  ├─ thread
│  ├─ attention
│  ├─ worktree
│  └─ capability
├─ backend
│  └─ codex
│     ├─ protocol
│     ├─ compat
│     ├─ transports
│     └─ fallback_discovery
├─ registry
│  ├─ project_index
│  ├─ session_index
│  └─ lazy_cache
├─ persistence
│  ├─ db
│  ├─ migrations
│  ├─ config
│  └─ backup
├─ git
├─ terminal
│  ├─ capabilities
│  ├─ keyboard
│  ├─ palette
│  ├─ screen
│  ├─ mouse
│  └─ quirks
├─ ui
│  ├─ compositor
│  ├─ components
│  ├─ theme
│  └─ keymap
├─ diagnostics
├─ security
└─ cli
~~~

Do not split this into many Cargo crates immediately. Keep module boundaries first; split crates only when independent compile/API boundaries become valuable.

## 33. Reference lessons

### Helix

Useful:

- functional core
- explicit view/TUI layer
- compositor/layers
- command registry separated from keymap
- generic event subsystem
- cancellable/debounced async hooks

Maintenance lesson:

Clear internal boundaries let a large terminal application evolve with many contributors.

### Ratatui

Useful:

- component template
- event handling remains application-owned
- snapshot testing
- PTY tests for whole application
- terminal handoff/suspend patterns

### Zellij

Useful:

- session compatibility across versions
- built-in plugins used to dogfood extension APIs
- preserve old wire protocol when source APIs change
- explicit plugin permissions

Maintenance warning:

Plugin compatibility and permission UX are permanent work.

### Lazygit

Useful:

- semantic configurable keymap
- config schema
- platform-aware key defaults
- custom commands

Maintenance warning:

Once keymap/config become public, deprecations and terminal-specific behavior remain long-lived.

### Yazi

Useful:

- async I/O architecture
- explicit initialization/dependency order
- plugin package pinning
- migration documentation

Maintenance warning:

Fast-moving plugin APIs create substantial upgrade burden.

### Atuin

Useful:

- SQLite/WAL
- explicit protocol generations
- separating current and legacy protocol

Maintenance warning:

Automatic data/protocol migration is operationally difficult and must be planned early.

### Nushell

Useful:

- explicit versioned plugin protocol

Maintenance warning:

Exact plugin/core version coupling creates synchronized upgrade burden.

### Agent Deck

Useful:

- mature session-control DB and command center

Maintenance warning:

Generic agent support brings continual agent-specific lifecycle/status/version heuristics and subprocess probing bugs.

### OpenAI Codex itself

Useful:

- App Server v2 evolution
- compatibility schemas
- status/Project/Thread primitives
- module-size guidance
- snapshots
- terminal quirk handling

Maintenance warning:

Daemon version skew, experimental schema/method mismatch, terminal probing and remote topology are all active sources of complexity.

## 34. Revised scope principle

The safest long-term product boundary is:

> codex-tui is a Codex-native project/thread/worktree control plane with an integrated conversation view.

Not:

> a universal terminal supervisor for every coding agent.

If a future multi-agent product is desired, build it on top of codex-tui's external control/backend abstraction later instead of polluting the initial core.

## 35. Next research directions

Continue broad research in parallel tracks:

1. terminal capability matrix and automated PTY lab
2. App Server compatibility/schema-diff automation
3. config + DB migration framework options
4. worktree/git concurrency model
5. attention/notification UX from k9s, editors, issue trackers and ops consoles
6. scalable registry/search indexing
7. release/update/rollback strategy
8. diagnostics bundle design
9. accessibility and reduced-motion baseline
10. remote/SSH topology without daemon lock-in
11. security/threat model for approvals, local control socket and external commands
12. contribution architecture and code-ownership boundaries
13. benchmark methodology and memory budgets
14. licensing/reference governance

The project should continue exploring broadly while preserving the central architectural constraint: every new feature must have an explicit ongoing maintenance-cost assessment.
