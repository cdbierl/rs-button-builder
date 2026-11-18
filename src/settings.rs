//! Local application preferences and startup layout selection.

use crate::layout_config::{Config, save_yaml};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    last_layout: Option<PathBuf>,
}

/// An absent destination disables persistence, useful for isolated app instances.
#[derive(Default)]
pub struct SettingsStore {
    path: Option<PathBuf>,
    settings: Settings,
    pub warning: Option<String>,
}

impl SettingsStore {
    pub fn load() -> Self {
        match settings_path(
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("HOME"),
        ) {
            Some(path) => Self::open(path),
            None => Self {
                warning: Some(
                    "Could not locate the app settings directory; set HOME or XDG_CONFIG_HOME."
                        .into(),
                ),
                ..Default::default()
            },
        }
    }

    pub fn open(path: PathBuf) -> Self {
        let result = match fs::read_to_string(&path) {
            Ok(source) => serde_yaml_ng::from_str(&source)
                .map_err(|error| format!("Invalid app settings in {}: {error}", path.display())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Settings::default()),
            Err(error) => Err(format!(
                "Could not read app settings from {}: {error}",
                path.display()
            )),
        };
        let (settings, warning) = match result {
            Ok(settings) => (settings, None),
            Err(error) => (Settings::default(), Some(error)),
        };
        Self {
            path: Some(path),
            settings,
            warning,
        }
    }

    /// An explicit file is strict; a stale remembered file leaves the GUI usable.
    pub fn startup_layout(
        &mut self,
        explicit: Option<PathBuf>,
    ) -> Result<(Option<PathBuf>, Config), String> {
        if let Some(path) = explicit {
            let config = Config::load_existing(&path)?;
            return Ok((Some(path), config));
        }
        if let Some(path) = &self.settings.last_layout {
            match Config::load_existing(path) {
                Ok(config) => return Ok((Some(path.clone()), config)),
                Err(error) => {
                    self.warning = Some(format!(
                        "Could not reopen the last layout. {error} Load another config to continue."
                    ));
                }
            }
        }
        Ok((None, Config::default()))
    }

    /// Store absolute paths so startup does not depend on the working directory.
    pub fn remember_layout(&mut self, layout: &Path) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let layout = std::path::absolute(layout).map_err(|error| {
            format!(
                "Could not resolve layout path {}: {error}",
                layout.display()
            )
        })?;
        if self.settings.last_layout.as_ref() == Some(&layout) && path.is_file() {
            return Ok(());
        }
        let mut settings = self.settings.clone();
        settings.last_layout = Some(layout);
        let parent = path
            .parent()
            .ok_or_else(|| "The app settings file needs a parent directory.".to_owned())?;
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create app settings directory {}: {error}",
                parent.display()
            )
        })?;
        save_yaml(&settings, path).map_err(|error| {
            format!("The layout is open, but app settings were not saved. {error}")
        })?;
        self.settings = settings;
        Ok(())
    }
}

