//! Application state, layout persistence, and frame composition.

// UI components are children of app so they can share its private state.
#[path = "ui/mod.rs"]
pub(crate) mod ui;

#[path = "workspace/mod.rs"]
mod workspace;

use workspace::TerminalWorkspace;

use crate::config_picker::ConfigPicker;
use crate::layout_config::{ButtonConfig, Config, DEFAULT_LAYOUT_NAME};
use crate::settings::SettingsStore;
use crate::theme::ThemeSettings;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const UNSAVED_LAYOUT_NOTICE: &str = "Unsaved layout · Load a config or save a new layout";

struct Editor {
    index: usize,
    button: ButtonConfig,
    error: Option<String>,
}

struct SaveAs {
    picker: ConfigPicker,
    config: Config,
    close_editor: bool,
    create_new_layout: bool,
}

pub struct ButtonBuilder {
    config: Config,
    config_path: Option<PathBuf>,
    title_state: Option<(String, bool)>,
    settings: SettingsStore,
    theme: ThemeSettings,
    applied_theme: Option<(ThemeSettings, bool)>,
    config_picker: Option<ConfigPicker>,
    save_as: Option<SaveAs>,
    new_layout_requested: bool,
    new_layout_error: Option<String>,
    layout_name_edit: Option<String>,
    focus_layout_name: bool,
    workspace: TerminalWorkspace,
    editor: Option<Editor>,
    edit_mode: bool,
    dragged_button: Option<usize>,
    appearance_open: bool,
    fonts: Vec<String>,
    dirty: bool,
    notice: String,
    completion_notice: Option<(String, Instant)>,
    error: Option<String>,
}

impl ButtonBuilder {
    pub fn new(ctx: &egui::Context, config_path: Option<PathBuf>, config: Config) -> Self {
        crate::ui::setup(ctx);
        let fonts = crate::fonts::load(ctx);
        let loaded_layout = config_path.is_some();
        let notice = match &config_path {
            Some(path) => format!("Loaded {}", path.display()),
            None => UNSAVED_LAYOUT_NOTICE.into(),
        };
        let mut app = Self {
            config,
            config_path,
            title_state: None,
            settings: SettingsStore::default(),
            theme: ThemeSettings::default(),
            applied_theme: Some((ThemeSettings::default(), true)),
            config_picker: None,
            save_as: None,
            new_layout_requested: false,
            new_layout_error: None,
            layout_name_edit: None,
            focus_layout_name: false,
            workspace: TerminalWorkspace::new(ctx),
            editor: None,
            edit_mode: false,
            dragged_button: None,
            appearance_open: false,
            fonts,
            dirty: false,
            notice,
            completion_notice: None,
            error: None,
        };
        if loaded_layout {
            app.start_auto_buttons();
        }
        app
    }

    pub fn with_settings(mut self, settings: SettingsStore) -> Self {
        self.theme = settings.theme();
        if settings.warning.is_some() {
            self.error = settings.warning.clone();
        }
        self.settings = settings;
        self.remember_layout();
        self
    }

    fn remember_layout(&mut self) {
        if let Some(path) = &self.config_path
            && let Err(error) = self.settings.remember_layout(path)
        {
            log::warn!("{error}");
            self.error = Some(error);
        }
    }

    fn save(&mut self) {
        self.apply_layout_name();
        if let Err(error) = self.save_config(self.config.clone(), false) {
            self.error = Some(error);
        }
    }

    fn save_config(&mut self, config: Config, close_editor: bool) -> Result<(), String> {
        if let Some(path) = &self.config_path {
            config.save(path)?;
            self.finish_save(config, path.clone(), close_editor);
        } else {
            self.save_as = Some(SaveAs {
                picker: ConfigPicker::save(None),
                config,
                close_editor,
                create_new_layout: false,
            });
        }
        Ok(())
    }

    fn finish_save(&mut self, config: Config, path: PathBuf, close_editor: bool) {
        self.config = config;
        self.notice = format!("Saved {}", path.display());
        self.completion_notice = None;
        self.config_path = Some(path);
        self.dirty = false;
        self.error = None;
        if close_editor {
            self.editor = None;
        }
        if let Some(picker) = &mut self.config_picker {
            picker.error = None;
        }
        self.remember_layout();
    }

    fn apply_layout_name(&mut self) {
        let Some(draft) = self.layout_name_edit.take() else {
            return;
        };
        let name = draft.trim();
        let name = if name.is_empty() {
            DEFAULT_LAYOUT_NAME
        } else {
            name
        };
        if self.config.display_name != name {
            self.config.display_name = name.to_owned();
            self.dirty = true;
        }
        self.focus_layout_name = false;
    }

    fn edit(&mut self, index: usize) {
        self.editor = Some(Editor {
            index,
            button: self.config.buttons[index].clone(),
            error: None,
        });
    }

    fn load_config(&mut self, path: PathBuf) -> Result<(), String> {
        let config = Config::load_existing(&path)?;
        self.replace_layout(config, Some(path));
        Ok(())
    }

    fn request_load_config(&mut self, path: PathBuf) {
        if self.has_unsaved_changes() {
            self.config_picker = Some(ConfigPicker::new(Some(&path)));
        } else if let Err(error) = self.load_config(path) {
            self.error = Some(error);
        }
    }

