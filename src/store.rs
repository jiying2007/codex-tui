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
pub struct AppConfig {
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub notifications: NotificationsConfig,
    #[serde(default)]
    pub app_server: AppServerConfig,
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

    pub fn state_path(&self) -> PathBuf {
        self.state_dir.join("state-v1.json")
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

impl LocalStore for FileStore {
    fn load_config(&self) -> Result<AppConfig> {
        self.ensure_default_config()?;
        let path = self.config_path();
        let text =
            fs::read_to_string(&path).with_context(|| format!("read config {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parse config {}", path.display()))
    }

    fn load_state(&self) -> Result<LocalStateV1> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(LocalStateV1::default());
        }
        let text =
            fs::read_to_string(&path).with_context(|| format!("read state {}", path.display()))?;
        let state: LocalStateV1 = serde_json::from_str(&text)
            .with_context(|| format!("parse state {}", path.display()))?;
        anyhow::ensure!(
            state.schema_version == 1,
            "unsupported LocalStore schemaVersion {}",
            state.schema_version
        );
        Ok(state)
    }

    fn save_state(&self, state: &LocalStateV1) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(state).context("serialize local state")?;
        atomic_write(&self.state_path(), &bytes)
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
    fn config_is_toml_and_machine_state_is_versioned_json() {
        let root = tempdir().expect("tempdir");
        let store = FileStore::at(root.path());
        let config = store.load_config().expect("config");
        assert!(config.ui.mouse);
        assert_eq!(config.ui.language, LanguagePreference::Auto);
        assert_eq!(config.ui.presentation, PresentationMode::Normal);
        assert_eq!(config.notifications.mode, NotificationMode::Off);
        assert_eq!(config.app_server.active, "local");
        assert!(config.app_server.targets.is_empty());

        let mut state = LocalStateV1::default();
        state.pins.insert("thread-1".into());
        state.host_local_only = true;
        state.repo_backed_only = true;
        store.save_state(&state).expect("save");
        let loaded = store.load_state().expect("load");
        assert_eq!(loaded, state);

        let config_text = fs::read_to_string(store.config_path()).expect("read config");
        let state_text = fs::read_to_string(store.state_path()).expect("read state");
        assert!(config_text.contains("[ui]"));
        assert!(config_text.contains("language = \"auto\""));
        assert!(config_text.contains("presentation = \"normal\""));
        assert!(config_text.contains("[notifications]"));
        assert!(config_text.contains("mode = \"off\""));
        assert!(config_text.contains("[app_server]"));
        assert!(config_text.contains("active = \"local\""));
        assert!(state_text.contains("\"schemaVersion\": 1"));
        assert!(state_text.contains("\"hostLocalOnly\": true"));
        assert!(state_text.contains("\"repoBackedOnly\": true"));

        let legacy = r#"{"schemaVersion":1}"#;
        fs::write(store.state_path(), legacy).expect("write legacy state");
        let legacy_loaded = store.load_state().expect("load legacy state");
        assert!(!legacy_loaded.host_local_only);
        assert!(!legacy_loaded.repo_backed_only);
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

    #[test]
    fn repeated_state_write_replaces_previous_content() {
        let root = tempdir().expect("tempdir");
        let store = FileStore::at(root.path());
        let mut state = LocalStateV1::default();
        store.save_state(&state).expect("first save");
        state.pins.insert("thread-2".into());
        store.save_state(&state).expect("second save");
        assert_eq!(store.load_state().expect("load"), state);
    }
}
