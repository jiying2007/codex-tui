use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const REPO_CONFIG_FILE: &str = ".codex-tui.toml";
pub const LAUNCH_CONFIG_VERSION: u32 = 1;
const MAX_PRESETS: usize = 64;
const MAX_ARGV: usize = 64;
const MAX_ARG_CHARS: usize = 4096;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchCwd {
    #[default]
    Repo,
    Thread,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchPreset {
    pub name: String,
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: LaunchCwd,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoLaunchConfig {
    pub version: u32,
    #[serde(default, rename = "launch")]
    pub launches: Vec<LaunchPreset>,
}

impl Default for RepoLaunchConfig {
    fn default() -> Self {
        Self {
            version: LAUNCH_CONFIG_VERSION,
            launches: vec![],
        }
    }
}

impl RepoLaunchConfig {
    pub fn load(repo_root: &Path) -> Result<Self> {
        let path = repo_root.join(REPO_CONFIG_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read repository launch config {}", path.display()))?;
        let config: Self = toml::from_str(&text)
            .with_context(|| format!("parse repository launch config {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.version == LAUNCH_CONFIG_VERSION,
            "unsupported .codex-tui.toml version {}",
            self.version
        );
        anyhow::ensure!(
            self.launches.len() <= MAX_PRESETS,
            "too many launch presets; maximum is {MAX_PRESETS}"
        );

        let mut names = BTreeSet::new();
        for preset in &self.launches {
            preset.validate()?;
            anyhow::ensure!(
                names.insert(preset.name.trim().to_ascii_lowercase()),
                "duplicate launch preset name: {}",
                preset.name
            );
        }
        Ok(())
    }
}

impl LaunchPreset {
    pub fn validate(&self) -> Result<()> {
        let name = self.name.trim();
        anyhow::ensure!(!name.is_empty(), "launch preset name must not be empty");
        anyhow::ensure!(
            name.chars().count() <= 80,
            "launch preset name must be at most 80 characters"
        );
        anyhow::ensure!(!self.argv.is_empty(), "launch preset argv must not be empty");
        anyhow::ensure!(
            self.argv.len() <= MAX_ARGV,
            "launch preset argv exceeds {MAX_ARGV} items"
        );
        for (index, arg) in self.argv.iter().enumerate() {
            anyhow::ensure!(
                !arg.contains('\0'),
                "launch preset argv[{index}] contains NUL"
            );
            anyhow::ensure!(
                arg.chars().count() <= MAX_ARG_CHARS,
                "launch preset argv[{index}] exceeds {MAX_ARG_CHARS} characters"
            );
        }
        anyhow::ensure!(
            !self.argv[0].trim().is_empty(),
            "launch preset executable must not be empty"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchPlan {
    pub name: String,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub config_path: PathBuf,
}

impl LaunchPlan {
    pub fn build(
        preset: &LaunchPreset,
        repo_root: &Path,
        thread_cwd: &Path,
    ) -> Result<Self> {
        preset.validate()?;
        let repo_root = canonical_directory(repo_root, "repository root")?;
        let thread_cwd = canonical_directory(thread_cwd, "thread cwd")?;
        anyhow::ensure!(
            thread_cwd.starts_with(&repo_root),
            "thread cwd is outside repository root"
        );
        let cwd = match preset.cwd {
            LaunchCwd::Repo => repo_root.clone(),
            LaunchCwd::Thread => thread_cwd,
        };
        Ok(Self {
            name: preset.name.trim().to_string(),
            argv: preset.argv.clone(),
            cwd,
            config_path: repo_root.join(REPO_CONFIG_FILE),
        })
    }

    pub fn command_preview(&self) -> String {
        self.argv
            .iter()
            .map(|arg| format!("{arg:?}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn execute(&self) -> Result<u32> {
        let (program, args) = self
            .argv
            .split_first()
            .context("launch plan argv is empty")?;
        let child = Command::new(program)
            .args(args)
            .current_dir(&self.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| {
                format!(
                    "spawn launch preset {:?} in {}",
                    self.name,
                    self.cwd.display()
                )
            })?;
        Ok(child.id())
    }
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("resolve {label} {}", path.display()))?;
    anyhow::ensure!(
        canonical.is_dir(),
        "{label} is not a directory: {}",
        canonical.display()
    );
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn config_accepts_only_versioned_argv_presets() {
        let root = tempdir().expect("tempdir");
        fs::write(
            root.path().join(REPO_CONFIG_FILE),
            r#"
version = 1

[[launch]]
name = "Tests"
argv = ["cargo", "test", "--all-targets"]
cwd = "repo"

[[launch]]
name = "Current thread"
argv = ["code", "."]
cwd = "thread"
"#,
        )
        .expect("write");

        let config = RepoLaunchConfig::load(root.path()).expect("config");
        assert_eq!(config.launches.len(), 2);
        assert_eq!(config.launches[0].argv[0], "cargo");
        assert_eq!(config.launches[1].cwd, LaunchCwd::Thread);
    }

    #[test]
    fn config_rejects_shell_string_and_unknown_fields() {
        let root = tempdir().expect("tempdir");
        fs::write(
            root.path().join(REPO_CONFIG_FILE),
            r#"
version = 1

[[launch]]
name = "Bad"
command = "cargo test && rm -rf build"
"#,
        )
        .expect("write");
        assert!(RepoLaunchConfig::load(root.path()).is_err());
    }

    #[test]
    fn duplicate_names_and_empty_argv_fail_closed() {
        let config = RepoLaunchConfig {
            version: 1,
            launches: vec![
                LaunchPreset {
                    name: "Tests".into(),
                    argv: vec!["cargo".into()],
                    cwd: LaunchCwd::Repo,
                },
                LaunchPreset {
                    name: "tests".into(),
                    argv: vec![],
                    cwd: LaunchCwd::Repo,
                },
            ],
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn thread_scope_cannot_escape_repository() {
        let repo = tempdir().expect("repo");
        let outside = tempdir().expect("outside");
        let preset = LaunchPreset {
            name: "outside".into(),
            argv: vec!["echo".into(), "ok".into()],
            cwd: LaunchCwd::Thread,
        };
        assert!(LaunchPlan::build(&preset, repo.path(), outside.path()).is_err());
    }
}
