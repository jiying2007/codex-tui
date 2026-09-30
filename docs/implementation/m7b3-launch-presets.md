# M7b3: repository-shared safe launch presets

Status: implementation slice under #24 / #27

M7b3 adds repository-shared launch presets without turning codex-tui into a workflow engine or shell wrapper.

## Repository config

Presets live only in the repository root:

```text
.codex-tui.toml
```

The format is versioned and intentionally narrow:

```toml
version = 1

[[launch]]
name = "Tests"
argv = ["cargo", "test", "--all-targets"]
cwd = "repo"

[[launch]]
name = "Open workspace"
argv = ["code", "."]
cwd = "thread"
```

Only these fields are accepted:

- top level: `version`, `[[launch]]`
- launch: `name`, `argv`, `cwd`

Unknown fields fail closed via serde `deny_unknown_fields`.

## Safety contract

A launch preset is not a shell command string.

- `argv` must be a non-empty TOML array;
- codex-tui calls `Command::new(argv[0]).args(&argv[1..])`;
- no shell interpolation, command concatenation or eval is performed;
- common shell executables (`sh`, `bash`, `zsh`, `fish`, `cmd`, PowerShell/pwsh) are rejected;
- environment injection is not part of the config schema;
- names, preset count, argv count and argument lengths are bounded;
- `cwd` is exactly `repo` or `thread`;
- `thread` cwd must canonicalize inside the canonical repository root.

A repository can still launch an executable or executable script that its users intentionally committed/configured. The key boundary is that codex-tui does not interpret a command language.

## UI flow

Workspace / Review context actions expose **Launch repository preset…** when Git repository identity is known.

The flow is:

1. read and validate `.codex-tui.toml`;
2. choose a preset;
3. build a canonical `LaunchPlan`;
4. preview:
   - preset name;
   - config path;
   - canonical cwd;
   - exact argv;
5. press `y` to start or `c` / Esc to cancel.

The reducer never reads config files or canonicalizes paths. Those operations remain effects/runtime work.

The external process is spawned with stdin/stdout/stderr detached to null. The UI reports the spawned PID or the immediate spawn failure. codex-tui does not become the child process supervisor.

## Doctor

From a repository:

```bash
codex-tui doctor presets
```

The command prints:

- resolved `.codex-tui.toml` path;
- config version;
- preset count;
- each preset name, cwd scope and exact argv;
- validation errors in degraded form.

Doctor does not execute a preset.

## Explicit non-goals

M7b3 does not add:

- environment-variable templating;
- shell strings;
- command substitution;
- chained commands;
- hooks;
- dependencies between presets;
- background job scheduling;
- remote execution;
- embedded PTY.

The embedded Terminal Drawer remains M7c.
