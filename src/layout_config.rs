//! Button layout definitions, terminal appearance, and YAML persistence.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_GRID_ROWS: usize = 12;
pub const MAX_GRID_COLUMNS: usize = 8;
pub const DEFAULT_LAYOUT_NAME: &str = "Your buttons";

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ButtonConfig {
    pub label: String,
    pub command: String,
    pub arguments: String,
    pub working_directory: String,
    pub shell: bool,
    pub log_output: bool,
    pub fg_color: Option<[u8; 3]>,
    pub bg_color: Option<[u8; 3]>,
}

impl Default for ButtonConfig {
    fn default() -> Self {
        Self {
            label: "New button".into(),
            command: String::new(),
            arguments: String::new(),
            working_directory: String::new(),
            shell: false,
            log_output: false,
            fg_color: None,
            bg_color: None,
        }
    }
}

impl ButtonConfig {
    pub fn args(&self) -> Result<Vec<String>, String> {
        if self.command.trim().is_empty() {
            return Err("Enter a program to run.".into());
        }
        if self.command.contains('\0') || self.arguments.contains('\0') {
            return Err("Commands and arguments cannot contain a NUL character.".into());
        }
        if self.shell {
            return Ok(vec!["-c".into(), self.command.clone()]);
        }
        shlex::split(&self.arguments)
            .ok_or_else(|| "Arguments contain an unclosed quote or trailing escape.".into())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalStyle {
    pub font: String,
    pub font_size: f32,
    pub line_spacing: f32,
    pub fg_color: [u8; 3],
    pub bg_color: [u8; 3],
}

impl Default for TerminalStyle {
    fn default() -> Self {
        Self {
            font: "Monospace".into(),
            font_size: 14.0,
            line_spacing: 1.15,
            fg_color: [220, 227, 238],
            bg_color: [15, 19, 27],
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub display_name: String,
    pub rows: usize,
    pub columns: usize,
    pub buttons: Vec<ButtonConfig>,
    pub terminal: TerminalStyle,
}

impl Default for Config {
    fn default() -> Self {
        let buttons: Vec<_> = (0..9)
            .map(|index| ButtonConfig {
                label: format!("Button {}", index + 1),
                ..Default::default()
            })
            .collect();
        Self {
            display_name: DEFAULT_LAYOUT_NAME.into(),
            rows: 3,
            columns: 3,
            buttons,
            terminal: TerminalStyle::default(),
        }
    }
}

impl Config {
    pub fn display_name(&self) -> &str {
        let name = self.display_name.trim();
        if name.is_empty() {
            DEFAULT_LAYOUT_NAME
        } else {
            name
        }
    }

    pub fn visible_buttons(&self) -> usize {
        self.rows * self.columns
    }

    /// Keep hidden definitions so reducing the grid never discards a user's actions.
    pub fn resize_grid(&mut self, rows: usize, columns: usize) -> Result<(), String> {
        if !(1..=MAX_GRID_ROWS).contains(&rows) || !(1..=MAX_GRID_COLUMNS).contains(&columns) {
            return Err(format!(
                "Grid dimensions must be 1–{MAX_GRID_ROWS} rows and 1–{MAX_GRID_COLUMNS} columns."
            ));
        }
        self.rows = rows;
        self.columns = columns;
        while self.buttons.len() < self.visible_buttons() {
            self.buttons.push(ButtonConfig {
                label: format!("Button {}", self.buttons.len() + 1),
                ..Default::default()
            });
        }
        Ok(())
    }

    /// Loading a missing or invalid file must fail instead of substituting a layout.
    pub fn load_existing(path: &Path) -> Result<Self, String> {
        let source = fs::read_to_string(path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        Self::parse(&source, path)
    }

    fn parse(source: &str, path: &Path) -> Result<Self, String> {
        let mut config: Self = serde_yaml_ng::from_str(source)
            .map_err(|error| format!("Invalid configuration in {}: {error}", path.display()))?;
        config.resize_grid(config.rows, config.columns)?;
        if !config.terminal.font_size.is_finite() || !config.terminal.line_spacing.is_finite() {
            return Err("Terminal font size and line spacing must be finite numbers.".into());
        }
        config.terminal.font_size = config.terminal.font_size.clamp(8.0, 32.0);
        config.terminal.line_spacing = config.terminal.line_spacing.clamp(1.0, 1.8);
        Ok(config)
    }

    /// Write beside the destination, then atomically replace it after the write succeeds.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        save_yaml(self, path)
    }
}

/// Atomically persist YAML for both layouts and application preferences.
pub(crate) fn save_yaml(value: &impl Serialize, path: &Path) -> Result<(), String> {
    let source = serde_yaml_ng::to_string(value).map_err(|error| error.to_string())?;
    static NEXT_SAVE: AtomicU64 = AtomicU64::new(0);
    let temp = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        NEXT_SAVE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(source.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    result.map_err(|error| format!("Could not save {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_preserve_quoting_without_shell_expansion() {
        let button = ButtonConfig {
            command: "/usr/bin/printf".into(),
            arguments: r#"'%s\n' 'two words' '$HOME' ';'"#.into(),
            ..Default::default()
        };
        assert_eq!(button.args().unwrap(), ["%s\\n", "two words", "$HOME", ";"]);
        assert!(
            ButtonConfig {
                arguments: "'unclosed".into(),
                ..button
            }
            .args()
            .is_err()
        );
    }

    #[test]
    fn color_keys_round_trip_with_the_new_names() {
        let source = "rows: 1\ncolumns: 1\nbuttons:\n  - label: Custom colors\n    fg_color: [12, 34, 56]\n    bg_color: [78, 90, 123]\n    log_output: true\nterminal:\n  fg_color: [210, 220, 230]\n  bg_color: [5, 10, 15]\n";
        let config = Config::parse(source, Path::new("custom-colors.yaml")).unwrap();
        assert_eq!(config.buttons[0].fg_color, Some([12, 34, 56]));
        assert_eq!(config.buttons[0].bg_color, Some([78, 90, 123]));
        assert_eq!(config.terminal.fg_color, [210, 220, 230]);
        assert_eq!(config.terminal.bg_color, [5, 10, 15]);
        assert!(config.buttons[0].log_output);

        let saved = serde_yaml_ng::to_string(&config).unwrap();
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&saved).unwrap();
        for colors in [&value["buttons"][0], &value["terminal"]] {
            assert!(colors.get("fg_color").is_some());
            assert!(colors.get("bg_color").is_some());
            assert!(colors.get("foreground").is_none());
            assert!(colors.get("background").is_none());
        }
        let loaded = Config::parse(&saved, Path::new("renamed-colors.yaml")).unwrap();
        assert_eq!(loaded.buttons[0].fg_color, config.buttons[0].fg_color);
        assert_eq!(loaded.buttons[0].bg_color, config.buttons[0].bg_color);
        assert_eq!(loaded.terminal.fg_color, config.terminal.fg_color);
        assert_eq!(loaded.terminal.bg_color, config.terminal.bg_color);
    }

    #[test]
    fn yaml_round_trip_preserves_all_nine_buttons_and_style() {
        let mut config = Config {
            display_name: "System Monitor · Local".into(),
            ..Default::default()
        };
        config.buttons[8].label = "Last button".into();
        config.buttons[8].log_output = true;
        config.terminal.font_size = 22.0;
        let decoded: Config =
            serde_yaml_ng::from_str(&serde_yaml_ng::to_string(&config).unwrap()).unwrap();
        assert!(decoded.buttons[0].command.is_empty());
        assert_eq!(decoded.display_name, "System Monitor · Local");
        assert_eq!(decoded.buttons[8].label, "Last button");
        assert!(decoded.buttons[8].log_output);
        assert!(!decoded.buttons[0].log_output);
        assert_eq!(decoded.terminal.font_size, 22.0);
        assert_eq!((decoded.rows, decoded.columns), (3, 3));
    }

    #[test]
    fn resizing_preserves_hidden_actions_and_persists_dimensions() {
        let mut config = Config::default();
        config.resize_grid(4, 5).unwrap();
        config.buttons[19].label = "Keep me".into();
        config.buttons[19].command = "/usr/bin/top".into();
        config.buttons[19].fg_color = Some([240, 230, 220]);
        config.buttons[19].bg_color = Some([30, 40, 50]);
        config.resize_grid(1, 2).unwrap();
        let path = std::env::temp_dir().join(format!(
            "rs-button-builder-grid-test-{}.yaml",
            std::process::id()
        ));
        config.save(&path).unwrap();
        let mut loaded = Config::load_existing(&path).unwrap();
        assert_eq!(
            (loaded.rows, loaded.columns, loaded.visible_buttons()),
            (1, 2, 2)
        );
        loaded.resize_grid(4, 5).unwrap();
        assert_eq!(loaded.buttons[19].label, "Keep me");
        assert_eq!(loaded.buttons[19].command, "/usr/bin/top");
        assert_eq!(loaded.buttons[19].fg_color, Some([240, 230, 220]));
        assert_eq!(loaded.buttons[19].bg_color, Some([30, 40, 50]));
        assert!(loaded.resize_grid(0, 3).is_err());
        assert!(loaded.resize_grid(3, MAX_GRID_COLUMNS + 1).is_err());
    }

    #[test]
    fn old_layouts_default_to_three_by_three() {
        let mut config: Config =
            serde_yaml_ng::from_str("buttons:\n  - label: Existing\n    command: /usr/bin/top\n")
                .unwrap();
        config.resize_grid(config.rows, config.columns).unwrap();
        assert_eq!((config.rows, config.columns), (3, 3));
        assert_eq!(config.buttons.len(), 9);
        assert_eq!(config.buttons[0].label, "Existing");
        assert!(config.buttons.iter().all(|button| !button.log_output));
        assert_eq!(config.display_name(), DEFAULT_LAYOUT_NAME);
        config.display_name = "   ".into();
        assert_eq!(config.display_name(), DEFAULT_LAYOUT_NAME);
    }
}