fn settings_path(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let root = xdg
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(".config"))
        })?;
    Some(root.join("rs_button_builder/settings.yaml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "rs-button-builder-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn layout(directory: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(directory).unwrap();
        let path = directory.join(format!("{name}.yaml"));
        Config {
            display_name: name.into(),
            ..Default::default()
        }
        .save(&path)
        .unwrap();
        path
    }

    #[test]
    fn settings_use_the_app_subdirectory_and_honor_absolute_xdg_paths() {
        let home = Some(OsString::from("/home/example"));
        let default = PathBuf::from("/home/example/.config/rs_button_builder/settings.yaml");
        assert_eq!(settings_path(None, home.clone()), Some(default.clone()));
        assert_eq!(
            settings_path(Some("/tmp/preferences".into()), home.clone()),
            Some("/tmp/preferences/rs_button_builder/settings.yaml".into())
        );
        for xdg in ["", "relative"] {
            assert_eq!(
                settings_path(Some(xdg.into()), home.clone()),
                Some(default.clone())
            );
        }
        assert!(settings_path(None, None).is_none());
    }

    #[test]
    fn first_start_is_empty_and_does_not_create_settings_until_a_layout_is_selected() {
        let path = directory().join("rs_button_builder/settings.yaml");
        let mut store = SettingsStore::open(path.clone());
        let (layout, config) = store.startup_layout(None).unwrap();
        assert!(layout.is_none() && config.buttons[0].command.is_empty());
        assert!(store.warning.is_none() && !path.exists());
    }

    #[test]
    fn remembered_relative_paths_are_absolute_and_round_trip_on_startup() {
        let path = directory().join("rs_button_builder/settings.yaml");
        let relative = Path::new("examples/system-monitor.yaml");
        SettingsStore::open(path.clone())
            .remember_layout(relative)
            .unwrap();
        let mut store = SettingsStore::open(path);
        let (loaded, config) = store.startup_layout(None).unwrap();
        assert_eq!(loaded, Some(std::path::absolute(relative).unwrap()));
        assert_eq!(config.display_name(), "System Monitor");
        assert!(store.warning.is_none());
    }

    #[test]
    fn explicit_file_overrides_the_remembered_layout_and_remains_strict() {
        let directory = directory();
        let remembered = layout(&directory, "Last layout");
        let explicit = layout(&directory, "Requested layout");
        let settings = directory.join("rs_button_builder/settings.yaml");
        let mut store = SettingsStore::open(settings.clone());
        store.remember_layout(&remembered).unwrap();
        let (path, config) = store.startup_layout(Some(explicit.clone())).unwrap();
        assert_eq!(path, Some(explicit));
        assert_eq!(config.display_name(), "Requested layout");
        assert!(
            store
                .startup_layout(Some(directory.join("missing.yaml")))
                .is_err()
        );
        let invalid = directory.join("invalid.yaml");
        fs::write(&invalid, "rows: invalid").unwrap();
        assert!(store.startup_layout(Some(invalid)).is_err());
        assert_eq!(
            SettingsStore::open(settings).settings.last_layout,
            Some(remembered)
        );
    }

    #[test]
    fn missing_or_invalid_remembered_layout_falls_back_without_replacing_the_reference() {
        let directory = directory();
        fs::create_dir_all(&directory).unwrap();
        let invalid = directory.join("invalid.yaml");
        fs::write(&invalid, "rows: invalid").unwrap();
        let settings = directory.join("rs_button_builder/settings.yaml");
        for stale in [directory.join("missing.yaml"), invalid] {
            let mut store = SettingsStore::open(settings.clone());
            store.remember_layout(&stale).unwrap();
            let (path, config) = store.startup_layout(None).unwrap();
            assert!(path.is_none() && config.buttons[0].command.is_empty());
            assert!(
                store
                    .warning
                    .as_deref()
                    .unwrap()
                    .contains("Could not reopen")
            );
            assert_eq!(
                SettingsStore::open(settings.clone()).settings.last_layout,
                Some(stale)
            );
        }
    }

    #[test]
    fn malformed_settings_are_recoverable_and_missing_fields_use_defaults() {
        let directory = directory();
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.yaml");
        fs::write(&path, "last_layout: [not, a, path]").unwrap();
        let mut store = SettingsStore::open(path.clone());
        assert!(store.warning.is_some());
        assert!(store.startup_layout(None).unwrap().0.is_none());
        fs::write(&path, "{}").unwrap();
        let mut store = SettingsStore::open(path);
        assert!(store.warning.is_none());
        assert!(store.startup_layout(None).unwrap().0.is_none());
    }

    #[test]
    fn failed_settings_write_does_not_adopt_the_new_layout() {
        let directory = directory();
        let original = layout(&directory, "Original");
        let mut store = SettingsStore::open(directory.join("settings.yaml"));
        store.remember_layout(&original).unwrap();
        let blocked = directory.join("blocked");
        fs::write(&blocked, "not a directory").unwrap();
        store.path = Some(blocked.join("settings.yaml"));
        assert!(
            store
                .remember_layout(&directory.join("Other.yaml"))
                .is_err()
        );
        assert_eq!(store.settings.last_layout, Some(original.clone()));
        assert_eq!(
            SettingsStore::open(directory.join("settings.yaml"))
                .settings
                .last_layout,
            Some(original)
        );
    }
}
