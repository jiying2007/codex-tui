<!-- docs-id: team-quickstart -->
<!-- docs-lang: en -->
# Team quickstart
<!-- docs-section: overview -->

**Language / 语言:** [English](team-quickstart.md) · [简体中文](zh-CN/team-quickstart.md)


codex-tui is personal-first. Team reuse comes from the repository and existing forge; there is no codex-tui team server, shared database or RBAC layer.

## 1. Install and verify
<!-- docs-section: install -->

Use the native archive for the current platform and verify it against the release bundle `SHA256SUMS`.

After putting the binary on `PATH`:

```text
codex-tui --version
codex-tui doctor compat
```

`doctor` prints the local config/state paths. Keep those files per developer; do not check local SQLite/operator state into the repository.

## 2. Keep team authority in the repository
<!-- docs-section: authority -->

Use:

- `AGENTS.md` for repository/team instructions;
- `.codex/config.toml` for Codex repository configuration;
- Git/GitLab/GitHub for canonical collaboration and delivery state.

codex-tui projects those authorities and stores only local operator/planning state.

## 3. Optional safe launch presets
<!-- docs-section: presets -->

If the team wants shared shortcuts, add only the narrow repository-root `.codex-tui.toml` format:

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

Validate without executing:

```text
codex-tui doctor presets
```

Presets are argv-only. Shell strings, environment templating, chaining, hooks and schedulers are intentionally unsupported.

## 4. Forge setup
<!-- docs-section: forge -->

For an internal GitLab repository, authenticate `glab` to that exact host. For GitHub.com read-only projection, authenticate `gh`.

Then run:

```text
codex-tui doctor forge
```

Maintainers can retain a capability fixture with `scripts/release/capture_forge_capability.py`; capabilities come from the observed environment, not from a guessed server-version matrix.

## 5. Daily flow
<!-- docs-section: daily -->

The small set worth learning first:

- `Ctrl+K`: contextual command palette;
- `/`: global search, with Esc restoring the origin view when possible;
- `j/k` + Enter: navigate/open;
- `t`: Terminal Drawer; `F6` (or Ctrl+]) returns focus to codex-tui;
- Board: `b`/palette to open, then `h/l` columns and `j/k` items.

`?` is the executable Help contract; CI checks it against the keymap/Command/reducer surface.

## 6. Troubleshooting
<!-- docs-section: troubleshooting -->

Start with:

```text
codex-tui doctor compat
codex-tui doctor store
codex-tui doctor forge
codex-tui doctor terminal
```

For a shareable diagnostic artifact:

```text
codex-tui doctor bundle --output ./codex-tui-support
```

The bundle is metadata-only and excludes environment variables, authentication tokens, prompts/transcripts, comment bodies, repository/file paths and raw errors.

If local state is damaged after an upgrade, do not repeatedly delete/reinitialize it. Preserve the state directory and support bundle first; forward/unknown schemas and corrupt stores fail closed.

## Deliberate non-goals
<!-- docs-section: boundaries -->

Do not add these merely for team reuse without evidence:

- Registry paging / a second Registry SQLite index;
- native GitLab REST/GraphQL beside the current provider abstraction;
- codex-tui team server/RBAC;
- generic plugin or multi-agent frameworks;
- Terminal Drawer as a tmux replacement.
