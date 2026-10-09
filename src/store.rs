use crate::app_server_target::AppServerConfig;
use crate::domain::ThreadUiState;
use crate::{
    i18n::LanguagePreference, notification::NotificationMode, presentation::PresentationMode,
};
use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub mouse: bool,
    #[serde(default)]
    pub language: LanguagePreference,
    #[serde(default)]
    pub presentation: PresentationMode,
}

const fn default_true() -> bool {
    true
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            mouse: true,
            language: LanguagePreference::Auto,
            presentation: PresentationMode::Normal,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationsConfig {
    #[serde(default)]
    pub mode: NotificationMode,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchConfig {
    // Raw user/assistant messages never persist without explicit local opt-in.
    #[serde(default)]
    pub persist_local_transcripts: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub notifications: NotificationsConfig,
    #[serde(default)]
    pub app_server: AppServerConfig,
    #[serde(default)]
    pub search: SearchConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStateV1 {
    pub schema_version: u32,
    #[serde(default)]
    pub thread_ui: BTreeMap<String, ThreadUiState>,
    #[serde(default)]
    pub pins: BTreeSet<String>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub marked_unread: BTreeSet<String>,
    #[serde(default)]
    pub acknowledged_attention: BTreeSet<String>,
    #[serde(default)]
    pub host_local_only: bool,
    #[serde(default)]
    pub repo_backed_only: bool,
}

impl Default for LocalStateV1 {
    fn default() -> Self {
        Self {
            schema_version: 1,
            thread_ui: BTreeMap::new(),
            pins: BTreeSet::new(),
            aliases: BTreeMap::new(),
            marked_unread: BTreeSet::new(),
            acknowledged_attention: BTreeSet::new(),
            host_local_only: false,
            repo_backed_only: false,
        }
    }
}

pub trait LocalStore {
    fn load_config(&self) -> Result<AppConfig>;
    fn load_state(&self) -> Result<LocalStateV1>;
    fn save_state(&self, state: &LocalStateV1) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct FileStore {
    config_dir: PathBuf,
    state_dir: PathBuf,
}

impl FileStore {
    pub fn discover() -> Result<Self> {
        let dirs = ProjectDirs::from("dev", "jiying2007", "codex-tui")
            .context("unable to resolve platform application directories")?;
        Ok(Self {
            config_dir: dirs.config_dir().to_path_buf(),
            state_dir: dirs.data_local_dir().to_path_buf(),
        })
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            config_dir: root.join("config"),
            state_dir: root.join("state"),
        }
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn data_dir(&self) -> &Path {
        &self.state_dir
    }

    fn ensure_default_config(&self) -> Result<()> {
        let path = self.config_path();
        if path.exists() {
            return Ok(());
        }
        let default =
            toml::to_string_pretty(&AppConfig::default()).context("serialize default config")?;
        atomic_write(&path, default.as_bytes())
    }
}

impl FileStore {
    pub fn load_config(&self) -> Result<AppConfig> {
        self.ensure_default_config()?;
        let path = self.config_path();
        let text =
            fs::read_to_string(&path).with_context(|| format!("read config {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parse config {}", path.display()))
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("create directory {}", parent.display()))?;

    let mut temp = NamedTempFile::new_in(parent).context("create temporary state file")?;
    temp.write_all(bytes)
        .context("write temporary state file")?;
    temp.flush().context("flush temporary state file")?;
    temp.as_file()
        .sync_all()
        .context("sync temporary state file")?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("atomically replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn transcript_persistence_is_opt_in_only() {
        assert!(!AppConfig::default().search.persist_local_transcripts);
        let configured: AppConfig = toml::from_str(
            "[search]\npersist_local_transcripts = true\n",
        )
        .expect("opt-in config");
        assert!(configured.search.persist_local_transcripts);
    }

    #[test]
    fn config_defaults_are_toml() {
        let root = tempdir().expect("tempdir");
        let store = FileStore::at(root.path());
        let config = store.load_config().expect("config");
        assert!(config.ui.mouse);
        assert_eq!(config.ui.language, LanguagePreference::Auto);
        assert_eq!(config.ui.presentation, PresentationMode::Normal);
        assert_eq!(config.notifications.mode, NotificationMode::Off);
        assert_eq!(config.app_server.active, "local");
        assert!(config.app_server.targets.is_empty());

        let config_text = fs::read_to_string(store.config_path()).expect("read config");
        assert!(config_text.contains("[ui]"));
        assert!(config_text.contains("language = \"auto\""));
        assert!(config_text.contains("presentation = \"normal\""));
        assert!(config_text.contains("[notifications]"));
        assert!(config_text.contains("mode = \"off\""));
        assert!(config_text.contains("[app_server]"));
        assert!(config_text.contains("active = \"local\""));
    }

    #[test]
    fn config_accepts_explicit_english_and_simplified_chinese() {
        let root = tempdir().expect("tempdir");
        let store = FileStore::at(root.path());

        store.load_config().expect("create default config");
        fs::write(
            store.config_path(),
            "[ui]\nmouse = true\nlanguage = \"zh-CN\"\n",
        )
        .expect("write zh config");
        assert_eq!(
            store.load_config().expect("zh config").ui.language,
            LanguagePreference::SimplifiedChinese
        );

        fs::write(
            store.config_path(),
            "[ui]\nmouse = true\nlanguage = \"en\"\n",
        )
        .expect("write en config");
        assert_eq!(
            store.load_config().expect("en config").ui.language,
            LanguagePreference::English
        );
    }

    #[test]
    fn legacy_config_without_notifications_defaults_to_off() {
        let root = tempdir().expect("tempdir");
        let store = FileStore::at(root.path());
        fs::create_dir_all(store.config_path().parent().expect("config parent"))
            .expect("config dir");
        fs::write(
            store.config_path(),
            "[ui]\nmouse = true\nlanguage = \"en\"\n",
        )
        .expect("write legacy config");

        let config = store.load_config().expect("legacy config");
        assert_eq!(config.ui.presentation, PresentationMode::Normal);
        assert_eq!(config.notifications.mode, NotificationMode::Off);
    }

    #[test]
    fn presentation_mode_accepts_quiet_and_screen_reader_values() {
        let root = tempdir().expect("tempdir");
        let store = FileStore::at(root.path());
        fs::create_dir_all(store.config_path().parent().expect("config parent"))
            .expect("config dir");
        for (wire, expected) in [
            ("quiet", PresentationMode::Quiet),
            ("screen-reader", PresentationMode::ScreenReader),
        ] {
            fs::write(
                store.config_path(),
                format!("[ui]\nmouse = true\nlanguage = \"auto\"\npresentation = \"{wire}\"\n"),
            )
            .expect("write presentation config");
            assert_eq!(
                store
                    .load_config()
                    .expect("presentation config")
                    .ui
                    .presentation,
                expected
            );
        }
    }

    #[test]
    fn notification_mode_accepts_terminal_and_os_values() {
        let root = tempdir().expect("tempdir");
        let store = FileStore::at(root.path());
        fs::create_dir_all(store.config_path().parent().expect("config parent"))
            .expect("config dir");
        for (wire, expected) in [
            ("terminal", NotificationMode::Terminal),
            ("os", NotificationMode::Os),
        ] {
            fs::write(
                store.config_path(),
                format!(
                    "[ui]\nmouse = true\nlanguage = \"auto\"\n\n[notifications]\nmode = \"{wire}\"\n"
                ),
            )
            .expect("write notification config");
            assert_eq!(
                store
                    .load_config()
                    .expect("notification config")
                    .notifications
                    .mode,
                expected
            );
        }
    }
}
