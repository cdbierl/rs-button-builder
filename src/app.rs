//! Application state, layout persistence, terminal lifecycle, and frame composition.

// UI components are children of app so they can share its private state.
#[path = "ui/mod.rs"]
pub(crate) mod ui;

use crate::config_picker::ConfigPicker;
use crate::layout_config::{ButtonConfig, Config, DEFAULT_LAYOUT_NAME, TerminalStyle};
use crate::settings::SettingsStore;
use crate::terminal::Terminal;
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

struct TerminalTab {
    id: u64,
    pane: usize,
    label: String,
    command: String,
    button_index: Option<usize>,
    terminal: Terminal,
    style: TerminalStyle,
    scrollback: usize,
    scroll_fraction: f32,
    selection: ui::TerminalSelection,
}

impl TerminalTab {
    fn poll(&mut self) {
        let previous = self.terminal.screen.scrolled_lines;
        self.terminal.poll();
        self.update_view(previous);
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        let previous = self.terminal.screen.scrolled_lines;
        self.terminal.resize(cols, rows);
        self.update_view(previous);
    }

    fn update_view(&mut self, previous_scrolled_lines: usize) {
        self.selection.prepare(&self.terminal.screen);
        // Output and viewport resizing can both move the viewed text into history.
        if self.scrollback > 0 {
            self.scrollback = self.scrollback.saturating_add(
                self.terminal
                    .screen
                    .scrolled_lines
                    .saturating_sub(previous_scrolled_lines),
            );
        }
    }

    fn copy_text(&self) -> String {
        self.selection
            .text(&self.terminal.screen)
            .unwrap_or_else(|| self.terminal.screen.text())
    }
}

pub struct ButtonBuilder {
    config: Config,
    config_path: Option<PathBuf>,
    title_display_name: Option<String>,
    settings: SettingsStore,
    theme: ThemeSettings,
    applied_theme: Option<(ThemeSettings, bool)>,
    config_picker: Option<ConfigPicker>,
    save_as: Option<SaveAs>,
    new_layout_requested: bool,
    new_layout_error: Option<String>,
    layout_name_edit: Option<String>,
    focus_layout_name: bool,
    tabs: Vec<TerminalTab>,
    panes: ui::TerminalPanes,
    active_tab: Option<usize>,
    next_tab_id: u64,
    next_poll_tab: usize,
    reveal_active_tab: bool,
    editor: Option<Editor>,
    edit_mode: bool,
    dragged_button: Option<usize>,
    appearance_open: bool,
    fonts: Vec<String>,
    dirty: bool,
    notice: String,
    completion_notice: Option<(String, Instant)>,
    error: Option<String>,
    terminal_size: (usize, usize),
    focus_terminal: bool,
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
            title_display_name: None,
            settings: SettingsStore::default(),
            theme: ThemeSettings::default(),
            applied_theme: Some((ThemeSettings::default(), true)),
            config_picker: None,
            save_as: None,
            new_layout_requested: false,
            new_layout_error: None,
            layout_name_edit: None,
            focus_layout_name: false,
            tabs: Vec::new(),
            panes: ui::TerminalPanes::default(),
            active_tab: None,
            next_tab_id: 1,
            next_poll_tab: 0,
            reveal_active_tab: false,
            editor: None,
            edit_mode: false,
            dragged_button: None,
            appearance_open: false,
            fonts,
            dirty: false,
            notice,
            completion_notice: None,
            error: None,
            terminal_size: (80, 24),
            focus_terminal: false,
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

    fn running(&self) -> bool {
        self.tabs.iter().any(|tab| tab.terminal.is_running())
    }

    fn active(&self) -> Option<&TerminalTab> {
        self.active_tab.and_then(|index| self.tabs.get(index))
    }

    fn select_tab(&mut self, index: usize) {
        self.active_tab = Some(index);
        let tab = &self.tabs[index];
        self.panes.focused = tab.pane;
        self.panes.selected[tab.pane] = Some(tab.id);
        self.reveal_active_tab = true;
        self.focus_terminal = true;
    }

    fn close_tab(&mut self, index: usize) {
        let active_closed = self.active_tab == Some(index);
        let pane = self.tabs[index].pane;
        // Dropping a terminal requests termination; reaping happens off the UI thread.
        self.tabs.remove(index);
        self.active_tab = match self.active_tab {
            _ if self.tabs.is_empty() => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) if active == index => Some(index.min(self.tabs.len() - 1)),
            active => active,
        };
        if active_closed {
            if self.panes.split.is_some() {
                self.active_tab = self
                    .tabs
                    .iter()
                    .enumerate()
                    .find(|(other, tab)| *other >= index && tab.pane == pane)
                    .or_else(|| {
                        self.tabs
                            .iter()
                            .enumerate()
                            .rev()
                            .find(|(_, tab)| tab.pane == pane)
                    })
                    .map(|(index, _)| index)
                    .or(self.active_tab);
            }
            self.focus_terminal = true;
            self.reveal_active_tab = true;
        }
        self.normalize_panes();
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
        for tab in &mut self.tabs {
            tab.button_index = None;
        }
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
        if self.config.buttons[index].command.trim().is_empty() {
            self.edit(index);
            return;
        }
        if let Some(tab) = self
            .tabs
            .iter()
            .position(|tab| tab.button_index == Some(index) && tab.terminal.is_running())
        {
            self.select_tab(tab);
            return;
        }
        match Terminal::start(
            &self.config.buttons[index],
            self.terminal_size.0,
            self.terminal_size.1,
        ) {
            Ok(terminal) => {
                self.completion_notice = None;
                let tab = TerminalTab {
                    id: self.next_tab_id,
                    pane: self.panes.focused,
                    label: self.config.buttons[index].label.clone(),
                    command: self.config.buttons[index].command.clone(),
                    button_index: Some(index),
                    terminal,
                    style: self.config.terminal.clone(),
                    scrollback: 0,
                    scroll_fraction: 0.0,
                    selection: ui::TerminalSelection::default(),
                };
                self.next_tab_id += 1;
                self.tabs.push(tab);
                self.select_tab(self.tabs.len() - 1);
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
        // Share a frame budget and rotate the starting tab so noisy tabs cannot starve others.
        let deadline = Instant::now() + Duration::from_millis(4);
        let count = self.tabs.len();
        for _ in 0..count {
            if Instant::now() >= deadline {
                break;
            }
            let index = self.next_poll_tab % count;
            self.tabs[index].poll();
            self.next_poll_tab = (index + 1) % count;
        }
        // Remove only after polling has observed success and drained all terminal output.
        for index in (0..self.tabs.len()).rev() {
            if self.tabs[index].terminal.should_auto_close() {
                self.completion_notice = Some((
                    format!("{} completed successfully", self.tabs[index].label),
                    Instant::now() + Duration::from_secs(4),
                ));
                self.close_tab(index);
                ctx.request_repaint();
            }
        }
    }

    fn show(&mut self, ctx: &egui::Context) {
        self.sync_theme(ctx);
        self.poll_terminals(ctx);
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
        if self.title_display_name.as_deref() != Some(display_name) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                "Button Builder - {display_name}"
            )));
            self.title_display_name = Some(display_name.to_owned());
        }
        if self.tabs.iter().any(|tab| tab.terminal.needs_poll()) {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
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