    fn replace_layout(&mut self, config: Config, path: Option<PathBuf>) {
        self.config = config;
        self.completion_notice = None;
        self.notice = match &path {
            Some(path) => format!("Loaded {}", path.display()),
            None => UNSAVED_LAYOUT_NOTICE.into(),
        };
        self.config_path = path;
        self.config_picker = None;
        self.save_as = None;
        self.new_layout_requested = false;
        self.new_layout_error = None;
        self.layout_name_edit = None;
        self.focus_layout_name = false;
        self.editor = None;
        self.appearance_open = false;
        self.edit_mode = false;
        self.dragged_button = None;
        self.workspace.detach_buttons();
        self.dirty = false;
        self.error = None;
        self.remember_layout();
        if self.config_path.is_some() {
            self.start_auto_buttons();
        }
    }

    fn has_unsaved_changes(&self) -> bool {
        self.dirty || self.editor.is_some() || self.layout_name_edit.is_some()
    }

    fn layout_display_label(&self) -> String {
        format!(
            "{}{}",
            self.config.display_name(),
            if self.dirty { " *" } else { "" }
        )
    }

    fn request_new_layout(&mut self) {
        self.new_layout_error = None;
        if self.has_unsaved_changes() {
            self.new_layout_requested = true;
        } else {
            self.create_new_layout();
        }
    }

    fn create_new_layout(&mut self) {
        self.replace_layout(Config::default(), None);
    }

    fn save_before_new_layout(&mut self) {
        self.apply_layout_name();
        let mut config = self.config.clone();
        if let Some(editor) = &self.editor {
            if let Err(error) = validate_button(&editor.button) {
                self.new_layout_error = Some(error);
                return;
            }
            config.buttons[editor.index] = editor.button.clone();
        }
        match self.save_config(config, true) {
            Ok(()) => {
                if let Some(save_as) = &mut self.save_as {
                    save_as.create_new_layout = true;
                } else {
                    self.create_new_layout();
                }
            }
            Err(error) => self.new_layout_error = Some(error),
        }
    }

    fn launch(&mut self, index: usize) {
        if !self.config.buttons[index].command.trim().is_empty()
            && self.workspace.focus_running_button(index)
        {
            return;
        }
        self.launch_new(index);
    }

    fn launch_new(&mut self, index: usize) {
        if self.config.buttons[index].command.trim().is_empty() {
            self.edit(index);
            return;
        }
        match self
            .workspace
            .start_button(index, &self.config.buttons[index], &self.config.terminal)
        {
            Ok(()) => {
                self.completion_notice = None;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn start_auto_buttons(&mut self) {
        let buttons: Vec<_> = self
            .config
            .buttons
            .iter()
            .take(self.config.visible_buttons())
            .enumerate()
            .filter_map(|(index, button)| {
                (button.auto_start && !button.command.trim().is_empty()).then_some(index)
            })
            .collect();
        let mut errors = Vec::new();
        for index in buttons {
            self.launch(index);
            if let Some(error) = self.error.take() {
                errors.push(error);
            }
        }
        if !errors.is_empty() {
            self.error = Some(errors.join(" · "));
        }
    }

    fn poll_terminals(&mut self, ctx: &egui::Context) {
        if self
            .completion_notice
            .as_ref()
            .is_some_and(|(_, until)| Instant::now() >= *until)
        {
            self.completion_notice = None;
        }
        if let Some(label) = self.workspace.poll(ctx) {
            self.completion_notice = Some((
                format!("{label} completed successfully"),
                Instant::now() + Duration::from_secs(4),
            ));
        }
    }

    fn show(&mut self, ctx: &egui::Context) {
        self.sync_theme(ctx);
        self.poll_terminals(ctx);
        self.handle_shortcuts(ctx);
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| self.footer(ui));
        egui::SidePanel::left("buttons")
            .resizable(true)
            .default_width(330.0)
            .width_range(290.0..=500.0)
            .show(ctx, |ui| {
                self.buttons_panel(ui);
            });
        egui::CentralPanel::default().show(ctx, |ui| self.terminal_workspace(ui));
        if self.save_as.is_none() && !self.new_layout_requested {
            self.editor_window(ctx);
        }
        if self.save_as.is_none() && !self.new_layout_requested {
            self.appearance_window(ctx);
        }
        if self.save_as.is_none() && !self.new_layout_requested {
            self.config_picker_window(ctx);
        }
        if self.save_as.is_none() {
            self.new_layout_window(ctx);
        }
        self.save_as_window(ctx);
        let display_name = self.config.display_name();
        if self
            .title_state
            .as_ref()
            .is_none_or(|(name, dirty)| name != display_name || *dirty != self.dirty)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                "Button Builder - {}",
                self.layout_display_label()
            )));
            self.title_state = Some((display_name.to_owned(), self.dirty));
        }
        self.workspace.schedule_repaint(ctx);
        if let Some((_, until)) = self.completion_notice {
            ctx.request_repaint_after(until.saturating_duration_since(Instant::now()));
        }
    }
}

impl eframe::App for ButtonBuilder {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show(ctx);
    }
}

fn validate_button(button: &ButtonConfig) -> Result<(), String> {
    if button.label.trim().is_empty() {
        Err("Enter a label for the button.".into())
    } else if !button.command.trim().is_empty() {
        button.args().map(|_| ())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
